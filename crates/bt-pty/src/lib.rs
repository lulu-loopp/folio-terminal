//! ConPTY ownership and the bounded PTY-to-Term transport from DESIGN.md §1.3.

use std::{
    collections::VecDeque,
    ffi::{OsStr, OsString},
    fs::File,
    io::{Read, Write},
    num::{NonZeroU16, NonZeroUsize},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, MutexGuard},
    thread::JoinHandle,
    time::Instant,
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
const PTY_DUMP_ENV: &str = "BT_PTY_DUMP";

/// Diagnostic-only byte sink for one ConPTY reader. The main file is byte-for-byte suitable for
/// `BT_PROBE_INPUT`; the adjacent `.chunks` file preserves reader arrival boundaries and timing.
struct PtyDump {
    bytes: File,
    chunks: File,
    started: Instant,
    sequence: u64,
}

impl PtyDump {
    fn from_environment() -> Result<Option<Self>, PtyError> {
        let Some(path) = std::env::var_os(PTY_DUMP_ENV) else {
            return Ok(None);
        };
        Self::create(&PathBuf::from(path)).map(Some)
    }

    fn create(path: &Path) -> Result<Self, PtyError> {
        let bytes = File::create(path)?;
        let mut chunks = File::create(pty_dump_chunks_path(path))?;
        writeln!(chunks, "# BT_PTY_DUMP_CHUNKS_V1 sequence elapsed_us bytes")?;
        Ok(Self {
            bytes,
            chunks,
            started: Instant::now(),
            sequence: 0,
        })
    }

    fn write_chunk(&mut self, chunk: &[u8]) -> std::io::Result<()> {
        self.bytes.write_all(chunk)?;
        let elapsed_us = u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX);
        writeln!(
            self.chunks,
            "{} {elapsed_us} {}",
            self.sequence,
            chunk.len()
        )?;
        self.sequence = self.sequence.saturating_add(1);
        Ok(())
    }

    /// Interleave a resize marker with the byte chunks so a replay can apply the new dimensions
    /// at the exact point they took effect. The `#` prefix keeps older manifest parsers working.
    fn write_resize(&mut self, columns: u16, rows: u16) -> std::io::Result<()> {
        let elapsed_us = u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX);
        writeln!(self.chunks, "# RESIZE {columns} {rows} {elapsed_us}")
    }
}

fn pty_dump_chunks_path(path: &Path) -> PathBuf {
    let mut chunks = path.as_os_str().to_os_string();
    chunks.push(".chunks");
    PathBuf::from(chunks)
}

fn read_pty_output(
    reader: &mut dyn Read,
    output: &OutputRing,
    wake: &OutputWake,
    dump: Option<Arc<Mutex<PtyDump>>>,
) {
    if let Some(dump) = dump {
        read_pty_output_with_dump(reader, output, wake, dump);
    } else {
        read_pty_output_without_dump(reader, output, wake);
    }
}

/// Keep the normal reader loop free of dump branches, clocks, allocations, and file operations.
fn read_pty_output_without_dump(reader: &mut dyn Read, output: &OutputRing, wake: &OutputWake) {
    let mut buffer = [0_u8; READER_CHUNK_BYTES];
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        if output.push(buffer[..count].to_vec()).is_err() {
            break;
        }
        wake();
    }
}

fn read_pty_output_with_dump(
    reader: &mut dyn Read,
    output: &OutputRing,
    wake: &OutputWake,
    dump: Arc<Mutex<PtyDump>>,
) {
    let mut buffer = [0_u8; READER_CHUNK_BYTES];
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        let write_result = dump
            .lock()
            .map_err(|_| std::io::Error::other("dump mutex poisoned"))
            .and_then(|mut dump| dump.write_chunk(&buffer[..count]));
        if let Err(error) = write_result {
            eprintln!("BT_PTY_DUMP disabled after write failure: {error}");
            if output.push(buffer[..count].to_vec()).is_ok() {
                wake();
                read_pty_output_without_dump(reader, output, wake);
            }
            return;
        }
        if output.push(buffer[..count].to_vec()).is_err() {
            break;
        }
        wake();
    }
}

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
    pub environment: Vec<(OsString, OsString)>,
    declare_color_support: bool,
}

