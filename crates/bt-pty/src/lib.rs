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
}

impl PtySession {
    pub fn spawn_default(size: PtySize, wake: OutputWake) -> Result<Self, PtyError> {
        let command = PtyCommand::powershell()
            .working_directory(std::env::current_dir().map_err(PtyError::Io)?);
        Self::spawn(command, size, wake)
    }

    pub fn spawn(command: PtyCommand, size: PtySize, wake: OutputWake) -> Result<Self, PtyError> {
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
    use std::{sync::mpsc, time::Duration};

    use super::*;

    fn size(columns: u16, rows: u16) -> PtySize {
        PtySize::cells(
            NonZeroU16::new(columns).unwrap(),
            NonZeroU16::new(rows).unwrap(),
        )
    }

    fn no_wake() -> OutputWake {
        Arc::new(|| {})
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
}