impl PtyCommand {
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            arguments: Vec::new(),
            working_directory: None,
            environment: Vec::new(),
            declare_color_support: false,
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

    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        let key = key.into();
        let value = value.into();
        if let Some((_, existing_value)) = self
            .environment
            .iter_mut()
            .find(|(existing_key, _)| environment_key_eq(existing_key, &key))
        {
            *existing_value = value;
        } else {
            self.environment.push((key, value));
        }
        self
    }

    pub fn powershell() -> Self {
        let mut command = Self::new("powershell.exe").arg("-NoLogo");
        command.declare_color_support = true;
        command
    }

    fn command_has_no_color(&self) -> bool {
        self.environment
            .iter()
            .any(|(key, _)| environment_key_eq(key, OsStr::new("NO_COLOR")))
    }

    /// Color-capable interactive shells receive `COLORTERM`/`TERM` declarations merged with the
    /// caller's environment (explicit values win). A caller that explicitly sets `NO_COLOR` opts
    /// out and gets no declarations. An *inherited* `NO_COLOR` does not suppress them — it is
    /// stripped instead, see `strips_inherited_no_color`.
    fn resolved_environment(&self) -> Vec<(OsString, OsString)> {
        let command_has_no_color = self.command_has_no_color();
        let mut environment = Vec::with_capacity(self.environment.len() + 2);
        if self.declare_color_support && !command_has_no_color {
            if !self
                .environment
                .iter()
                .any(|(key, _)| environment_key_eq(key, OsStr::new("COLORTERM")))
            {
                environment.push(("COLORTERM".into(), "truecolor".into()));
            }
            if !self
                .environment
                .iter()
                .any(|(key, _)| environment_key_eq(key, OsStr::new("TERM")))
            {
                environment.push(("TERM".into(), "xterm-256color".into()));
            }
        }
        environment.extend(self.environment.iter().cloned());
        environment
    }

    /// A color-capable interactive shell must not inherit a `NO_COLOR` that was aimed at the
    /// terminal process itself: `NO_COLOR` mutes programs that *emit* ANSI color, but the terminal
    /// *renders* it, so an inherited value is launch noise, not the user's intent for this session.
    /// Strip it so the shell sees a color-capable environment. A caller that explicitly sets
    /// `NO_COLOR`, or the user's own shell profile, still opts out (the profile runs inside the
    /// shell, after this strip, and wins).
    fn strips_inherited_no_color(&self) -> bool {
        self.declare_color_support && !self.command_has_no_color()
    }
}

fn environment_key_eq(left: &OsStr, right: &OsStr) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
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
    /// Shared with the reader thread so `resize` can interleave `# RESIZE` markers with chunks.
    dump: Option<Arc<Mutex<PtyDump>>>,
}

impl PtySession {
    pub fn spawn_default(size: PtySize, wake: OutputWake) -> Result<Self, PtyError> {
        let command = PtyCommand::powershell()
            .working_directory(std::env::current_dir().map_err(PtyError::Io)?);
        Self::spawn(command, size, wake)
    }

    pub fn spawn(command: PtyCommand, size: PtySize, wake: OutputWake) -> Result<Self, PtyError> {
        let dump = PtyDump::from_environment()?.map(|dump| Arc::new(Mutex::new(dump)));
        let conpty_source = conpty_source();
        let strip_inherited_no_color = command.strips_inherited_no_color();
        let environment = command.resolved_environment();
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
        // Drop an inherited NO_COLOR before layering our declarations so the shell starts in a
        // color-capable environment regardless of how the terminal itself was launched.
        if strip_inherited_no_color {
            builder.env_remove("NO_COLOR");
        }
        for (key, value) in environment {
            builder.env(key, value);
        }
        let child = pair.slave.spawn_command(builder).map_err(backend)?;
        drop(pair.slave);
        let writer = pair.master.take_writer().map_err(backend)?;
        let mut reader = pair.master.try_clone_reader().map_err(backend)?;
        let output = Arc::new(OutputRing::new(PTY_RING_BYTES));
        let reader_output = Arc::clone(&output);
        let reader_dump = dump.clone();
        let reader_thread = std::thread::spawn(move || {
            read_pty_output(reader.as_mut(), &reader_output, &wake, reader_dump);
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
            dump,
        })
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<(), PtyError> {
        let writer = self.writer.as_mut().ok_or(PtyError::RingClosed)?;
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, size: PtySize) -> Result<(), PtyError> {
        if let Some(dump) = &self.dump
            && let Ok(mut dump) = dump.lock()
            && let Err(error) = dump.write_resize(size.columns.get(), size.rows.get())
        {
            eprintln!("BT_PTY_DUMP resize marker failed: {error}");
        }
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
    use bt_term::{TerminalAdapter, TerminalCursor};

    const ORACLE_EMPTY_PROMPT_LINE: &str = concat!(
        "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP",
        "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP",
    );
    const ORACLE_PROMPT: &str = concat!(
        "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP",
        "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP",
        " ",
    );
    const ORACLE_HISTORY_COMMAND: &str = "echo BTHT";
    const ORACLE_HISTORY_OUTPUT: &[u8] = b"BTHT";
    const ORACLE_EDITING_PREFIX: &str = "typed_XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX";
    const ORACLE_POST_RESIZE_INPUT: &str = "Z";

    #[derive(Debug)]
    struct CursorOracleEvidence {
        synchronization_dsr_requests: usize,
        synchronization_replies: String,
        synchronization_output: String,
        typed_cursor: TerminalCursor,
        typed_line: String,
        recalled_cursor: TerminalCursor,
        recalled_line: String,
        cleared_line: String,
        typed_screen: Vec<String>,
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
            let startup = r#"Set-PSReadLineOption -HistorySaveStyle SaveNothing; function global:prompt { Write-Host ('Q' * 110); (('P' * 81) + ' ') }"#;
            let command = PtyCommand::new("powershell.exe")
                .arg("-NoLogo")
                .arg("-NoProfile")
                .arg("-NoExit")
                .arg("-Command")
                .arg(startup);
            let session = PtySession::spawn(command, size(100, 18), no_wake()).unwrap();
            let terminal = TerminalAdapter::new(nz32(100), nz32(18));
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
        let synchronization_output_start = oracle.raw_output.len();
        let synchronization_reply_start = oracle.pty_replies.len();
        oracle.write_line("Start-Sleep -Seconds 2");
        oracle.pump_for(Duration::from_millis(100));
        let prior_resize_storm = [
            (101, 19),
            (109, 21),
            (118, 23),
            (124, 24),
            (123, 23),
            (99, 19),
            (62, 12),
            (53, 10),
            (52, 10),
            (61, 12),
            (89, 17),
            (96, 18),
            (86, 15),
            (42, 8),
            (29, 7),
            (32, 7),
            (65, 12),
            (83, 15),
            (85, 15),
            (71, 13),
            (26, 6),
            (20, 5),
            (37, 8),
            (98, 17),
            (99, 17),
            (57, 10),
            (23, 6),
            (26, 7),
            (77, 14),
            (104, 17),
            (90, 16),
            (61, 12),
            (40, 9),
            (46, 10),
            (82, 17),
            (114, 22),
            (118, 23),
            (119, 23),
        ];
        oracle.terminal.begin_resize_transaction();
        for (columns, rows) in prior_resize_storm {
            oracle.resize_terminal(columns, rows);
            oracle.pump_for(Duration::from_millis(4));
        }
        oracle.resize_conpty(119, 23);
        oracle.terminal.reconcile_resize_transaction_to_viewport();
        oracle.pump_for(Duration::from_millis(500));
        oracle.pump_until_quiet(Duration::from_secs(3));
        oracle.terminal.finish_resize_transaction();
        oracle.wait_for_current_line(ORACLE_EMPTY_PROMPT_LINE);
        oracle.write_line("Clear-Host");
        oracle.pump_until_quiet(Duration::from_secs(3));
        oracle.wait_for_current_line(ORACLE_EMPTY_PROMPT_LINE);

        let history_start = oracle.raw_output.len();
        oracle.write_line(ORACLE_HISTORY_COMMAND);
        oracle.wait_for_output_since(history_start, ORACLE_HISTORY_OUTPUT);
        oracle.pump_until_quiet(Duration::from_secs(3));
        oracle.wait_for_current_line(ORACLE_EMPTY_PROMPT_LINE);

        let resize_storm = [
            (111, 20),
            (46, 7),
            (12, 1),
            (13, 2),
            (28, 7),
            (71, 15),
            (79, 16),
            (66, 14),
            (22, 7),
            (18, 6),
            (42, 12),
            (98, 21),
            (60, 14),
            (16, 6),
            (27, 9),
            (79, 17),
            (89, 19),
            (85, 18),
            (25, 7),
            (19, 7),
            (51, 11),
            (90, 16),
            (53, 11),
            (11, 5),
            (42, 10),
            (86, 15),
            (85, 15),
            (49, 10),
            (31, 8),
            (64, 13),
            (104, 18),
            (99, 17),
            (46, 10),
            (38, 9),
            (59, 14),
            (117, 21),
            (118, 21),
            (72, 13),
            (33, 9),
            (39, 10),
            (79, 18),
            (92, 20),
            (95, 20),
            (96, 20),
        ];
        for (final_columns, final_rows) in [(96, 20)] {
            oracle.terminal.begin_resize_transaction();
            for (columns, rows) in resize_storm {
                oracle.resize_terminal(columns, rows);
                oracle.pump_for(Duration::from_millis(4));
            }
            oracle.resize_terminal(final_columns, final_rows);
            oracle.resize_conpty(final_columns, final_rows);
            oracle.terminal.reconcile_resize_transaction_to_viewport();
            oracle.terminal.finish_resize_transaction();
        }

        oracle
            .session
            .write(format!("{ORACLE_EDITING_PREFIX}{ORACLE_POST_RESIZE_INPUT}").as_bytes())
            .unwrap();
        oracle.pump_for(Duration::from_millis(300));
        let typed_cursor = oracle.terminal.cursor();
        let typed_line = oracle.current_prompt_text();
        let typed_screen = oracle.terminal.visible_text();

        oracle.session.write(b"\x1b[A").unwrap();
        oracle.pump_for(Duration::from_millis(300));
        let recalled_cursor = oracle.terminal.cursor();
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
            typed_cursor,
            typed_line,
            recalled_cursor,
            recalled_line,
            cleared_line,
            typed_screen,
            recalled_screen,
            cleared_screen,
        }
    }

    fn environment_value<'a>(
        environment: &'a [(OsString, OsString)],
        key: &str,
    ) -> Option<&'a std::ffi::OsStr> {
        environment
            .iter()
            .find(|(candidate, _)| environment_key_eq(candidate, OsStr::new(key)))
            .map(|(_, value)| value.as_os_str())
    }

    #[test]
    fn powershell_declares_truecolor_environment_by_default() {
        let command = PtyCommand::powershell();
        assert!(command.strips_inherited_no_color());
        let environment = command.resolved_environment();
        assert_eq!(
            environment_value(&environment, "COLORTERM"),
            Some(std::ffi::OsStr::new("truecolor"))
        );
        assert_eq!(
            environment_value(&environment, "TERM"),
            Some(std::ffi::OsStr::new("xterm-256color"))
        );
    }

    #[test]
    fn plain_command_declares_no_color_capability() {
        let command = PtyCommand::new("some-tool.exe");
        assert!(!command.strips_inherited_no_color());
        let environment = command.resolved_environment();
        assert_eq!(environment_value(&environment, "COLORTERM"), None);
        assert_eq!(environment_value(&environment, "TERM"), None);
    }

    #[test]
    fn explicit_environment_overrides_default_color_declarations() {
        let colorterm_key = if cfg!(windows) {
            "colorterm"
        } else {
            "COLORTERM"
        };
        let environment = PtyCommand::powershell()
            .env(colorterm_key, "24bit")
            .env("TERM", "better-terminal")
            .resolved_environment();
        assert_eq!(
            environment_value(&environment, "COLORTERM"),
            Some(std::ffi::OsStr::new("24bit"))
        );
        assert_eq!(
            environment_value(&environment, "TERM"),
            Some(std::ffi::OsStr::new("better-terminal"))
        );
    }

    #[test]
    fn command_no_color_opts_out_while_inherited_is_stripped() {
        // An inherited NO_COLOR no longer suppresses the declarations: it is stripped at spawn
        // and the color-capable shell is declared regardless.
        let interactive = PtyCommand::powershell();
        assert!(interactive.strips_inherited_no_color());
        assert_eq!(
            environment_value(&interactive.resolved_environment(), "COLORTERM"),
            Some(std::ffi::OsStr::new("truecolor"))
        );

        // An explicit command-level NO_COLOR is a genuine opt-out: nothing is stripped, no
        // declarations are added, and the value passes through to the child unchanged.
        let no_color_key = if cfg!(windows) {
            "no_color"
        } else {
            "NO_COLOR"
        };
        let command_opt_out = PtyCommand::powershell().env(no_color_key, "1");
        assert!(!command_opt_out.strips_inherited_no_color());
        let environment = command_opt_out.resolved_environment();
        assert_eq!(environment_value(&environment, "COLORTERM"), None);
        assert_eq!(environment_value(&environment, "TERM"), None);
        assert_eq!(
            environment_value(&environment, "NO_COLOR"),
            Some(std::ffi::OsStr::new("1"))
        );
    }

    #[test]
    fn pty_dump_is_an_exact_byte_sidecar_with_replayable_chunk_metadata() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("bt-pty-dump-{}-{unique}.vt", std::process::id()));
        let chunks_path = pty_dump_chunks_path(&path);
        let dump = PtyDump::create(&path).unwrap();
        let ring = OutputRing::new(NonZeroUsize::new(64).unwrap());
        let mut reader = std::io::Cursor::new(b"first\x1b[2Jsecond".to_vec());

        read_pty_output(
            &mut reader,
            &ring,
            &no_wake(),
            Some(Arc::new(Mutex::new(dump))),
        );

        assert_eq!(
            ring.try_pop(NonZeroUsize::new(64).unwrap()),
            b"first\x1b[2Jsecond"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"first\x1b[2Jsecond");
        let manifest = std::fs::read_to_string(&chunks_path).unwrap();
        let record = manifest
            .lines()
            .find(|line| !line.starts_with('#'))
            .unwrap()
            .split_ascii_whitespace()
            .collect::<Vec<_>>();
        assert_eq!(record[0], "0");
        assert_eq!(record[2], b"first\x1b[2Jsecond".len().to_string());

        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(chunks_path).unwrap();
    }

    #[test]
    fn real_conpty_child_receives_color_environment_even_under_inherited_no_color() {
        // The terminal strips an inherited NO_COLOR, so this holds regardless of the host's own
        // NO_COLOR (this dev host and many CI runners export NO_COLOR=1) — the child must still
        // come up color-capable and must not see the inherited opt-out.
        let command = PtyCommand::powershell()
            .arg("-NoProfile")
            .arg("-Command")
            .arg("cmd.exe /D /C set");
        let mut session = PtySession::spawn(command, size(80, 20), no_wake()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = Vec::new();
        let mut child_exited = false;
        while Instant::now() < deadline {
            output.extend(session.read_output());
            child_exited |= session.try_wait().unwrap().is_some();
            if child_exited && session.output_is_drained() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        output.extend(session.read_output());
        session.shutdown().unwrap();
        let output = String::from_utf8_lossy(&output);
        assert!(
            output
                .lines()
                .any(|line| line.trim() == "COLORTERM=truecolor"),
            "child environment did not contain COLORTERM=truecolor: {output:?}"
        );
        assert!(
            output
                .lines()
                .any(|line| line.trim() == "TERM=xterm-256color"),
            "child environment did not contain TERM=xterm-256color: {output:?}"
        );
        assert!(
            !output.lines().any(|line| line
                .trim_start()
                .to_ascii_uppercase()
                .starts_with("NO_COLOR=")),
            "child environment still carried an inherited NO_COLOR: {output:?}"
        );
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
            evidence.synchronization_dsr_requests, 2,
            "each of the two committed ConPTY resizes must request one cursor synchronization: {evidence:?}"
        );
        assert!(
            evidence.synchronization_replies.contains('R'),
            "terminal did not answer ConPTY's DSR with a CPR: {evidence:?}"
        );
        assert_eq!(
            evidence.typed_line,
            format!("{ORACLE_PROMPT}{ORACLE_EDITING_PREFIX}{ORACLE_POST_RESIZE_INPUT}"),
            "post-resize relative input was not echoed on the current prompt; screen={:?}: {evidence:?}",
            evidence.typed_screen
        );
        assert_eq!(
            evidence.recalled_line,
            format!("{ORACLE_PROMPT}{ORACLE_HISTORY_COMMAND}"),
            "CSI A mixed history text into the wrong prompt row; screen={:?}, output={:?}: {evidence:?}",
            evidence.recalled_screen,
            evidence.synchronization_output
        );
        let typed_prompt_row = evidence
            .typed_cursor
            .row
            .saturating_sub((evidence.typed_line.chars().count() / 96) as u32);
        let recalled_prompt_row = evidence
            .recalled_cursor
            .row
            .saturating_sub((evidence.recalled_line.chars().count() / 96) as u32);
        assert_eq!(
            recalled_prompt_row, typed_prompt_row,
            "history recall moved to a stale prompt row: {evidence:?}"
        );
        assert_eq!(
            &evidence.recalled_screen[..typed_prompt_row as usize],
            &evidence.typed_screen[..typed_prompt_row as usize],
            "history recall overwrote content above the active prompt row: {evidence:?}"
        );
        assert_eq!(
            evidence.cleared_line,
            format!("{ORACLE_PROMPT}{ORACLE_EDITING_PREFIX}{ORACLE_POST_RESIZE_INPUT}"),
            "CSI B did not restore the post-resize input on its prompt row; screen={:?}: {evidence:?}",
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
