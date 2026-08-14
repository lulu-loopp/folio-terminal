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

mod shell;
pub use shell::{
    ResolvedShell, ShellChoice, ShellEnvironment, SystemShellEnvironment, resolve_default_shell,
    resolve_powershell_seven,
};

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
/// VT input translated by ConPTY to the shell integration's Ctrl+Alt+Shift+F12 resize-anchor chord.
/// On PSReadLine 2.4.x the handler repairs the cached input anchor and render geometry without
/// repainting; older/unproven versions consume the chord as a no-op.
pub const PSREADLINE_INVOKE_PROMPT_INPUT: &[u8] = b"\x1b[24;8~";

/// Windows PowerShell — the shell that is part of the operating system.
///
/// The floor under every resolution and every fallback in this crate, and named once so that the
/// three places that have to agree on it (the fallback command, the "do not retry what already
/// failed" guard, and the record a fallback leaves behind) cannot drift onto three spellings.
/// A bare name rather than a path, deliberately: it is on `PATH` on every Windows there is, and a
/// composed `%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe` would be this crate
/// guessing at a layout the loader already knows.
pub const WINDOWS_POWERSHELL: &str = "powershell.exe";
const READER_CHUNK_BYTES: usize = 16 * 1024;
const PTY_DUMP_ENV: &str = "BT_PTY_DUMP";

/// The name this terminal announces itself under, in `TERM_PROGRAM`.
///
/// **Public because it is half of a handshake, not a private label.** The value
/// crosses into every child process and is read back by the shell integration
/// script, which turns `FORCE_HYPERLINK` on only for sessions it recognises as
/// ours; `scripts/shell-integration/folio.ps1` therefore carries this exact
/// string as a literal, and the two are held equal by
/// `shell_integration::tests::the_integration_script_knows_the_name_this_terminal_announces`.
/// Renaming one alone leaves every PowerShell pane quietly without hyperlinks —
/// a failure whose only symptom is links that used to work and now print as
/// text, which is why the equality is a gate rather than a convention.
///
/// Third-party tools read it too (it is the de-facto terminal identity variable
/// iTerm2 established), so it is the product's name and not an internal id.
pub const TERM_PROGRAM: &str = "Folio";
const TERM_PROGRAM_VERSION: &str = env!("CARGO_PKG_VERSION");

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

    /// Windows PowerShell as this terminal starts it: interactive, color-capable, `-NoLogo`.
    ///
    /// The one command in this crate that still names an argument, because it is the one command
    /// that names a *specific shell*: it is the guaranteed fallback every other spawn retries
    /// against, so it cannot ask a caller which flags PowerShell takes.
    pub fn powershell() -> Self {
        Self::interactive_shell(WINDOWS_POWERSHELL).arg("-NoLogo")
    }

    /// An interactive, color-capable shell command for `program` — the `COLORTERM`/`TERM`
    /// declaration policy, and **no arguments**.
    ///
    /// `-NoLogo` used to be welded in here as "the one argument", which was true only while every
    /// shell this terminal could start was a PowerShell. It is a PowerShell flag: `cmd.exe` reads
    /// it as the name of a batch file and `bash` as a file to open, so a terminal that can start
    /// either cannot pass it to both. Arguments are now the caller's — for the app, a profile's
    /// own `args` list — and what this constructor still owns is the thing that is genuinely
    /// common to every interactive shell: that it is one, and that it may emit colour.
    pub fn interactive_shell(program: impl Into<OsString>) -> Self {
        let mut command = Self::new(program);
        command.declare_color_support = true;
        command
    }

    fn command_has_no_color(&self) -> bool {
        self.environment
            .iter()
            .any(|(key, _)| environment_key_eq(key, OsStr::new("NO_COLOR")))
    }

    /// Every child receives terminal identity declarations. Color-capable interactive shells also
    /// receive `COLORTERM`/`TERM`. All declarations are merged with the caller's environment, and
    /// explicit values win. A caller that explicitly sets `NO_COLOR` opts out of the color
    /// declarations. An *inherited* `NO_COLOR` does not suppress them — it is stripped instead,
    /// see `strips_inherited_no_color`.
    fn resolved_environment(&self) -> Vec<(OsString, OsString)> {
        let command_has_no_color = self.command_has_no_color();
        let mut environment = Vec::with_capacity(self.environment.len() + 4);
        if !self
            .environment
            .iter()
            .any(|(key, _)| environment_key_eq(key, OsStr::new("TERM_PROGRAM")))
        {
            environment.push(("TERM_PROGRAM".into(), TERM_PROGRAM.into()));
        }
        if !self
            .environment
            .iter()
            .any(|(key, _)| environment_key_eq(key, OsStr::new("TERM_PROGRAM_VERSION")))
        {
            environment.push(("TERM_PROGRAM_VERSION".into(), TERM_PROGRAM_VERSION.into()));
        }
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

/// `spawn_default`'s fallback is a single retry against `powershell.exe`: once resolution has
/// already landed on `WindowsPowerShell` (nothing overrode it, no `pwsh` was found), a spawn
/// failure has no further shell left to fall back to, so it propagates instead of retrying the
/// identical command.
fn shell_spawn_failure_should_fall_back(choice: ShellChoice) -> bool {
    choice != ShellChoice::WindowsPowerShell
}

/// The flags this terminal starts any PowerShell with.
const POWERSHELL_INTERACTIVE_ARGS: &[&str] = &["-NoLogo"];

/// Whether `program` already names the shell the fallback would retry with.
///
/// The named-program half of [`shell_spawn_failure_should_fall_back`]'s rule, and the same rule:
/// retrying a spawn that has just failed with the identical program is not a fallback, it is the
/// same failure twice. Compared case-insensitively and by file name because Windows paths are
/// case-insensitive and `powershell.exe` reaches this both as a bare name (what
/// `resolve_default_shell` returns) and as the System32 path a profile would resolve to.
fn program_is_windows_powershell(program: &OsStr) -> bool {
    Path::new(program)
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(OsStr::new(WINDOWS_POWERSHELL)))
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
    /// Set once, only when a spawn had to fall back to [`WINDOWS_POWERSHELL`] after the
    /// resolved shell failed to start. `Runtime` turns it into the pane's first line, then
    /// discards it.
    shell_fallback: Option<ShellFallback>,
}

/// What happened, for the window to say it in its own words.
///
/// **A fact, not a sentence**, and the change is the whole of this ticket's third item. The
/// sentence this used to be was assembled here, out of the only two things this crate knows: the
/// path of the program and the operating system's account of why it would not start. Both are the
/// wrong register for the one place it is printed — the top of the user's own screen, where the
/// shell's first line belongs. The path is not what the user picked (they picked *Git Bash*, not
/// `D:\App\Tool\Git\bin\bash.exe`), and the OS's account arrives wrapped in the vendored
/// launcher's debugging: the whole `CreateProcessW` call, the command line `Debug`-quoted with the
/// `NUL` terminator `CreateProcessW` requires still on the end of it, and the working directory
/// again.
///
/// So the crate that knows *why* keeps that for the log ([`eprintln!`], where a debugging string
/// is exactly right), and hands up only what it also knows to be true: this program did not start,
/// that one did. The crate that knows the **profiles** is the one that can name them, and it is
/// the one that writes the line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellFallback {
    /// The program that would not start.
    pub requested: OsString,
    /// The program that did — always [`WINDOWS_POWERSHELL`], which is part of Windows.
    pub started: &'static str,
}

impl PtySession {
    /// Shell selection order (ruling 2026-08-04): `BT_SHELL` wins outright; otherwise
    /// `pwsh.exe` (PowerShell 7) is used when [`resolve_default_shell`]'s probe can find an
    /// install, and `powershell.exe` (Windows PowerShell 5.1) is the default. See
    /// `docs/shell-integration.md` and `crate::shell` for the full rationale and the exact
    /// `BT_SHELL` semantics.
    ///
    /// If the resolved shell fails to spawn, this falls back to `powershell.exe` once and
    /// records what happened (`take_shell_fallback`) instead of failing the session —
    /// a Windows PowerShell 5.1 install is effectively guaranteed, while a `BT_SHELL` override or
    /// a `pwsh` resolved from a stale PATH entry is not.
    ///
    /// Whichever program resolution picks — `BT_SHELL`'s value, a found `pwsh.exe`, or the
    /// `powershell.exe` default — is spawned exactly as `spawn_default` always has: `-NoLogo`
    /// plus the terminal's color-capable environment declarations (`PtyCommand::interactive_shell`).
    /// `BT_SHELL` exists to pick *which* PowerShell-family build runs, not to swap in an unrelated
    /// shell, so this is a single uniform rule rather than a per-source special case.
    pub fn spawn_default(size: PtySize, wake: OutputWake) -> Result<Self, PtyError> {
        Self::spawn_default_with(size, wake, None, &SystemShellEnvironment)
    }

    /// [`spawn_default`](Self::spawn_default), started in `working_directory`
    /// instead of this process's own.
    ///
    /// `None` means "wherever the terminal itself is standing", which is what
    /// every session did before there was anything else to inherit. The caller
    /// that passes `Some` is the new-tab verb: a new shell opens where the one
    /// you are looking at is standing, and the directory it hands over is the one
    /// that shell reported over OSC 7 — a fact the shell stated, never a guess.
    pub fn spawn_default_in(
        size: PtySize,
        wake: OutputWake,
        working_directory: Option<PathBuf>,
    ) -> Result<Self, PtyError> {
        Self::spawn_default_with(size, wake, working_directory, &SystemShellEnvironment)
    }

    /// Start `program` with `args` as this terminal's interactive shell, in `working_directory`,
    /// with `environment` layered over the terminal's own declarations.
    ///
    /// The entry point a **profile** spawns through: the caller has already decided which
    /// executable this is, which flags it takes and which variables it needs, because those are
    /// properties of the profile the user picked and not of "the shell". `spawn_default_in` is the
    /// special case of this where the caller wants the default-shell resolution order instead of a
    /// named program.
    ///
    /// The arguments are `OsString` and not `&str` because a profile's arguments now include
    /// **paths** — the init file a bash-family profile is handed — and a path is not text this
    /// crate is entitled to require be UTF-8.
    ///
    /// `environment` is applied to the resolved program only. The `powershell.exe` retry below is a
    /// *different profile* by the time it runs, and variables that were chosen for the shell that
    /// would not start are not facts about the one that did.
    ///
    /// It keeps the same recoverable-failure contract as `spawn_default`: a program that will not
    /// start falls back once to `powershell.exe` and leaves a record of the swap
    /// (`take_shell_fallback`) rather than failing the session, because a window with no
    /// shell in it is worse than a window with the wrong one *provided the swap is stated*. The
    /// retry is skipped when the program is already `powershell.exe`, where it would repeat an
    /// identical, already-failed spawn.
    pub fn spawn_shell_in(
        program: impl Into<OsString>,
        args: &[OsString],
        environment: &[(OsString, OsString)],
        size: PtySize,
        wake: OutputWake,
        working_directory: Option<PathBuf>,
    ) -> Result<Self, PtyError> {
        let program = program.into();
        let fall_back = !program_is_windows_powershell(&program);
        Self::spawn_interactive(
            program,
            args,
            environment,
            size,
            wake,
            working_directory,
            fall_back,
        )
    }

    /// The testable core of `spawn_default`: shell resolution goes through the injected
    /// `environment` rather than `std::env`/the real filesystem, so resolution-order and
    /// fallback-on-failure tests are deterministic regardless of what is installed on the host.
    fn spawn_default_with(
        size: PtySize,
        wake: OutputWake,
        working_directory: Option<PathBuf>,
        environment: &dyn ShellEnvironment,
    ) -> Result<Self, PtyError> {
        let resolved = resolve_default_shell(environment);
        Self::spawn_interactive(
            resolved.program,
            // PowerShell's own flag, stated by the one entry point that knows it is starting a
            // PowerShell. Every other shell's arguments arrive through `spawn_shell_in`.
            &POWERSHELL_INTERACTIVE_ARGS
                .iter()
                .map(OsString::from)
                .collect::<Vec<_>>(),
            &[],
            size,
            wake,
            working_directory,
            shell_spawn_failure_should_fall_back(resolved.choice),
        )
    }

    /// Both spawn doors' shared body: validate the directory, build the interactive command, and
    /// apply the one-shot fallback when the caller says the failure is recoverable.
    ///
    /// One function so the two doors cannot drift on the three things that are not about *which*
    /// shell — that a vanished working directory is survivable, that every shell gets the colour
    /// declarations, and what a fallback leaves behind for the window to say.
    fn spawn_interactive(
        program: OsString,
        args: &[OsString],
        environment: &[(OsString, OsString)],
        size: PtySize,
        wake: OutputWake,
        working_directory: Option<PathBuf>,
        fall_back: bool,
    ) -> Result<Self, PtyError> {
        // A directory that no longer exists would fail the spawn outright, and a
        // tab that refuses to open because the last one was deleted out from
        // under it is a worse answer than a tab that opens at home. This is the
        // system boundary — the filesystem — so it is checked here and nowhere
        // else.
        let working_directory = match working_directory {
            Some(directory) if directory.is_dir() => directory,
            _ => std::env::current_dir().map_err(PtyError::Io)?,
        };
        let command = environment
            .iter()
            .fold(
                args.iter().fold(
                    PtyCommand::interactive_shell(program.clone()),
                    |command, argument| command.arg(argument),
                ),
                |command, (key, value)| command.env(key, value),
            )
            .working_directory(working_directory.clone());
        match Self::spawn(command, size, wake.clone()) {
            Ok(session) => Ok(session),
            Err(spawn_error) if fall_back => {
                // The whole of the operating system's account, kept where a debugging string is
                // the right register and read by whoever is debugging. It is deliberately not
                // carried up: see [`ShellFallback`].
                eprintln!(
                    "recoverable shell spawn failure: {} did not start ({spawn_error}); \
                     using {WINDOWS_POWERSHELL} instead",
                    Path::new(&program).display()
                );
                let fallback = PtyCommand::powershell().working_directory(working_directory);
                let mut session = Self::spawn(fallback, size, wake)?;
                session.shell_fallback = Some(ShellFallback {
                    requested: program,
                    started: WINDOWS_POWERSHELL,
                });
                Ok(session)
            }
            Err(spawn_error) => Err(spawn_error),
        }
    }

    /// Take the one-shot record of a spawn fallback, if there was one. `None` once taken, and
    /// `None` for every session that started its resolved shell cleanly.
    pub fn take_shell_fallback(&mut self) -> Option<ShellFallback> {
        self.shell_fallback.take()
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
            shell_fallback: None,
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
    use bt_term::{DualPlaneSession, RESIZE_REQUEST_QUIET, TerminalAdapter, TerminalCursor};

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
        /// Every cursor-position report this terminal answered, with the grid it was computed
        /// from. Under the sidecar that answer becomes the child's own cursor, so it is evidence
        /// about *us*, not about ConPTY.
        cpr_log: Vec<CprExchange>,
        /// The width the probe last projected onto the local grid, recorded alongside each reply.
        cpr_columns: u16,
    }

    impl InteractiveOracle {
        fn spawn() -> Self {
            let startup = r#"Set-PSReadLineOption -HistorySaveStyle SaveNothing; function global:prompt { Write-Host ('Q' * 110); (('P' * 81) + ' ') }"#;
            Self::spawn_with(startup, 100, 18)
        }

        /// Same live child, but with a caller-chosen startup script and geometry. Probes that care
        /// about absolute row addressing need a short prompt and a wide start width.
        fn spawn_with(startup: &str, columns: u16, rows: u16) -> Self {
            Self::spawn_shell_with("powershell.exe", startup, columns, rows)
        }

        /// The line editor is the component that caches absolute rows, so which shell hosts it is
        /// part of the probe's subject: `bt-app` seats spawn `pwsh.exe`, whose PSReadLine is not
        /// the 2.0.0 that ships inside Windows PowerShell.
        fn spawn_shell_with(shell: &str, startup: &str, columns: u16, rows: u16) -> Self {
            Self::spawn_shell_profile(shell, startup, columns, rows, false)
        }

        /// `load_profile` runs the host's real `$PROFILE` chain instead of `-NoProfile`. Nothing a
        /// reconstruction can do is as faithful as the user's own conda hook and their own
        /// dot-source of `scripts/shell-integration/folio.ps1`, so the corruption hunt
        /// gets the real thing and the reconstruction is kept only as the controlled comparison.
        fn spawn_shell_profile(
            shell: &str,
            startup: &str,
            columns: u16,
            rows: u16,
            load_profile: bool,
        ) -> Self {
            let mut command = PtyCommand::new(shell).arg("-NoLogo");
            if !load_profile {
                command = command.arg("-NoProfile");
            }
            let command = command.arg("-NoExit").arg("-Command").arg(startup);
            let session = PtySession::spawn(command, size(columns, rows), no_wake()).unwrap();
            let terminal = TerminalAdapter::new(nz32(columns), nz32(rows));
            Self {
                session,
                terminal,
                raw_output: Vec::new(),
                pty_replies: Vec::new(),
                cpr_log: Vec::new(),
                cpr_columns: columns,
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
                if reply.starts_with(b"\x1b[") && reply.ends_with(b"R") {
                    self.cpr_log.push(CprExchange {
                        reply: escaped(&reply),
                        local_columns: self.cpr_columns,
                        cursor: self.terminal.cursor(),
                    });
                }
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

        /// The naive staging: resize the local grid and the pseudoconsole and nothing else.
        ///
        /// This is deliberately *not* what the application does, and the difference is the whole
        /// point of stages (F)–(H). Without a resize transaction there is no vendor reconcile, so
        /// the bottom-anchored grid keeps every row the reflow added out of the viewport while
        /// conhost keeps them in it — and the two disagree about which absolute row the prompt is
        /// on. Use `begin_resize_transaction` + `reconcile_resize_transaction_to_viewport` to
        /// reproduce the product; use this only to show what that reconcile is buying.
        fn resize_both(&mut self, columns: u16, rows: u16) {
            self.resize_terminal(columns, rows);
            self.resize_conpty(columns, rows);
        }

        /// Ask the child to read conhost's own text buffer, viewport origin and cursor back to us
        /// (`BTDUMP` is installed by the probe's startup script). On the pinned sidecar this is the
        /// only way to see what a resize did to rows already on screen, because it repaints
        /// nothing; on the inbox implementation it is the independent check that its repaint told
        /// us the truth.
        fn probe_buffer_dump(&mut self, stage: &str, rows: u16) {
            let start = self.raw_output.len();
            self.write_line(&format!("BTDUMP {rows}"));
            self.wait_for_output_since(start, b"BTDUMPEND");
            self.pump_until_quiet(Duration::from_secs(10));
            self.wait_for_current_line("BTP>");
            let emitted = String::from_utf8_lossy(&self.raw_output[start..]).into_owned();
            let dump = emitted
                .rsplit("BTDUMPBEGIN")
                .next()
                .and_then(|tail| tail.split("BTDUMPEND").next())
                .map(|body| body.replace(['\r', '\n'], ""))
                .unwrap_or_default();
            eprintln!("BT_CONPTY_NARROW_PROBE stage={stage}-dump{dump}");
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
            environment_value(&environment, "TERM_PROGRAM"),
            Some(std::ffi::OsStr::new("Folio"))
        );
        assert_eq!(
            environment_value(&environment, "TERM_PROGRAM_VERSION"),
            Some(std::ffi::OsStr::new(env!("CARGO_PKG_VERSION")))
        );
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
    fn plain_command_declares_terminal_identity_but_no_color_capability() {
        let command = PtyCommand::new("some-tool.exe");
        assert!(!command.strips_inherited_no_color());
        let environment = command.resolved_environment();
        assert_eq!(
            environment_value(&environment, "TERM_PROGRAM"),
            Some(std::ffi::OsStr::new("Folio"))
        );
        assert_eq!(
            environment_value(&environment, "TERM_PROGRAM_VERSION"),
            Some(std::ffi::OsStr::new(env!("CARGO_PKG_VERSION")))
        );
        assert_eq!(environment_value(&environment, "COLORTERM"), None);
        assert_eq!(environment_value(&environment, "TERM"), None);
    }

    #[test]
    fn explicit_environment_overrides_default_terminal_and_color_declarations() {
        let colorterm_key = if cfg!(windows) {
            "colorterm"
        } else {
            "COLORTERM"
        };
        let term_program_key = if cfg!(windows) {
            "term_program"
        } else {
            "TERM_PROGRAM"
        };
        let environment = PtyCommand::powershell()
            .env(colorterm_key, "24bit")
            .env("TERM", "better-terminal")
            .env(term_program_key, "UserTerminal")
            .env("TERM_PROGRAM_VERSION", "user-version")
            .resolved_environment();
        assert_eq!(
            environment_value(&environment, "TERM_PROGRAM"),
            Some(std::ffi::OsStr::new("UserTerminal"))
        );
        assert_eq!(
            environment_value(&environment, "TERM_PROGRAM_VERSION"),
            Some(std::ffi::OsStr::new("user-version"))
        );
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
        let deadline = Instant::now() + Duration::from_secs(15);
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
            output
                .lines()
                .any(|line| line.trim() == "TERM_PROGRAM=Folio"),
            "child environment did not contain TERM_PROGRAM=Folio: {output:?}"
        );
        let expected_version = format!("TERM_PROGRAM_VERSION={}", env!("CARGO_PKG_VERSION"));
        assert!(
            output.lines().any(|line| line.trim() == expected_version),
            "child environment did not contain {expected_version}: {output:?}"
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

    /// A path that is guaranteed not to exist on the test host, so a spawn attempt against it
    /// deterministically fails regardless of what shells happen to be installed.
    fn nonexistent_program(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "bt-pty-missing-{label}-{}-{}.exe",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn spawn_default_honors_bt_shell_override_when_it_resolves() {
        // `BT_SHELL` set to a real, spawnable shell: the resolved-shell path runs and no fallback
        // notice is left behind. `powershell.exe` is a bare name here deliberately — it exercises
        // the "resolved by the OS at spawn time" half of the documented `BT_SHELL` semantics.
        let environment = shell::FakeShellEnvironment::new().with_var("BT_SHELL", "powershell.exe");
        let mut session =
            PtySession::spawn_default_with(size(40, 8), no_wake(), None, &environment).unwrap();
        assert!(session.take_shell_fallback().is_none());
        assert!(session.child_id().is_some());
        session.shutdown().unwrap();
    }

    #[test]
    fn spawn_default_falls_back_to_windows_powershell_when_bt_shell_cannot_start() {
        let missing = nonexistent_program("bt-shell");
        let environment = shell::FakeShellEnvironment::new().with_var("BT_SHELL", &missing);
        let mut session =
            PtySession::spawn_default_with(size(40, 8), no_wake(), None, &environment).unwrap();
        let fallback = session
            .take_shell_fallback()
            .expect("a spawn failure on the resolved shell must leave a record of the fallback");
        assert_eq!(fallback.requested, missing.as_os_str());
        assert_eq!(fallback.started, WINDOWS_POWERSHELL);
        assert!(
            session.take_shell_fallback().is_none(),
            "the record is one-shot"
        );
        assert!(session.child_id().is_some());
        session.shutdown().unwrap();
    }

    #[test]
    fn spawn_default_falls_back_to_windows_powershell_when_resolved_pwsh_cannot_start() {
        // No `BT_SHELL`; the fake probe reports a `pwsh.exe` inside a real, existing directory
        // (`std::env::temp_dir()`, so `PATH` search itself is exercised faithfully), but the fake
        // never actually creates that file, so the real spawn attempt genuinely fails with "not
        // found". The resolution order still prefers `pwsh` (`PowerShellCore`), and it is that
        // *spawn* failure that drives the fallback, exactly as an unavailable real install would
        // drive it in production.
        let pwsh_path = std::env::temp_dir().join("pwsh.exe");
        let environment = shell::FakeShellEnvironment::new()
            .with_var(
                "PATH",
                std::env::join_paths([std::env::temp_dir()]).unwrap(),
            )
            .with_file(&pwsh_path);
        let resolved = resolve_default_shell(&environment);
        assert_eq!(resolved.choice, ShellChoice::PowerShellCore);
        assert_eq!(resolved.program, pwsh_path.as_os_str());

        let mut session =
            PtySession::spawn_default_with(size(40, 8), no_wake(), None, &environment).unwrap();
        let fallback = session
            .take_shell_fallback()
            .expect("an unresolvable pwsh.exe path must still fall back and leave a record");
        assert_eq!(fallback.requested, pwsh_path.as_os_str());
        assert_eq!(fallback.started, WINDOWS_POWERSHELL);
        assert!(session.child_id().is_some());
        session.shutdown().unwrap();
    }

    #[test]
    fn fallback_retry_is_only_attempted_for_a_resolved_shell_other_than_windows_powershell() {
        // Pins the exact condition `spawn_default_with` retries on. A `WindowsPowerShell`
        // resolution is already `powershell.exe`; retrying it on failure would just repeat the
        // identical, already-failed spawn, so `spawn_default` must propagate that error instead
        // of manufacturing an infinite-seeming loop of one.
        assert!(shell_spawn_failure_should_fall_back(ShellChoice::Override));
        assert!(shell_spawn_failure_should_fall_back(
            ShellChoice::PowerShellCore
        ));
        assert!(!shell_spawn_failure_should_fall_back(
            ShellChoice::WindowsPowerShell
        ));
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
        assert_resize_cursor_outcome_contract(&evidence);
    }

    /// The outcome every ConPTY implementation owes after a resize storm, independent of *how* it
    /// resynchronizes the cursor. Kept separate from the mechanism assertions because the two
    /// implementations reach this contract by different means: the pinned sidecar asks the terminal
    /// `CSI 6 n` and adopts the reply (`microsoft/terminal#19535`), while the Windows inbox
    /// implementation repaints the whole viewport and places the cursor itself.
    fn assert_resize_cursor_outcome_contract(evidence: &CursorOracleEvidence) {
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

    /// What the Windows inbox implementation does with the same oracle, measured rather than
    /// assumed. This test used to assert the `#18725` failure shape as a known-bad record; on
    /// Windows 11 build 26200 that shape no longer occurs, so asserting it would have pinned a
    /// falsehood. What survives of `#18725` here is a *mechanism* divergence, not an outcome one:
    /// the inbox implementation never asks for a cursor position report, and instead emits an
    /// unsolicited full-viewport repaint terminated by an absolute CUP on every resize (see the
    /// `stage=on-narrow` evidence of `narrow_resize_conpty_reflow_probe` under
    /// `BT_CONPTY_FORCE_SYSTEM=1`).
    ///
    /// The pin therefore no longer rests on this oracle's outcome. It rests on the repaint traffic
    /// that mechanism implies: one full viewport per committed resize, arriving mid-transaction,
    /// which is exactly the traffic `docs/M1.8-resize-visual-stability.md` records the sidecar as
    /// not producing. Anyone proposing to flip the default to the inbox implementation must re-run
    /// the M1.8 resize-stability corpus, not just this oracle.
    #[test]
    #[ignore = "upstream A/B record: drives a real interactive PowerShell through the inbox ConPTY"]
    fn system_conpty_resize_cursor_outcome_matches_the_sidecar_contract() {
        assert!(
            std::env::var_os("BT_CONPTY_FORCE_SYSTEM").is_some(),
            "run in a fresh process with BT_CONPTY_FORCE_SYSTEM=1"
        );
        assert_eq!(conpty_source(), ConPtySource::System);
        let evidence = run_resize_cursor_oracle();
        eprintln!("BT_CONPTY_ORACLE {} evidence={evidence:?}", conpty_source());
        assert_eq!(
            evidence.synchronization_dsr_requests, 0,
            "the inbox implementation is expected to resynchronize by repaint, not by DSR; a nonzero \
             count means it adopted the `#19535` handshake and this record needs revisiting: {evidence:?}"
        );
        assert_resize_cursor_outcome_contract(&evidence);
    }

    /// The 49-column hard-terminated row the narrow-reflow probe writes. Deliberately longer than
    /// the narrow width it is then squeezed into, and self-indexing so a wrap point is readable.
    const NARROW_PROBE_LINE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvw";
    const NARROW_PROBE_WIDE: u16 = 104;
    const NARROW_PROBE_NARROW: u16 = 45;

    fn escaped(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes)
            .chars()
            .map(|c| match c {
                '\x1b' => "<ESC>".to_owned(),
                '\r' => "<CR>".to_owned(),
                '\n' => "<LF>".to_owned(),
                '\x07' => "<BEL>".to_owned(),
                c if (c as u32) < 0x20 => format!("<{:02X}>", c as u32),
                c => c.to_string(),
            })
            .collect()
    }

    fn occupied_rows(rows: &[String]) -> Vec<(usize, String)> {
        rows.iter()
            .enumerate()
            .filter(|(_, row)| !row.trim_end().is_empty())
            .map(|(index, row)| (index, row.trim_end().to_owned()))
            .collect()
    }

    /// Ground-truth probe for narrow-resize reflow (dev-gated; run with `--ignored --nocapture`).
    ///
    /// Question: when a real ConPTY narrows below the length of a *hard-terminated* row that is
    /// already on screen, does conhost re-wrap that row (pushing everything below it down), does it
    /// repaint at all, and where does it then address the prompt with absolute CUP?
    ///
    /// What it answered, on the pinned sidecar ConPTY (`CONPTY_SIDECAR_VERSION`):
    ///
    /// * conhost **does** re-wrap a hard-terminated row that is longer than the new width. A
    ///   49-column row narrowed to 45 becomes two buffer rows (45 + 4) and every row below it moves
    ///   down. The `WRAPLINE`-agnostic split bt-term inherits from the vendored grid is therefore
    ///   the faithful rule, not a divergence. Widening straight back restores the original rows
    ///   exactly, so the reflow is lossless in both directions.
    /// * conhost's viewport origin stays at buffer row 0 across the narrowing: the rows the reflow
    ///   added are absorbed by the blank region at the bottom, nothing scrolls into scrollback.
    /// * the sidecar emits **nothing at all** on resize. It resynchronizes only later, by asking the
    ///   terminal `CSI 6 n` and adopting the answer, which makes this terminal's post-reflow cursor
    ///   row the coordinate the child then renders against.
    /// * the inbox/system ConPTY (`BT_CONPTY_FORCE_SYSTEM=1`) instead repaints the whole viewport
    ///   on resize and places the cursor itself: `CSI ?25l`, `CSI 8;rows;cols t`, `CSI H`, every row
    ///   followed by `EL` and CRLF, then an absolute CUP and `CSI ?25h`. Repaint-on-resize is thus
    ///   ConPTY-implementation dependent; do not build terminal-side rules on the assumption that
    ///   either happens.
    ///
    /// Stages (F)–(H) then ask the question that decides ownership: *is the divergence ours?*
    ///
    /// * Resizing the local grid and the pseudoconsole directly (stages A–E, `resize_both`) does
    ///   diverge. The vendored grid is bottom-anchored — `shrink_columns` pushes every row the
    ///   reflow adds out through the top of the viewport into scrollback — while conhost grows
    ///   downward into the blank tail. After a 104→46 narrowing our viewport was one row short of
    ///   conhost's, and because the sidecar adopts our CPR, conhost's own buffer then took the
    ///   child's next write on the wrong row.
    /// * The application never resizes that way. `resize_at` opens a transaction, the coalesced
    ///   pseudoconsole size is committed by `mark_pty_resize_requested_at`, and the vendor
    ///   reconcile in between (`reconcile_resize_transaction_to_viewport`) re-evaluates height
    ///   after the final-width reflow. Through that path (stages F–H) our viewport is row-for-row
    ///   identical to conhost's buffer, our CPR is the row conhost itself would have chosen, and
    ///   the child's redraw lands on the prompt row — under both implementations, for a single
    ///   resize, for the recording's nine-step drag, pumped or unpumped, for a plain keystroke and
    ///   for a wrapping history recall. The reconcile is the seam that keeps ConPTY's absolute
    ///   addressing meaningful; nothing else in the resize path may be allowed to skip it.
    ///
    /// This is an observation harness, not a behaviour assertion on conhost: the assertions below
    /// only check that the probe actually produced the evidence it claims to produce. Read the
    /// `BT_CONPTY_NARROW_PROBE` lines on stderr for the raw byte and buffer evidence.
    #[test]
    #[ignore = "dev probe: drives a real interactive PowerShell through ConPTY; host-timing sensitive"]
    fn narrow_resize_conpty_reflow_probe() {
        // Each question gets a fresh child: ConPTY re-emits already-visible text when a later write
        // reflows it, so reusing one child would let a stale repaint answer a newer question.

        // (A) Narrow only. What did conhost do to the two over-long hard-terminated rows?
        let mut narrowed = narrow_probe_child();
        let mark = narrowed.raw_output.len();
        narrowed.resize_both(NARROW_PROBE_NARROW, 26);
        narrowed.pump_for(Duration::from_millis(750));
        eprintln!(
            "BT_CONPTY_NARROW_PROBE stage=on-narrow width={NARROW_PROBE_NARROW} emitted={:?}",
            escaped(&narrowed.raw_output[mark..])
        );
        narrowed.probe_buffer_dump("narrow", 6);

        // (B) Narrow, then widen straight back with nothing written in between. Is the clipped tail
        //     restored, and does the buffer return to its original row assignment?
        let mut round_trip = narrow_probe_child();
        round_trip.resize_both(NARROW_PROBE_NARROW, 26);
        round_trip.pump_for(Duration::from_millis(400));
        let mark = round_trip.raw_output.len();
        round_trip.resize_both(NARROW_PROBE_WIDE, 26);
        round_trip.pump_for(Duration::from_millis(750));
        eprintln!(
            "BT_CONPTY_NARROW_PROBE stage=on-widen width={NARROW_PROBE_WIDE} emitted={:?}",
            escaped(&round_trip.raw_output[mark..])
        );
        round_trip.probe_buffer_dump("widened", 6);

        // (C) Narrow, then make the line editor redraw. The absolute CUP row it picks is what our
        //     grid must agree with, because that redraw lands on whatever row we put there.
        let mut edited = narrow_probe_child();
        let wide_rows = edited.terminal.visible_text();
        let wide_cursor = edited.terminal.cursor();
        eprintln!(
            "BT_CONPTY_NARROW_PROBE stage=wide width={NARROW_PROBE_WIDE} cursor={wide_cursor:?} rows={:?}",
            occupied_rows(&wide_rows)
        );
        edited.resize_both(NARROW_PROBE_NARROW, 26);
        edited.pump_for(Duration::from_millis(400));
        let mark = edited.raw_output.len();
        edited.session.write(b"Z").unwrap();
        edited.pump_for(Duration::from_millis(750));
        eprintln!(
            "BT_CONPTY_NARROW_PROBE stage=narrow-keystroke emitted={:?} local_cursor={:?} local_rows={:?}",
            escaped(&edited.raw_output[mark..]),
            edited.terminal.cursor(),
            occupied_rows(&edited.terminal.visible_text())
        );
        edited.session.write(b"").unwrap();
        edited.pump_until_quiet(Duration::from_secs(10));
        edited.probe_buffer_dump("after-keystroke", 6);

        // (D) The recording's exact shape: one hard-terminated 49-column line with the prompt
        //     directly beneath it and nothing above, narrowed to 46 the way the seat gesture did.
        let mut recording = narrow_probe_clean_child();
        eprintln!(
            "BT_CONPTY_NARROW_PROBE stage=recording-wide cursor={:?} rows={:?}",
            recording.terminal.cursor(),
            occupied_rows(&recording.terminal.visible_text())
        );
        recording.resize_both(46, 26);
        recording.pump_for(Duration::from_millis(750));
        eprintln!(
            "BT_CONPTY_NARROW_PROBE stage=recording-narrow local_cursor={:?} local_rows={:?}",
            recording.terminal.cursor(),
            occupied_rows(&recording.terminal.visible_text())
        );
        recording.probe_buffer_dump("recording", 4);

        // (E) The recording's shape *and* the gesture that follows it: one keystroke into
        //     PSReadLine after the narrowing. The line editor redraws its input at an absolute
        //     row it derived from the console it can see; whether that row is the prompt row
        //     this terminal actually has is the whole question.
        let mut recording_edit = narrow_probe_clean_child();
        recording_edit.resize_both(46, 26);
        recording_edit.pump_for(Duration::from_millis(400));
        let mark = recording_edit.raw_output.len();
        recording_edit.session.write(b"Z").unwrap();
        recording_edit.pump_for(Duration::from_millis(750));
        eprintln!(
            "BT_CONPTY_NARROW_PROBE stage=recording-keystroke source={} emitted={:?} local_cursor={:?} local_rows={:?}",
            conpty_source(),
            escaped(&recording_edit.raw_output[mark..]),
            recording_edit.terminal.cursor(),
            occupied_rows(&recording_edit.terminal.visible_text())
        );
        recording_edit.session.write(b"\x03").unwrap();
        recording_edit.pump_until_quiet(Duration::from_secs(10));
        recording_edit.probe_buffer_dump("recording-keystroke", 5);

        // (F) The same shape driven the way the application actually drives it: a resize
        //     transaction around many projected local widths, one committed pseudoconsole size,
        //     and the vendor reconcile in between. This is the only staging whose disagreement
        //     with the `*-dump` line is a real product bug.
        for (label, storm) in [
            ("single", &[46u16][..]),
            (
                "storm",
                &[51, 47, 45, 27, 46, 50, 49, 27, 46, 64, 54, 52, 46][..],
            ),
        ] {
            let mut app = narrow_probe_clean_child();
            app.terminal.begin_resize_transaction();
            for width in storm {
                app.resize_terminal(*width, 26);
                app.pump_for(Duration::from_millis(4));
            }
            app.resize_conpty(46, 26);
            app.terminal.reconcile_resize_transaction_to_viewport();
            app.pump_for(Duration::from_millis(400));
            eprintln!(
                "BT_CONPTY_NARROW_PROBE stage=app-{label}-narrow local_cursor={:?} local_rows={:?}",
                app.terminal.cursor(),
                occupied_rows(&app.terminal.visible_text())
            );
            let mark = app.raw_output.len();
            app.session.write(b"Z").unwrap();
            app.pump_for(Duration::from_millis(750));
            eprintln!(
                "BT_CONPTY_NARROW_PROBE stage=app-{label}-keystroke emitted={:?} local_cursor={:?} local_rows={:?}",
                escaped(&app.raw_output[mark..]),
                app.terminal.cursor(),
                occupied_rows(&app.terminal.visible_text())
            );
            app.session.write(b"\x03").unwrap();
            app.pump_until_quiet(Duration::from_secs(10));
            app.terminal.finish_resize_transaction();
            app.probe_buffer_dump(&format!("app-{label}"), 5);
        }

        // (G) The seat recording replayed as a live child: its 39-column prompt, its drag (each
        //     step resizes the pseudoconsole, exactly as the `# RESIZE` markers record), and then
        //     one keystroke. The line editor's redraw carries an absolute row; whether that row is
        //     the prompt row is the whole user-visible difference between the two ConPTYs.
        let mut seat = narrow_probe_seat_child();
        eprintln!(
            "BT_CONPTY_NARROW_PROBE stage=seat-wide source={} cursor={:?} rows={:?}",
            conpty_source(),
            seat.terminal.cursor(),
            occupied_rows(&seat.terminal.visible_text())
        );
        seat.terminal.begin_resize_transaction();
        for width in [51u16, 47, 45, 27, 46, 50, 49, 27, 46] {
            seat.resize_terminal(width, 26);
            seat.resize_conpty(width, 26);
            seat.terminal.reconcile_resize_transaction_to_viewport();
            seat.pump_for(Duration::from_millis(60));
        }
        eprintln!(
            "BT_CONPTY_NARROW_PROBE stage=seat-narrow local_cursor={:?} local_rows={:?}",
            seat.terminal.cursor(),
            occupied_rows(&seat.terminal.visible_text())
        );
        let mark = seat.raw_output.len();
        seat.session.write(b"Z").unwrap();
        seat.pump_for(Duration::from_millis(750));
        eprintln!(
            "BT_CONPTY_NARROW_PROBE stage=seat-keystroke emitted={:?} local_cursor={:?} local_rows={:?}",
            escaped(&seat.raw_output[mark..]),
            seat.terminal.cursor(),
            occupied_rows(&seat.terminal.visible_text())
        );

        // (H) The same seat drag with the terminal never servicing the pipe in between, so the
        //     keystroke reaches the child in the same wake-up as the resize. On the sidecar the
        //     child's own cursor read then races a cursor-position round trip that has not even
        //     been asked yet; on the inbox implementation there is no round trip to race.
        let mut race = narrow_probe_seat_child();
        race.session.write(SEAT_TYPED.as_bytes()).unwrap();
        race.pump_for(Duration::from_millis(400));
        race.terminal.begin_resize_transaction();
        for width in [51u16, 47, 45, 27, 46, 50, 49, 27, 46] {
            race.resize_terminal(width, 26);
            race.resize_conpty(width, 26);
        }
        race.terminal.reconcile_resize_transaction_to_viewport();
        let mark = race.raw_output.len();
        race.session.write(b"Z").unwrap();
        race.pump_for(Duration::from_millis(1500));
        eprintln!(
            "BT_CONPTY_NARROW_PROBE stage=race-keystroke source={} emitted={:?} local_cursor={:?} local_rows={:?}",
            conpty_source(),
            escaped(&race.raw_output[mark..]),
            race.terminal.cursor(),
            occupied_rows(&race.terminal.visible_text())
        );

        assert!(
            wide_rows
                .iter()
                .any(|row| row.trim_end() == NARROW_PROBE_LINE),
            "probe never got its hard-terminated row on screen: {:?}",
            occupied_rows(&wide_rows)
        );
    }

    /// Startup script shared by the probe children: a short prompt whose absolute row is easy to
    /// read out of a CUP, plus `BTDUMP`, which reads conhost's own buffer and viewport back to us.
    const PROBE_STARTUP_COMMON: &str = concat!(
        "Set-PSReadLineOption -HistorySaveStyle SaveNothing; ",
        "function global:prompt { 'BTP> ' }; ",
        "function global:BTDUMP { param($n) $ui=$Host.UI.RawUI; ",
        "$w=$ui.BufferSize.Width; $bh=$ui.BufferSize.Height; ",
        "$wp=$ui.WindowPosition; $ws=$ui.WindowSize; $cp=$ui.CursorPosition; ",
        "$r=New-Object System.Management.Automation.Host.Rectangle 0,0,($w-1),($n-1); ",
        "$c=$ui.GetBufferContents($r); ",
        "$t=(0..($n-1) | ForEach-Object { $y=$_; ",
        "'R'+$y+'<'+(-join (0..($w-1) | ForEach-Object { $c[$y,$_].Character })).TrimEnd()+'>' ",
        "}) -join ' '; ",
        "Write-Output ('BTDUMPBEGIN w='+$w+' bh='+$bh+' wp='+$wp.X+','+$wp.Y",
        "+' ws='+$ws.Width+'x'+$ws.Height+' cp='+$cp.X+','+$cp.Y+' '+$t+'BTDUMPEND') };",
    );

    /// The seat recording's shape: the over-long hard-terminated row is printed by the startup
    /// script, so the prompt sits directly beneath it with nothing above and no command echo.
    fn narrow_probe_clean_child() -> InteractiveOracle {
        let startup = format!("{PROBE_STARTUP_COMMON} Write-Host '{NARROW_PROBE_LINE}'");
        let mut oracle = InteractiveOracle::spawn_with(&startup, NARROW_PROBE_WIDE, 26);
        oracle.wait_for_current_line("BTP>");
        oracle.pump_until_quiet(Duration::from_secs(10));
        oracle
    }

    /// The seat recording's own geometry: a 39-column prompt (the width of
    /// `(base) PS D:\Developer\BetterTerminal> `) directly beneath one 49-column hard-terminated
    /// row. Prompt width is the variable that decides whether a stale absolute row anchor is
    /// visible: the line editor re-addresses column 39 on whatever row it still believes in.
    const SEAT_PROMPT: &str = "BTSEAT D:\\Developer\\BetterTerminal> ";

    /// Typed at the wide width before the drag, so the line editor enters the resize holding a
    /// non-empty buffer and a previous render — and long enough that re-rendering it past the
    /// 43-column prompt wraps onto a second physical row at the narrow width, which is what the
    /// recording's redraw had to do.
    const SEAT_TYPED: &str = "BT_SEAT_TYPED_INPUT_LONG_ENOUGH_TO_WRAP";

    /// The recording's prompt is not one string: a conda hook writes `(base) ` with `Write-Host`
    /// and the prompt function returns the rest, so the line editor's own idea of the prompt is
    /// shorter than the column its cursor actually starts in. Reproduce that split exactly.
    fn narrow_probe_seat_child() -> InteractiveOracle {
        let startup = format!(
            "Set-PSReadLineOption -HistorySaveStyle SaveNothing; \
             function global:prompt {{ Write-Host -NoNewline '(base) '; '{SEAT_PROMPT}' }}; \
             Write-Host '{NARROW_PROBE_LINE}'"
        );
        let mut oracle =
            InteractiveOracle::spawn_shell_with("pwsh.exe", &startup, NARROW_PROBE_WIDE, 26);
        oracle.wait_for_current_line(&format!("(base) {}", SEAT_PROMPT.trim_end()));
        oracle.pump_until_quiet(Duration::from_secs(10));
        oracle
    }

    /// A live child parked at a prompt with one hard-terminated row on screen that is longer than
    /// the width the probe is about to narrow to.
    fn narrow_probe_child() -> InteractiveOracle {
        let mut oracle = InteractiveOracle::spawn_with(PROBE_STARTUP_COMMON, NARROW_PROBE_WIDE, 26);
        oracle.wait_for_current_line("BTP>");
        oracle.pump_until_quiet(Duration::from_secs(10));
        let start = oracle.raw_output.len();
        oracle.write_line(&format!("Write-Output '{NARROW_PROBE_LINE}'"));
        oracle.wait_for_output_since(start, NARROW_PROBE_LINE.as_bytes());
        oracle.pump_until_quiet(Duration::from_secs(10));
        oracle.wait_for_current_line("BTP>");
        oracle
    }

    /// One profile shape the corruption hunt can put a real shell into. The reconstruction shapes
    /// isolate single ingredients; the `Real*` shapes are the reporting host's own environment,
    /// because nothing synthesized here is as faithful as the user's own conda hook and their own
    /// dot-source of `scripts/shell-integration/folio.ps1`.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum ProfileScenario {
        /// A plain one-part prompt, no integration: the control.
        Bare,
        /// A conda-style two-part prompt: a profile function writes its prefix with `Write-Host`
        /// and returns the rest, so the column the cursor starts in is wider than the string the
        /// line editor was handed.
        Conda,
        /// Folio's own OSC 133 / OSC 7 wrappers over a plain prompt.
        Integration,
        /// Both, in the order `$PROFILE` produces them: conda's `profile.ps1` first, our
        /// dot-source from `Microsoft.PowerShell_profile.ps1` second.
        CondaIntegration,
        /// The host's real conda hook plus the real integration script, same order.
        RealCondaIntegration,
        /// The host's own `$PROFILE` chain, run untouched.
        RealProfile,
    }

    impl ProfileScenario {
        fn label(self) -> &'static str {
            match self {
                Self::Bare => "bare",
                Self::Conda => "conda",
                Self::Integration => "integration",
                Self::CondaIntegration => "conda+integration",
                Self::RealCondaIntegration => "real-conda+real-integration",
                Self::RealProfile => "real-profile",
            }
        }

        /// The host's own profile chain is the only shape that must not be suppressed.
        fn loads_profile(self) -> bool {
            self == Self::RealProfile
        }

        /// Whether the probe can predict the prompt text, or has to read it off the settled screen.
        fn synthetic_prompt(self) -> Option<String> {
            match self {
                Self::Bare | Self::Integration => Some(PROFILE_PROBE_PROMPT.to_owned()),
                Self::Conda | Self::CondaIntegration => {
                    Some(format!("(base) {PROFILE_PROBE_PROMPT}"))
                }
                Self::RealCondaIntegration | Self::RealProfile => None,
            }
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct ProfileShape {
        scenario: ProfileScenario,
        /// A screen already full of scrollback, some of it long enough to re-wrap at the narrow
        /// width. Reflow then scrolls the prompt, which is what a stale anchor is measured against.
        filled: bool,
    }

    impl ProfileShape {
        fn label(&self) -> String {
            format!("{} filled={}", self.scenario.label(), self.filled)
        }
    }

    /// The reporting host's own prompt, minus the conda prefix that the hook writes separately.
    const PROFILE_PROBE_PROMPT: &str = "PS D:\\Developer\\BetterTerminal> ";
    /// The line the user's screenshot shows being overwritten; it is conda's own startup noise.
    const PROFILE_PROBE_NOISE: &str = "Did not find path entry D:\\App\\Base\\anaconda3\\bin";
    /// The command in the user's screenshot: typed, never submitted, and long enough that the
    /// prompt plus the input wraps once the pane narrows.
    const PROFILE_PROBE_TYPED: &str =
        "echo D:\\Developer\\BetterTerminal\\local-images\\sunset.svg";
    /// The same command, long enough that prompt plus input **already** occupies two rows at
    /// `PROFILE_PROBE_WIDE`. This is the one property of the reported gesture that no earlier probe
    /// shape varied: `PROFILE_PROBE_TYPED` is deliberately short enough to fit on one row until the
    /// pane narrows, and a line editor that re-derives its render anchor from the post-resize
    /// cursor is only wrong by the number of rows the input occupied *before* the resize.
    const PROFILE_PROBE_TYPED_WRAPPED: &str =
        "echo D:\\Developer\\BetterTerminal\\local-images\\sunset-wrapped2.svg";
    const PROFILE_PROBE_WIDE: u16 = 100;
    const PROFILE_PROBE_NARROW: u16 = 70;
    const PROFILE_PROBE_ROWS: u16 = 26;

    /// The repository root as a plain path. Deliberately not canonicalized: `canonicalize` returns
    /// a `\?\` extended-length path, and the prompt is a function of the working directory, so
    /// that would change the very geometry the recorded gesture is measured in.
    fn repository_root() -> PathBuf {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        manifest
            .parent()
            .and_then(Path::parent)
            .expect("the crate is two levels below the repository root")
            .to_path_buf()
    }

    fn integration_script_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("scripts")
            .join("shell-integration")
            .join("folio.ps1")
    }

    /// Rebuild the reporting host's profile in one `-Command` startup: the base prompt, conda's
    /// `Write-Host`-prefixed wrapper around it, then our integration script wrapping *that* — the
    /// same order `$PROFILE` produces, because `profile.ps1` (conda) runs before
    /// `Microsoft.PowerShell_profile.ps1` (our dot-source).
    fn profile_probe_startup(shape: ProfileShape) -> String {
        let mut startup = String::from("Set-PSReadLineOption -HistorySaveStyle SaveNothing; ");
        match shape.scenario {
            ProfileScenario::Bare | ProfileScenario::Integration => {
                startup.push_str(&format!(
                    "function global:prompt {{ '{PROFILE_PROBE_PROMPT}' }}; "
                ));
            }
            ProfileScenario::Conda | ProfileScenario::CondaIntegration => {
                startup.push_str(&format!(
                    "function global:prompt {{ '{PROFILE_PROBE_PROMPT}' }}; "
                ));
                startup.push_str(
                    "$global:BTBASEPROMPT = (Get-Command prompt -CommandType Function).ScriptBlock; \
                     function global:prompt { Write-Host -NoNewline '(base) '; \
                     & $global:BTBASEPROMPT }; ",
                );
            }
            ProfileScenario::RealCondaIntegration => {
                startup.push_str(
                    "$btconda = Get-Command conda.exe -ErrorAction SilentlyContinue; \
                     if ($btconda) { (& $btconda.Source shell.powershell hook) | Out-String | \
                     Invoke-Expression }; ",
                );
            }
            ProfileScenario::RealProfile => {}
        }
        if matches!(
            shape.scenario,
            ProfileScenario::Integration
                | ProfileScenario::CondaIntegration
                | ProfileScenario::RealCondaIntegration
        ) {
            startup.push_str(&format!(". '{}'; ", integration_script_path().display()));
        }
        if matches!(shape.scenario, ProfileScenario::RealProfile) {
            // The reporting host's own profile prints the conda noise line; adding the probe's copy
            // would push the prompt down a row and change the very geometry under test.
            return startup;
        }
        if shape.filled {
            startup.push_str(
                "0..17 | ForEach-Object { Write-Host (('BTSHORT{0:D2} ' -f $_) + ('s' * 26)) }; \
                 0..2 | ForEach-Object { Write-Host (('BTLONG{0:D2} ' -f $_) + ('L' * 66)) }; ",
            );
        }
        startup.push_str(&format!("Write-Host '{PROFILE_PROBE_NOISE}'"));
        startup
    }

    /// Park a live child at its prompt and report what that prompt's text actually is. A shape the
    /// probe cannot predict (the host's own profile) is read off the settled screen rather than
    /// asserted, so an unexpected prompt is an observation instead of a panic.
    fn profile_probe_child(
        shell: &str,
        shape: ProfileShape,
        columns: u16,
        rows: u16,
    ) -> Option<(InteractiveOracle, String)> {
        let mut oracle = InteractiveOracle::spawn_shell_profile(
            shell,
            &profile_probe_startup(shape),
            columns,
            rows,
            shape.scenario.loads_profile(),
        );
        let expected = shape.scenario.synthetic_prompt();
        let deadline = Instant::now() + Duration::from_secs(25);
        while Instant::now() < deadline {
            oracle.pump_once();
            let settled = match &expected {
                Some(prompt) => oracle.current_line() == prompt.trim_end(),
                None => oracle.current_line().ends_with('>'),
            };
            if settled {
                oracle.pump_until_quiet(Duration::from_secs(10));
                let line = oracle.current_line();
                let still_settled = match &expected {
                    Some(prompt) => line == prompt.trim_end(),
                    None => line.ends_with('>'),
                };
                if still_settled {
                    // The trailing space a prompt ends with is trimmed out of the grid row, and
                    // every prompt this probe drives ends in one.
                    return Some((oracle, format!("{line} ")));
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        eprintln!(
            "BT_CONPTY_PROFILE_PROBE shell={shell} {} SETUP-FAILED line={:?} rows={:?}",
            shape.label(),
            oracle.current_line(),
            occupied_rows(&oracle.terminal.visible_text())
        );
        None
    }

    #[derive(Debug)]
    struct ProfileProbeOutcome {
        shell: String,
        shape: ProfileShape,
        clean: bool,
        input_line: String,
        expected_input_line: String,
        noise_intact: bool,
        emitted: String,
        before_rows: Vec<(usize, String)>,
        settled_rows: Vec<(usize, String)>,
        final_rows: Vec<(usize, String)>,
        final_cursor: TerminalCursor,
    }

    /// Concatenate the visual rows the prompt's logical line occupies, from the row the prompt
    /// starts on through the cursor row. A stale render anchor shows up here as either a missing
    /// prompt row or a prompt line with foreign text spliced into it.
    fn profile_probe_input_line(oracle: &InteractiveOracle, prompt: &str) -> String {
        let opening = prompt.trim_end();
        let rows = oracle.terminal.visible_text();
        let cursor_row = (oracle.terminal.cursor().row as usize).min(rows.len().saturating_sub(1));
        (0..=cursor_row)
            .rev()
            .find(|row| rows[*row].starts_with(opening))
            .map(|start| rows[start..=cursor_row].concat())
            .unwrap_or_default()
    }

    /// One run of the user's exact gesture: type without submitting, drag the divider narrower by
    /// thirty columns through a resize transaction (many local widths, one committed pseudoconsole
    /// size, the vendor reconcile in between — the product's own path), then one more keystroke.
    fn run_profile_probe(shell: &str, shape: ProfileShape) -> Option<ProfileProbeOutcome> {
        let (mut oracle, prompt) =
            profile_probe_child(shell, shape, PROFILE_PROBE_WIDE, PROFILE_PROBE_ROWS)?;

        oracle
            .session
            .write(PROFILE_PROBE_TYPED.as_bytes())
            .unwrap();
        oracle.pump_for(Duration::from_millis(400));
        let before_rows = occupied_rows(&oracle.terminal.visible_text());

        let drag = [96u16, 91, 87, 84, 80, 77, 74, 72, PROFILE_PROBE_NARROW];
        oracle.terminal.begin_resize_transaction();
        for width in drag {
            oracle.resize_terminal(width, PROFILE_PROBE_ROWS);
            oracle.pump_for(Duration::from_millis(4));
        }
        oracle.resize_conpty(PROFILE_PROBE_NARROW, PROFILE_PROBE_ROWS);
        oracle.terminal.reconcile_resize_transaction_to_viewport();
        oracle.pump_for(Duration::from_millis(500));
        oracle.terminal.finish_resize_transaction();
        let settled_rows = occupied_rows(&oracle.terminal.visible_text());

        let mark = oracle.raw_output.len();
        oracle.session.write(b"Z").unwrap();
        oracle.pump_for(Duration::from_millis(900));

        let input_line = profile_probe_input_line(&oracle, &prompt);
        let expected_input_line = format!("{prompt}{PROFILE_PROBE_TYPED}Z");
        let final_rows = occupied_rows(&oracle.terminal.visible_text());
        let noise_intact = final_rows
            .iter()
            .any(|(_, row)| row.trim_end() == PROFILE_PROBE_NOISE)
            || final_rows
                .windows(2)
                .any(|pair| format!("{}{}", pair[0].1, pair[1].1).contains(PROFILE_PROBE_NOISE));
        Some(ProfileProbeOutcome {
            shell: shell.to_owned(),
            shape,
            clean: input_line == expected_input_line && noise_intact,
            input_line,
            expected_input_line,
            noise_intact,
            emitted: escaped(&oracle.raw_output[mark..]),
            before_rows,
            settled_rows,
            final_rows,
            final_cursor: oracle.terminal.cursor(),
        })
    }

    /// Dev probe for the corruption the user hits every time they narrow a pane mid-edit, run
    /// against the *reporting host's own profile shape* rather than a sterile prompt.
    ///
    /// Read the `BT_CONPTY_PROFILE_PROBE` lines on stderr. Each row of the sweep is one live child;
    /// the shapes isolate whether the conda-style two-part prompt, Folio's own shell
    /// integration, the host's real profile chain, or a screen full of scrollback is what turns a
    /// narrowing resize into a stale render anchor.
    #[test]
    #[ignore = "dev probe: drives real interactive shells through ConPTY; host-timing sensitive"]
    fn profile_shape_narrow_resize_probe() {
        let source = conpty_source();
        let mut outcomes = Vec::new();
        for shell in ["powershell.exe", "pwsh.exe"] {
            for filled in [false, true] {
                for scenario in [
                    ProfileScenario::Bare,
                    ProfileScenario::Conda,
                    ProfileScenario::Integration,
                    ProfileScenario::CondaIntegration,
                    ProfileScenario::RealCondaIntegration,
                    ProfileScenario::RealProfile,
                ] {
                    let shape = ProfileShape { scenario, filled };
                    let Some(outcome) = run_profile_probe(shell, shape) else {
                        continue;
                    };
                    eprintln!(
                        "BT_CONPTY_PROFILE_PROBE source={source} shell={shell} {} clean={} \
                         input={:?} expected={:?} noise_intact={} cursor={:?}",
                        shape.label(),
                        outcome.clean,
                        outcome.input_line,
                        outcome.expected_input_line,
                        outcome.noise_intact,
                        outcome.final_cursor
                    );
                    eprintln!(
                        "BT_CONPTY_PROFILE_PROBE_DETAIL shell={shell} {} emitted={:?}",
                        shape.label(),
                        outcome.emitted
                    );
                    for (stage, rows) in [
                        ("before", &outcome.before_rows),
                        ("settled", &outcome.settled_rows),
                        ("final", &outcome.final_rows),
                    ] {
                        eprintln!(
                            "BT_CONPTY_PROFILE_PROBE_ROWS shell={shell} {} {stage}={rows:?}",
                            shape.label()
                        );
                    }
                    outcomes.push(outcome);
                }
            }
        }
        eprintln!("BT_CONPTY_PROFILE_PROBE_SUMMARY source={source}");
        for outcome in &outcomes {
            eprintln!(
                "  {:<15} {:<40} -> {}",
                outcome.shell,
                outcome.shape.label(),
                if outcome.clean { "clean" } else { "CORRUPT" }
            );
        }
        assert!(
            !outcomes.is_empty(),
            "the sweep must actually have driven at least one live shell"
        );
    }

    /// One `CSI 6 n` exchange, as this terminal answered it.
    ///
    /// Under the pinned sidecar the answer is not a diagnostic: ConPTY adopts the row we report as
    /// the child's own cursor, so a reply computed from a grid the child does not have yet is a
    /// corruption *this terminal causes*. `local_columns` is the width our grid was on when we
    /// answered; the child was on whatever width the last committed pseudoconsole resize gave it.
    #[derive(Debug)]
    struct CprExchange {
        reply: String,
        local_columns: u16,
        cursor: TerminalCursor,
    }

    #[derive(Debug)]
    struct MistimedCprOutcome {
        shell: String,
        staggered: bool,
        clean: bool,
        input_line: String,
        expected_input_line: String,
        exchanges: Vec<CprExchange>,
        emitted: String,
        final_rows: Vec<(usize, String)>,
    }

    /// The mistimed-CPR hypothesis, tested rather than argued.
    ///
    /// A drag that pauses longer than the app's ConPTY quiet window commits *two* pseudoconsole
    /// resizes. Each one makes the sidecar ask `CSI 6 n`. Nothing makes the child's question arrive
    /// while our grid is still on the width that question was asked about — so if a second drag has
    /// already moved our grid, we answer the first question from the second grid, and the sidecar
    /// writes that stale row into the child. `staggered=false` answers every DSR from the grid the
    /// child actually has; `staggered=true` answers the first one from the next drag's grid, which
    /// is exactly what a two-stage divider drag produces.
    fn run_mistimed_cpr_probe(shell: &str, staggered: bool) -> Option<MistimedCprOutcome> {
        let shape = ProfileShape {
            scenario: ProfileScenario::CondaIntegration,
            filled: true,
        };
        let (mut oracle, prompt) =
            profile_probe_child(shell, shape, PROFILE_PROBE_WIDE, PROFILE_PROBE_ROWS)?;
        oracle
            .session
            .write(PROFILE_PROBE_TYPED.as_bytes())
            .unwrap();
        oracle.pump_for(Duration::from_millis(400));

        const FIRST: u16 = 85;
        const SECOND: u16 = 70;
        oracle.cpr_columns = PROFILE_PROBE_WIDE;

        // Drag one, committed to the pseudoconsole exactly as the app's quiet window would.
        oracle.terminal.begin_resize_transaction();
        for width in [96u16, 92, 88, FIRST] {
            oracle.resize_terminal(width, PROFILE_PROBE_ROWS);
            oracle.cpr_columns = width;
            oracle.pump_for(Duration::from_millis(4));
        }
        oracle.resize_conpty(FIRST, PROFILE_PROBE_ROWS);
        oracle.terminal.reconcile_resize_transaction_to_viewport();
        oracle.terminal.finish_resize_transaction();
        if !staggered {
            // Let the child's question be answered by the grid the child actually has.
            oracle.pump_for(Duration::from_millis(500));
        }

        // Drag two. In the staggered run the first drag's `CSI 6 n` is still unread in the pipe.
        oracle.terminal.begin_resize_transaction();
        for width in [82u16, 78, 74, SECOND] {
            oracle.resize_terminal(width, PROFILE_PROBE_ROWS);
            oracle.cpr_columns = width;
            if staggered {
                oracle.pump_for(Duration::from_millis(4));
            }
        }
        oracle.terminal.reconcile_resize_transaction_to_viewport();
        oracle.pump_for(Duration::from_millis(300));
        oracle.resize_conpty(SECOND, PROFILE_PROBE_ROWS);
        oracle.terminal.reconcile_resize_transaction_to_viewport();
        oracle.pump_for(Duration::from_millis(500));
        oracle.terminal.finish_resize_transaction();

        let mark = oracle.raw_output.len();
        oracle.session.write(b"Z").unwrap();
        oracle.pump_for(Duration::from_millis(900));

        let input_line = profile_probe_input_line(&oracle, &prompt);
        let expected_input_line = format!("{prompt}{PROFILE_PROBE_TYPED}Z");
        Some(MistimedCprOutcome {
            shell: shell.to_owned(),
            staggered,
            clean: input_line == expected_input_line,
            input_line,
            expected_input_line,
            exchanges: std::mem::take(&mut oracle.cpr_log),
            emitted: escaped(&oracle.raw_output[mark..]),
            final_rows: occupied_rows(&oracle.terminal.visible_text()),
        })
    }

    /// Dev probe: does a two-stage drag make *this terminal* hand the sidecar a stale cursor row?
    #[test]
    #[ignore = "dev probe: drives real interactive shells through ConPTY; host-timing sensitive"]
    fn mistimed_cpr_narrow_resize_probe() {
        let source = conpty_source();
        let mut outcomes = Vec::new();
        for shell in ["powershell.exe", "pwsh.exe"] {
            for staggered in [false, true] {
                let Some(outcome) = run_mistimed_cpr_probe(shell, staggered) else {
                    continue;
                };
                eprintln!(
                    "BT_CONPTY_CPR_PROBE source={source} shell={shell} staggered={staggered} \
                     clean={} input={:?} expected={:?}",
                    outcome.clean, outcome.input_line, outcome.expected_input_line
                );
                for exchange in &outcome.exchanges {
                    eprintln!(
                        "BT_CONPTY_CPR_PROBE_EXCHANGE shell={shell} staggered={staggered} \
                         reply={:?} answered_from_columns={} cursor={:?}",
                        exchange.reply, exchange.local_columns, exchange.cursor
                    );
                }
                eprintln!(
                    "BT_CONPTY_CPR_PROBE_EXCHANGE_COUNT shell={shell} staggered={staggered} {}",
                    outcome.exchanges.len()
                );
                eprintln!(
                    "BT_CONPTY_CPR_PROBE_DETAIL shell={shell} staggered={staggered} emitted={:?}",
                    outcome.emitted
                );
                eprintln!(
                    "BT_CONPTY_CPR_PROBE_ROWS shell={shell} staggered={staggered} final={:?}",
                    outcome.final_rows
                );
                outcomes.push(outcome);
            }
        }
        eprintln!("BT_CONPTY_CPR_PROBE_SUMMARY source={source}");
        for outcome in &outcomes {
            eprintln!(
                "  {:<15} staggered={:<5} -> {}",
                outcome.shell,
                outcome.staggered,
                if outcome.clean { "clean" } else { "CORRUPT" }
            );
        }
        assert!(!outcomes.is_empty(), "the probe drove no live shell");
    }

    /// The application's own resize loop, driven against a live child.
    ///
    /// `bt-app` projects every `Resized` onto the terminal immediately (`resize_at`) and commits
    /// only the last size to the pseudoconsole after `RESIZE_REQUEST_QUIET` of pointer silence
    /// (`take_due_pty_resize` -> `PtySession::resize` -> `mark_pty_resize_requested_at`), closing the
    /// transaction only once that request *and* the child's output have both been quiet
    /// (`finish_resize_if_quiescent`). Everything below drives exactly that loop, so a burst shape
    /// which corrupts here corrupts in the product, and the frame it is judged on is the composed
    /// one the user actually reads — frozen transcript, staging and live rows together.
    struct AppResizeOracle {
        pty: PtySession,
        session: DualPlaneSession,
        raw_output: Vec<u8>,
        cpr_log: Vec<AppCprExchange>,
        pending: Option<(u16, u16, Instant)>,
        commits: Vec<(u16, u16)>,
        /// The typed-input ConPTY resize gate (user ruling 2026-08-04). `false` is the loop exactly
        /// as it was before the mitigation, which is what makes the before/after pair one probe.
        typed_input_gate: bool,
        /// `bt-app::PendingPtyResize::blank_since`: when the gate started answering "empty" for the
        /// queued request. A real child's redraw arrives in whatever pieces a read returns, so a
        /// single "empty" sample is not an empty buffer — only an unbroken quiet window of them is.
        blank_since: Option<Instant>,
        /// Whether that window is honoured. `false` is the loop as it was before
        /// confirm-then-release, which is what makes the blank-window probe's pair one probe.
        confirm_blank_gate: bool,
        /// Product channel under test: after a successful ConPTY resize, send the shipped private
        /// chord only while the live OSC 133 input region is open.
        invoke_prompt_after_resize: bool,
        /// Green timing: coalesce that chord until the resize transaction closes after child-output
        /// silence. `false` preserves the shipped immediate-after-commit path as a real red arm.
        reanchor_after_resize_quiescence: bool,
        pending_reanchor: bool,
        invoke_prompt_writes: usize,
        /// What our own grid holds, and what the child was last told. They are the same number at
        /// rest; the gate's whole contract is that they stay the same number across a deferral too.
        grid: (u16, u16),
        conpty: (u16, u16),
    }

    impl AppResizeOracle {
        fn spawn(shell: &str, startup: &str, columns: u16, rows: u16, load_profile: bool) -> Self {
            let mut command = PtyCommand::new(shell).arg("-NoLogo");
            if !load_profile {
                command = command.arg("-NoProfile");
            }
            let command = command.arg("-NoExit").arg("-Command").arg(startup);
            let pty = PtySession::spawn(command, size(columns, rows), no_wake()).unwrap();
            Self {
                pty,
                session: DualPlaneSession::new(nz32(columns), nz32(rows)),
                raw_output: Vec::new(),
                cpr_log: Vec::new(),
                pending: None,
                commits: Vec::new(),
                typed_input_gate: false,
                blank_since: None,
                confirm_blank_gate: true,
                invoke_prompt_after_resize: false,
                reanchor_after_resize_quiescence: true,
                pending_reanchor: false,
                invoke_prompt_writes: 0,
                grid: (columns, rows),
                conpty: (columns, rows),
            }
        }

        /// The one question the gate asks, or `false` when the probe is running the old loop.
        fn deferring(&self) -> bool {
            self.typed_input_gate && self.session.typed_shell_input_live()
        }

        /// `bt-app::sample_typed_input_gate`: record this turn's answer against the queued request.
        fn sample_gate(&mut self, now: Instant) {
            if self.pending.is_none() {
                return;
            }
            if self.deferring() {
                self.blank_since = None;
            } else {
                self.blank_since.get_or_insert(now);
            }
        }

        /// One turn of the app's event loop.
        fn pump_once(&mut self) -> bool {
            let now = Instant::now();
            let bytes = self.pty.read_output();
            let had_output = !bytes.is_empty();
            if had_output {
                self.raw_output.extend_from_slice(&bytes);
                self.session.feed_at(&bytes, now).unwrap();
            }
            for reply in self.session.take_pty_writes() {
                if reply.starts_with(b"\x1b[") && reply.ends_with(b"R") {
                    self.cpr_log.push(AppCprExchange {
                        reply: escaped(&reply),
                        local_columns: self.grid.0,
                        conpty_columns: self.conpty.0,
                        cursor: self.session.terminal().cursor(),
                    });
                }
                self.pty.write(&reply).unwrap();
            }
            self.flush_pending_resize(now);
            if self.session.finish_resize_if_quiescent(now).unwrap() && self.pending_reanchor {
                if self.session.shell_input_region_open() {
                    self.write_invoke_prompt();
                }
                self.pending_reanchor = false;
            }
            had_output
        }

        /// `bt-app::flush_pending_pty_resize`: the coalesced size reaches ConPTY and the terminal
        /// reconciles to it, both at the quiet boundary and never before.
        fn flush_pending_resize(&mut self, now: Instant) {
            self.sample_gate(now);
            let Some((columns, rows, deadline)) = self.pending else {
                return;
            };
            if now < deadline {
                return;
            }
            // The gate. Held, this returns without taking the pending request, so the request keeps
            // its final size and lands whole once the shell lets go — and letting go means the
            // buffer has read empty for an unbroken quiet window, not for one sample.
            let released = if self.confirm_blank_gate {
                self.blank_since
                    .is_some_and(|since| now >= since + RESIZE_REQUEST_QUIET)
            } else {
                // The pre-fix loop: one sample of the gate is the whole release condition.
                !self.deferring()
            };
            if !released {
                return;
            }
            self.pending = None;
            self.blank_since = None;
            if (columns, rows) != self.grid {
                // The reflow deferral owed from `project_resize`, paid here so our grid and the
                // child's change together rather than drifting apart across the drag.
                self.session
                    .resize_at(nz32(columns), nz32(rows), now)
                    .unwrap();
                self.grid = (columns, rows);
            }
            self.pty.resize(size(columns, rows)).unwrap();
            self.conpty = (columns, rows);
            if self.invoke_prompt_after_resize {
                let reanchor_owed = self.session.shell_input_region_open();
                if self.reanchor_after_resize_quiescence {
                    self.pending_reanchor = reanchor_owed;
                } else if reanchor_owed {
                    self.write_invoke_prompt();
                }
            }
            self.session
                .mark_pty_resize_requested_at(nz32(columns), nz32(rows), now);
            self.commits.push((columns, rows));
        }

        fn write_invoke_prompt(&mut self) {
            self.pty.write(PSREADLINE_INVOKE_PROMPT_INPUT).unwrap();
            self.invoke_prompt_writes += 1;
        }

        /// One `WindowEvent::Resized`: projected onto the grid at once, coalesced towards ConPTY.
        fn project_resize(&mut self, columns: u16, rows: u16) {
            let now = Instant::now();
            // A drag that comes back to the size the child already has owes it nothing, so the
            // queued intermediate size is dropped rather than replayed at release.
            self.pending = ((columns, rows) != self.conpty).then_some((
                columns,
                rows,
                now + RESIZE_REQUEST_QUIET,
            ));
            if self.pending.is_none() {
                self.blank_since = None;
            }
            // A pointer frame samples the gate like any other turn, which is what keeps a drag over
            // an idle prompt free of a second quiet window.
            self.sample_gate(now);
            if self.deferring() {
                return;
            }
            self.session
                .resize_at(nz32(columns), nz32(rows), now)
                .unwrap();
            self.grid = (columns, rows);
        }

        fn pump_for(&mut self, duration: Duration) {
            let deadline = Instant::now() + duration;
            while Instant::now() < deadline {
                self.pump_once();
                std::thread::sleep(Duration::from_millis(2));
            }
            self.pump_once();
        }

        /// Pump only until the coalescer's quiet window elapses and the size reaches ConPTY, and
        /// not one millisecond further. The child's answer to that commit — the cursor-position
        /// round trip and the line editor's redraw — is still in flight when this returns, which is
        /// the state a pointer that resumes immediately after the pause finds the session in.
        fn pump_until_committed(&mut self, maximum: Duration) {
            let deadline = Instant::now() + maximum;
            while Instant::now() < deadline {
                self.pump_once();
                if self.pending.is_none() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }

        fn pump_until_quiet(&mut self, maximum: Duration) {
            let deadline = Instant::now() + maximum;
            let mut quiet_since = Instant::now();
            while Instant::now() < deadline {
                if self.pump_once() {
                    quiet_since = Instant::now();
                } else if quiet_since.elapsed() >= Duration::from_millis(150) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        }

        /// Every drawable row of the composed frame, which is what the window presents.
        fn composed_rows(&self) -> Vec<String> {
            let mut projection = self.session.new_projection(self.session.layout_key());
            self.session.refresh_projection(&mut projection);
            let frame = self.session.viewport_frame(&mut projection).unwrap();
            let columns = frame.columns.get() as usize;
            frame
                .cells
                .chunks(columns)
                .take(frame.drawable_rows())
                .map(|row| {
                    row.iter()
                        .map(|cell| cell.text.as_str())
                        .collect::<String>()
                        .trim_end()
                        .to_owned()
                })
                .collect()
        }

        fn current_line(&self) -> String {
            let cursor = self.session.terminal().cursor();
            self.session
                .terminal()
                .visible_text()
                .get(cursor.row as usize)
                .cloned()
                .unwrap_or_default()
        }

        /// The logical input line as composed: from the row the prompt opens on down to the cursor.
        fn composed_input_line(&self, prompt: &str) -> String {
            let opening = prompt.trim_end();
            let rows = self.composed_rows();
            let cursor_row =
                (self.session.terminal().cursor().row as usize).min(rows.len().saturating_sub(1));
            (0..=cursor_row)
                .rev()
                .find(|row| rows[*row].starts_with(opening))
                .map(|start| rows[start..=cursor_row].concat())
                .unwrap_or_default()
        }

        fn settle_at_prompt(&mut self, prompt: &str) -> bool {
            self.settle_at_prompt_matching(&|line| line == prompt.trim_end())
                .is_some()
        }

        /// Park at a prompt the probe cannot predict — the host's own `$PROFILE` writes it — and
        /// report the text it settled on, with the trailing space every prompt ends in restored.
        fn settle_at_prompt_matching(&mut self, accept: &dyn Fn(&str) -> bool) -> Option<String> {
            let deadline = Instant::now() + Duration::from_secs(25);
            while Instant::now() < deadline {
                self.pump_once();
                if accept(&self.current_line()) {
                    self.pump_until_quiet(Duration::from_secs(5));
                    let line = self.current_line();
                    if accept(&line) {
                        return Some(format!("{line} "));
                    }
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            None
        }
    }

    #[derive(Debug)]
    struct AppCprExchange {
        reply: String,
        local_columns: u16,
        conpty_columns: u16,
        cursor: TerminalCursor,
    }

    /// One shape of drag, described by what the *pointer* did rather than by what it should produce.
    #[derive(Clone, Copy, Debug)]
    struct BurstShape {
        label: &'static str,
        /// How many times the pointer stops long enough for the coalescer to commit. One is the
        /// gesture every earlier probe drove; a human drag that pauses to look is more than one.
        bursts: usize,
        /// Whether the pane also shortens as it narrows: a window-edge or corner drag, whose
        /// intermediate heights are rows the child never had.
        shortens: bool,
        /// A keystroke sent between two bursts, so the child is mid-write when the drag resumes.
        keystroke_between: bool,
        /// A keystroke sent while the pointer is still moving, which is the user's own report:
        /// narrowing the pane *mid-edit*. The line editor redraws at an absolute row while the grid
        /// is passing through sizes the child was never told about.
        keystroke_mid_burst: bool,
        /// Resume the next burst without letting the child's post-commit redraw arrive first.
        resume_before_settling: bool,
        /// Wake the line editor after the drag with keys that move the cursor but leave the input
        /// buffer identical (Left then Right). The recording's repaint carries no new character, so
        /// whatever woke that editor did not change what it had to draw — and PSReadLine's
        /// unchanged-buffer re-anchor is a different branch from its changed-buffer one.
        wake_without_edit: bool,
        /// Whether the input already wraps onto a second row *before* the pointer moves. Every
        /// earlier shape typed a command that fits on one row until the pane narrows, so the line
        /// editor's post-resize anchor happened to be right; the reported gesture had already
        /// wrapped, and a stale anchor is off by exactly the rows the input occupied.
        wrapped_input: bool,
        /// The pane the gesture happens in, and how much scrollback is behind the prompt. Every
        /// earlier shape drove a full 100x26 screen, where the prompt sits on the bottom row and
        /// reflow scrolls it; the reported gesture happened in a short pane whose prompt was the
        /// second row from the top, where reflow scrolls nothing at all.
        stage: BurstStage,
    }

    /// Pane and scrollback the drag is performed in.
    #[derive(Clone, Copy, Debug)]
    struct BurstStage {
        wide: u16,
        /// Rows the pane has before the drag. The recording states the committed size but not the
        /// one before it, so this is the parameter a sweep has to cover.
        wide_rows: u16,
        narrow: u16,
        rows: u16,
        /// A screen already full of scrollback (the prompt is on the last row), versus a session
        /// that has printed one line (the prompt is the second row, and nothing can scroll).
        filled: bool,
        /// What the typed command is, when the shape asks for input that already wraps at `wide`.
        wrapped_typed: &'static str,
        /// Run the host's real `$PROFILE` chain instead of a reconstruction of it. The prompt is
        /// then read off the settled screen rather than asserted, and the editor is configured the
        /// way the reporting user actually configured it.
        real_profile: bool,
        /// How long the gesture is left alone after the last commit before anything else is sent.
        /// The line editor notices a pseudoconsole resize on its own poll, not on the resize: the
        /// recording's editor repainted 827 ms after the commit, with no keystroke involved, and a
        /// probe that stops pumping 150 ms after the child goes quiet never sees that repaint.
        settle_after_commit: Duration,
    }

    /// The stage every burst shape before the recording was driven on.
    const BURST_STAGE_FULL_SCREEN: BurstStage = BurstStage {
        wide: PROFILE_PROBE_WIDE,
        wide_rows: PROFILE_PROBE_ROWS,
        narrow: PROFILE_PROBE_NARROW,
        rows: PROFILE_PROBE_ROWS,
        filled: true,
        wrapped_typed: PROFILE_PROBE_TYPED_WRAPPED,
        real_profile: false,
        settle_after_commit: Duration::from_millis(0),
    };

    /// The stage `.tmp-repaint-capture/s12-rearm-verify.vt` was recorded on, read off the recording
    /// itself: 74 columns before the drag (the prompt plus the typed command is 76 cells, and the
    /// child placed its cursor at row 3 column 3), 61x17 committed, one printed line above the
    /// prompt, and the user's own command.
    const BURST_STAGE_RECORDED: BurstStage = BurstStage {
        wide: 74,
        wide_rows: 17,
        narrow: 61,
        rows: 17,
        filled: false,
        wrapped_typed: RECORDED_TYPED,
        real_profile: false,
        settle_after_commit: Duration::from_millis(2_500),
    };

    /// The command in the recording, byte for byte. 37 cells after a 39-cell prompt.
    const RECORDED_TYPED: &str = "Write-Output ('BT_APP_' + 'INPUT_OK')";

    impl BurstShape {
        fn typed(self) -> &'static str {
            if self.wrapped_input {
                self.stage.wrapped_typed
            } else {
                PROFILE_PROBE_TYPED
            }
        }
    }

    #[derive(Debug)]
    struct BurstOutcome {
        shell: String,
        label: &'static str,
        clean: bool,
        commits: Vec<(u16, u16)>,
        input_line: String,
        expected_input_line: String,
        noise_intact: bool,
        spliced_rows: Vec<String>,
        /// The composed frame the instant the drag quiesced, before any later keystroke could make
        /// the line editor repaint over the damage.
        settled_clean: bool,
        settled_rows: Vec<String>,
        settled_spliced_rows: Vec<String>,
        composed_rows: Vec<String>,
    }

    /// One pane size the pointer projects, in cells.
    type DragSize = (u16, u16);
    /// One burst of a drag: the sizes the pointer moves through, then the size it comes to rest at
    /// — the only one of them the coalescer commits to the pseudoconsole.
    type DragBurst = (Vec<DragSize>, DragSize);

    /// The drag, as the pointer performs it: `bursts` groups of moving positions, each group ending
    /// where the pointer comes to rest. The resting size is the one the coalescer commits, so only
    /// the final burst rests at the narrow end — an intermediate pause commits an intermediate size,
    /// which is what makes a paused drag more than one pseudoconsole resize.
    fn burst_drag_steps(shape: BurstShape) -> Vec<DragBurst> {
        const STEPS_PER_BURST: usize = 4;
        let width_span = f64::from(shape.stage.wide - shape.stage.narrow);
        let stage_row_span = f64::from(shape.stage.wide_rows) - f64::from(shape.stage.rows);
        let row_span = if shape.shortens {
            stage_row_span.max(8.0)
        } else {
            stage_row_span
        };
        let at = |fraction: f64, wobble: f64| {
            let columns = f64::from(shape.stage.wide) - width_span * fraction + wobble;
            let rows = f64::from(shape.stage.wide_rows) - row_span * fraction + wobble;
            (
                columns.round().max(12.0) as u16,
                rows.round().max(4.0) as u16,
            )
        };
        (0..shape.bursts)
            .map(|burst| {
                let start = burst as f64 / shape.bursts as f64;
                let rest = (burst + 1) as f64 / shape.bursts as f64;
                let moving = (0..STEPS_PER_BURST)
                    .map(|step| {
                        let within = (step + 1) as f64 / (STEPS_PER_BURST + 1) as f64;
                        let fraction = start + (rest - start) * within;
                        // A real pointer overshoots and comes back; the sine makes the projected
                        // sizes differ from the resting one without leaving the drag's range.
                        let wobble = (within * std::f64::consts::PI).sin() * 4.0;
                        at(fraction, wobble)
                    })
                    .collect();
                (moving, at(rest, 0.0))
            })
            .collect()
    }

    fn run_resize_burst_probe(shell: &str, shape: BurstShape) -> Option<BurstOutcome> {
        let profile = ProfileShape {
            scenario: if shape.stage.real_profile {
                ProfileScenario::RealProfile
            } else {
                ProfileScenario::CondaIntegration
            },
            filled: shape.stage.filled,
        };
        let mut startup = profile_probe_startup(profile);
        if shape.stage.real_profile {
            // The reporting session's working directory is the repository root, and the prompt is
            // a function of it. Tests run from the crate directory, so state it.
            startup = format!("Set-Location '{}'; {startup}", repository_root().display());
        }
        let mut oracle = AppResizeOracle::spawn(
            shell,
            &startup,
            shape.stage.wide,
            shape.stage.wide_rows,
            shape.stage.real_profile,
        );
        let settled = if shape.stage.real_profile {
            oracle.settle_at_prompt_matching(&|line| line.ends_with('>'))
        } else {
            let want = format!("(base) {PROFILE_PROBE_PROMPT}");
            oracle.settle_at_prompt(&want).then_some(want)
        };
        let Some(prompt) = settled else {
            eprintln!(
                "BT_CONPTY_BURST_PROBE shell={shell} {} SETUP-FAILED line={:?}",
                shape.label,
                oracle.current_line()
            );
            return None;
        };

        oracle.pty.write(shape.typed().as_bytes()).unwrap();
        oracle.pump_for(Duration::from_millis(400));

        // Whether the conda startup line the drag must not destroy is on screen at all. A stage
        // driving the host's real `$PROFILE` inherits whatever that profile prints, which for pwsh
        // is no conda hook and therefore no such line; requiring its survival there would report a
        // clean gesture as corrupt.
        let noise_present_before = has_noise(&oracle.composed_rows());
        let mut typed_after = String::new();
        let emit_mark = oracle.raw_output.len();
        let bursts = burst_drag_steps(shape);
        let last_burst = bursts.len().saturating_sub(1);
        for (index, (moving, resting)) in bursts.into_iter().enumerate() {
            for (step, (columns, rows)) in moving.into_iter().enumerate() {
                if shape.keystroke_mid_burst && index == last_burst && step == 1 {
                    oracle.pty.write(b"Z").unwrap();
                    typed_after.push('Z');
                }
                oracle.project_resize(columns, rows);
                // ~60 Hz, the rate winit delivers a live drag at.
                oracle.pump_for(Duration::from_millis(16));
            }
            oracle.project_resize(resting.0, resting.1);
            // The pointer stops: the coalescer's quiet window elapses and the size is committed.
            if shape.resume_before_settling && index != last_burst {
                oracle.pump_until_committed(Duration::from_secs(3));
            } else {
                oracle.pump_for(RESIZE_REQUEST_QUIET + Duration::from_millis(40));
            }
            if index == last_burst {
                break;
            }
            if shape.keystroke_between {
                oracle.pty.write(b"Z").unwrap();
                typed_after.push('Z');
            }
            if !shape.resume_before_settling {
                // Let the child finish answering the commit before the pointer moves again, which
                // is what an unhurried human pause looks like; the transaction stays open either way.
                oracle.pump_for(Duration::from_millis(120));
            }
        }
        oracle.pump_until_quiet(Duration::from_secs(6));
        // The editor's own resize poll fires well after the child's post-commit output goes quiet.
        oracle.pump_for(shape.stage.settle_after_commit);
        oracle.pump_until_quiet(Duration::from_secs(6));
        // What is on screen when the gesture ends. A later keystroke makes the line editor repaint
        // its whole input from a freshly asked cursor position, which can *heal* a corrupted screen,
        // so the damage has to be read here as well as after.
        let settled_rows = oracle.composed_rows();
        let settled_input_line = oracle.composed_input_line(&prompt);
        let settled_expected = format!("{prompt}{}{typed_after}", shape.typed());
        let settled_spliced_rows = spliced_prompt_rows(&settled_rows, &prompt);

        if shape.wake_without_edit {
            oracle.pty.write(b"[D").unwrap();
            oracle.pump_for(Duration::from_millis(300));
            oracle.pty.write(b"[C").unwrap();
            oracle.pump_for(Duration::from_millis(300));
            oracle.pump_until_quiet(Duration::from_secs(6));
        }
        // The trailing keystroke every earlier probe ends with: the line editor redraws its input
        // against whatever row it believes it is on.
        oracle.pty.write(b"Z").unwrap();
        typed_after.push('Z');
        oracle.pump_for(Duration::from_millis(900));
        oracle.pump_until_quiet(Duration::from_secs(6));

        let expected_input_line = format!("{prompt}{}{typed_after}", shape.typed());
        let input_line = oracle.composed_input_line(&prompt);
        let composed_rows = oracle.composed_rows();
        let noise_intact = !noise_present_before || has_noise(&composed_rows);
        let spliced_rows = spliced_prompt_rows(&composed_rows, &prompt);
        if std::env::var_os("BT_BURST_EMIT").is_some() {
            eprintln!(
                "BT_CONPTY_BURST_PROBE_EMIT shell={shell} {} emitted={:?}",
                shape.label,
                escaped(&oracle.raw_output[emit_mark..])
            );
        }
        Some(BurstOutcome {
            shell: shell.to_owned(),
            label: shape.label,
            clean: input_line == expected_input_line && noise_intact && spliced_rows.is_empty(),
            commits: oracle.commits.clone(),
            input_line,
            expected_input_line,
            noise_intact,
            settled_clean: settled_input_line == settled_expected
                && settled_spliced_rows.is_empty(),
            settled_rows,
            settled_spliced_rows,
            spliced_rows,
            composed_rows,
        })
    }

    /// Whether the conda startup line survives on the composed frame, whole or across a wrap.
    fn has_noise(rows: &[String]) -> bool {
        rows.iter().any(|row| row.trim_end() == PROFILE_PROBE_NOISE)
            || rows
                .windows(2)
                .any(|pair| format!("{}{}", pair[0], pair[1]).contains(PROFILE_PROBE_NOISE))
    }

    /// The reported artifact, stated as a property of the composed frame: a prompt that begins
    /// anywhere but at the start of a row is a redraw spliced into a row still holding an older one
    /// — "Did not fi", the typed input and the prompt merged into a single line.
    fn spliced_prompt_rows(rows: &[String], prompt: &str) -> Vec<String> {
        let opening = prompt.trim_end();
        rows.iter()
            .filter(|row| row.contains(opening) && !row.starts_with(opening))
            .cloned()
            .collect()
    }

    fn cursor_addresses(bytes: &[u8]) -> Vec<(u16, u16)> {
        let mut addresses = Vec::new();
        let mut index = 0;
        while index + 3 < bytes.len() {
            if bytes[index] != 0x1b || bytes[index + 1] != b'[' {
                index += 1;
                continue;
            }
            let start = index + 2;
            let Some(end_offset) = bytes[start..]
                .iter()
                .position(|byte| byte.is_ascii_alphabetic())
            else {
                break;
            };
            let end = start + end_offset;
            if matches!(bytes[end], b'H' | b'f')
                && let Some(separator) = bytes[start..end].iter().position(|byte| *byte == b';')
            {
                let separator = start + separator;
                if let (Ok(row), Ok(column)) = (
                    std::str::from_utf8(&bytes[start..separator])
                        .unwrap_or_default()
                        .parse(),
                    std::str::from_utf8(&bytes[separator + 1..end])
                        .unwrap_or_default()
                        .parse(),
                ) {
                    addresses.push((row, column));
                }
            }
            index = end + 1;
        }
        addresses
    }

    fn redraw_addresses(bytes: &[u8]) -> Vec<(u16, u16)> {
        let hide = b"\x1b[?25l";
        bytes
            .windows(hide.len())
            .position(|window| window == hide)
            .map_or_else(Vec::new, |position| {
                cursor_addresses(&bytes[position + hide.len()..])
            })
    }

    fn wrapped_input_line(rows: &[String], cursor_row: u32, prompt: &str, columns: u16) -> String {
        let opening = &prompt[..usize::from(columns).min(prompt.len())];
        let cursor_row = (cursor_row as usize).min(rows.len().saturating_sub(1));
        (0..=cursor_row)
            .rev()
            .find(|row| rows[*row].starts_with(opening))
            .map(|start| rows[start..=cursor_row].concat().trim_end().to_owned())
            .unwrap_or_default()
    }

    fn psreadline_version(bytes: &[u8]) -> String {
        let text = String::from_utf8_lossy(bytes);
        let marker = "BT_PSRL_VERSION=";
        text.find(marker)
            .map(|start| &text[start + marker.len()..])
            .and_then(|tail| tail.split(['\u{7}', '\r', '\n']).next())
            .unwrap_or("unknown")
            .to_owned()
    }

    /// Dev probe: does a drag that commits *more than one* pseudoconsole size corrupt the child's
    /// screen, and which property of the burst is responsible?
    ///
    /// Every earlier narrow-resize probe drove one committed resize, because that is what a single
    /// coalescing window produces. The application's window is 200 ms of pointer silence and its
    /// transaction closes only after the *child* has also been quiet, so an ordinary human drag —
    /// move, pause to look, move again — commits twice or more inside one open transaction. The
    /// shapes below vary exactly one property at a time: the number of commits, whether the pane
    /// also shortens, whether a keystroke lands between the bursts, and whether the pointer resumes
    /// before the child's answer to the previous commit has arrived.
    ///
    /// The recorded shapes come from `.tmp-repaint-capture/s12-rearm-verify.vt`, the reporting
    /// user's own post-fix session: one commit, 74 columns down to 61x17, one printed line above
    /// the prompt, and a typed command that already wraps. That last property is the one no earlier
    /// shape varied, and the recording's own bytes show why it matters — a line editor that
    /// re-derives its anchor from the post-resize cursor is wrong by the rows its input occupied.
    ///
    /// Read the `BT_CONPTY_BURST_PROBE` lines on stderr. `BT_BURST_ONLY=<label>` drives a single
    /// shape (the full matrix is two shells by every shape, which is minutes of live child), and
    /// `BT_BURST_EMIT=1` dumps the child's own bytes for the gesture, which is what a claim about
    /// where the child anchored its redraw has to be read off.
    #[test]
    #[ignore = "dev probe: drives real interactive shells through ConPTY; host-timing sensitive"]
    fn resize_burst_composed_frame_probe() {
        let source = conpty_source();
        // One property varies per row, against the same control.
        let control = BurstShape {
            label: "one-commit",
            bursts: 1,
            shortens: false,
            keystroke_between: false,
            keystroke_mid_burst: false,
            resume_before_settling: false,
            wake_without_edit: false,
            wrapped_input: false,
            stage: BURST_STAGE_FULL_SCREEN,
        };
        let shapes = [
            control,
            BurstShape {
                label: "one-commit-mid-edit",
                keystroke_mid_burst: true,
                ..control
            },
            BurstShape {
                label: "two-commit",
                bursts: 2,
                ..control
            },
            BurstShape {
                label: "two-commit-shortens",
                bursts: 2,
                shortens: true,
                ..control
            },
            BurstShape {
                label: "two-commit-between",
                bursts: 2,
                shortens: true,
                keystroke_between: true,
                ..control
            },
            BurstShape {
                label: "two-commit-unsettled",
                bursts: 2,
                shortens: true,
                resume_before_settling: true,
                ..control
            },
            BurstShape {
                label: "two-commit-mid-edit",
                bursts: 2,
                shortens: true,
                keystroke_mid_burst: true,
                resume_before_settling: true,
                ..control
            },
            BurstShape {
                label: "four-commit-mid-edit",
                bursts: 4,
                shortens: true,
                keystroke_mid_burst: true,
                resume_before_settling: true,
                ..control
            },
            // The reported gesture, isolated: the input already wraps when the drag begins, and
            // nothing else differs from `control`.
            BurstShape {
                label: "one-commit-wrapped",
                wrapped_input: true,
                ..control
            },
            BurstShape {
                label: "two-commit-wrapped",
                bursts: 2,
                wrapped_input: true,
                ..control
            },
            // The recording's own pane, one property at a time: the short, nearly empty screen
            // whose prompt cannot scroll, then that screen with the input already wrapped, which
            // together are the gesture `s12-rearm-verify.vt` captured.
            BurstShape {
                label: "recorded-pane",
                stage: BURST_STAGE_RECORDED,
                ..control
            },
            BurstShape {
                label: "recorded-gesture",
                wrapped_input: true,
                stage: BURST_STAGE_RECORDED,
                ..control
            },
            BurstShape {
                label: "recorded-gesture-real-profile",
                wrapped_input: true,
                stage: BurstStage {
                    real_profile: true,
                    ..BURST_STAGE_RECORDED
                },
                ..control
            },
            BurstShape {
                label: "recorded-gesture-woken",
                wrapped_input: true,
                wake_without_edit: true,
                stage: BURST_STAGE_RECORDED,
                ..control
            },
            BurstShape {
                label: "recorded-gesture-woken-real-profile",
                wrapped_input: true,
                wake_without_edit: true,
                stage: BurstStage {
                    real_profile: true,
                    ..BURST_STAGE_RECORDED
                },
                ..control
            },
            BurstShape {
                label: "recorded-gesture-taller",
                wrapped_input: true,
                stage: BurstStage {
                    wide_rows: 25,
                    ..BURST_STAGE_RECORDED
                },
                ..control
            },
            BurstShape {
                label: "recorded-gesture-shorter",
                wrapped_input: true,
                stage: BurstStage {
                    wide_rows: 12,
                    ..BURST_STAGE_RECORDED
                },
                ..control
            },
            BurstShape {
                label: "recorded-gesture-filled",
                wrapped_input: true,
                stage: BurstStage {
                    filled: true,
                    ..BURST_STAGE_RECORDED
                },
                ..control
            },
            BurstShape {
                label: "recorded-gesture-two-commit",
                bursts: 2,
                wrapped_input: true,
                stage: BURST_STAGE_RECORDED,
                ..control
            },
        ];
        let only = std::env::var("BT_BURST_ONLY").ok();
        let mut outcomes = Vec::new();
        for shell in ["powershell.exe", "pwsh.exe"] {
            for shape in shapes {
                if only.as_deref().is_some_and(|want| want != shape.label) {
                    continue;
                }
                let Some(outcome) = run_resize_burst_probe(shell, shape) else {
                    continue;
                };
                eprintln!(
                    "BT_CONPTY_BURST_PROBE source={source} shell={shell} {} clean={} \
                     settled_clean={} commits={:?} input={:?} expected={:?} noise_intact={} \
                     spliced={:?} settled_spliced={:?}",
                    outcome.label,
                    outcome.clean,
                    outcome.settled_clean,
                    outcome.commits,
                    outcome.input_line,
                    outcome.expected_input_line,
                    outcome.noise_intact,
                    outcome.spliced_rows,
                    outcome.settled_spliced_rows,
                );
                if !outcome.clean || !outcome.settled_clean {
                    eprintln!(
                        "BT_CONPTY_BURST_PROBE_ROWS shell={shell} {} settled={:?} composed={:?}",
                        outcome.label, outcome.settled_rows, outcome.composed_rows
                    );
                }
                outcomes.push(outcome);
            }
        }
        eprintln!("BT_CONPTY_BURST_PROBE_SUMMARY source={source}");
        for outcome in &outcomes {
            eprintln!(
                "  {:<15} {:<22} commits={} -> settled {:<7} after-keystroke {}",
                outcome.shell,
                outcome.label,
                outcome.commits.len(),
                if outcome.settled_clean {
                    "clean"
                } else {
                    "CORRUPT"
                },
                if outcome.clean { "clean" } else { "CORRUPT" }
            );
        }
        assert!(!outcomes.is_empty(), "the probe drove no live shell");
    }

    /// The 2026-08-04 minimal repro, as geometry.
    ///
    /// The upstream rule the forensic run established: PSReadLine reduces its cached render-anchor
    /// *column* modulo the new width when the pane narrows, and never restores it when the pane
    /// widens again. Narrowing below the prompt's own width is what makes the reduced column land
    /// somewhere the prompt does not reach, and a non-empty buffer across that narrowing is what
    /// makes the stale anchor survive to be re-used. So the prompt is deliberately wide, the narrow
    /// step is deliberately narrower than it, and the widening step deliberately returns.
    const DEFER_PROBE_PROMPT: &str = "BTDEFER D:\\Developer\\BetterTerminal> ";
    /// Long enough that prompt plus input already occupies two rows at `DEFER_PROBE_WIDE`. The
    /// forensic sweep found this to be the decisive property: a line editor that re-derives its
    /// anchor from the post-resize cursor is wrong by exactly the rows its input occupied.
    const DEFER_PROBE_TYPED: &str =
        "echo D:\\Developer\\BetterTerminal\\local-images\\sunset-wrapped2.svg";
    const DEFER_PROBE_NOISE: &str = "BTDEFER_NOISE_ROW_MUST_SURVIVE";
    const DEFER_PROBE_WIDE: u16 = 100;
    const DEFER_PROBE_NARROW: u16 = 24;
    const DEFER_PROBE_ROWS: u16 = 17;
    /// The line editor notices a pseudoconsole resize on its own poll, not on the resize itself:
    /// the reporting user's recording repainted 827 ms after the commit with no keystroke involved,
    /// so a probe that stops pumping when the child goes quiet never sees the repaint that carries
    /// the stale anchor. Same wait, same reason, as `BURST_STAGE_RECORDED`.
    const DEFER_PROBE_SETTLE: Duration = Duration::from_millis(2_500);

    /// The prompt above, then Folio's own shell integration on top of it. The OSC 133 pair
    /// is not decoration here: it *is* the mitigation's information source. Without `A`/`B` the
    /// terminal has never been told where input begins, `typed_shell_input_live` answers `false`,
    /// and the gate correctly declines to act — which is the honest-degradation half of the ruling
    /// and is why this probe dot-sources the script the product ships.
    fn defer_probe_startup() -> String {
        format!(
            "Set-PSReadLineOption -HistorySaveStyle SaveNothing; \
             function global:prompt {{ '{DEFER_PROBE_PROMPT}' }}; \
             . '{}'; Write-Host '{DEFER_PROBE_NOISE}'",
            integration_script_path().display()
        )
    }

    #[derive(Debug)]
    struct DeferProbeOutcome {
        gated: bool,
        /// Whether the terminal ever saw an armed input region — the mitigation's precondition.
        armed: bool,
        clean: bool,
        input_line: String,
        expected_input_line: String,
        noise_intact: bool,
        /// The reported artifact itself: a row that contains the prompt but does not begin with it,
        /// which is a redraw spliced into a row still holding an older one.
        spliced_rows: Vec<String>,
        /// The same three judgements taken when the drag quiesced, before the trailing keystroke
        /// could repaint over the damage — a later redraw can *heal* a corrupted screen.
        live_clean: bool,
        live_spliced_rows: Vec<String>,
        live_rows: Vec<(usize, String)>,
        /// Every size the child was actually told about, in order.
        commits: Vec<(u16, u16)>,
        /// Sizes the child heard while its buffer was non-empty. The fault's precondition, counted.
        commits_while_typing: Vec<(u16, u16)>,
        composed_rows: Vec<(usize, String)>,
    }

    /// One run of the minimal repro through the application's own resize loop.
    ///
    /// Type without submitting; drag narrower than the prompt; drag wider again; press a key that
    /// redraws. `gated` is the app policy bit: `false` is the 2026-08-06 production default,
    /// `true` is the reversible retained mitigation, and nothing else about the run changes.
    fn run_deferred_resize_probe(gated: bool) -> Option<DeferProbeOutcome> {
        let mut oracle = AppResizeOracle::spawn(
            "pwsh.exe",
            &defer_probe_startup(),
            DEFER_PROBE_WIDE,
            DEFER_PROBE_ROWS,
            false,
        );
        oracle.typed_input_gate = gated;
        if !oracle.settle_at_prompt(DEFER_PROBE_PROMPT) {
            eprintln!(
                "BT_CONPTY_DEFER_PROBE gated={gated} SETUP-FAILED line={:?} rows={:?}",
                oracle.current_line(),
                occupied_rows(&oracle.composed_rows())
            );
            return None;
        }

        oracle.pty.write(DEFER_PROBE_TYPED.as_bytes()).unwrap();
        oracle.pump_for(Duration::from_millis(500));
        let armed = oracle.session.typed_shell_input_live();
        let emit_mark = oracle.raw_output.len();

        // Sizes the child hears while it is holding text. Sampling the gate's own question at each
        // commit is what makes "the child is never told about the narrow width while its buffer is
        // non-empty" a measured fact rather than an inference from the picture.
        let mut commits_while_typing = Vec::new();
        let mut seen_commits = 0usize;
        fn record(oracle: &AppResizeOracle, seen: &mut usize, into: &mut Vec<(u16, u16)>) {
            while *seen < oracle.commits.len() {
                into.push(oracle.commits[*seen]);
                *seen += 1;
            }
        }

        // Two gestures with a pause between them, which is what a hand does: drag narrower, let go,
        // drag wider again. Each pause is long enough for the coalescer's quiet window, so the
        // ungated loop commits each phase's final width — including the narrow one — and long
        // enough after that for the line editor's own resize poll to repaint.
        let mut typed_after = String::new();
        for (phase, keystroke) in [
            ([92_u16, 80, 64, 48, 36, DEFER_PROBE_NARROW], 'X'),
            ([36, 48, 64, 80, 92, DEFER_PROBE_WIDE], 'Z'),
        ] {
            for width in phase {
                oracle.project_resize(width, DEFER_PROBE_ROWS);
                // ~60 Hz, the rate winit delivers a live drag at.
                oracle.pump_for(Duration::from_millis(16));
            }
            oracle.pump_for(RESIZE_REQUEST_QUIET + Duration::from_millis(40));
            oracle.pump_until_quiet(Duration::from_secs(6));
            oracle.pump_for(DEFER_PROBE_SETTLE);
            oracle.pump_until_quiet(Duration::from_secs(6));
            if oracle.session.typed_shell_input_live() {
                record(&oracle, &mut seen_commits, &mut commits_while_typing);
            }
            // The keystroke that makes the editor *render* at the width it has just been given.
            // Rendering is where the reduction happens — PSReadLine folds its cached anchor column
            // modulo the buffer width when it finds the column no longer fits — so a narrowing the
            // editor never renders at is a narrowing it never reduces against. The second one is
            // "press any key that redraws" from the repro: the render that reads the anchor the
            // first one broke, now that the pane is wide again.
            oracle.pty.write(keystroke.to_string().as_bytes()).unwrap();
            typed_after.push(keystroke);
            oracle.pump_for(Duration::from_millis(900));
            oracle.pump_until_quiet(Duration::from_secs(6));
            if oracle.session.typed_shell_input_live() {
                record(&oracle, &mut seen_commits, &mut commits_while_typing);
            }
        }

        // What the composed frame says, and what the *live grid alone* says. The composed frame is
        // the picture the window presents; the live grid is the child's own screen with none of our
        // transcript or staging in it, which is where a splice has to be visible for the claim to
        // be about the child rather than about us.
        let live = oracle.session.terminal().visible_text();
        let live_input_line = oracle.composed_input_line(DEFER_PROBE_PROMPT);
        let live_expected = format!("{DEFER_PROBE_PROMPT}{DEFER_PROBE_TYPED}{typed_after}");
        let live_spliced_rows = spliced_prompt_rows(&live, DEFER_PROBE_PROMPT);

        let input_line = oracle.composed_input_line(DEFER_PROBE_PROMPT);
        let expected_input_line = live_expected.clone();
        let composed = oracle.composed_rows();
        let noise_intact = composed
            .iter()
            .any(|row| row.trim_end() == DEFER_PROBE_NOISE)
            || composed
                .windows(2)
                .any(|pair| format!("{}{}", pair[0], pair[1]).contains(DEFER_PROBE_NOISE));
        let spliced_rows = spliced_prompt_rows(&composed, DEFER_PROBE_PROMPT);
        if std::env::var_os("BT_DEFER_EMIT").is_some() {
            eprintln!(
                "BT_CONPTY_DEFER_PROBE_EMIT gated={gated} emitted={:?}",
                escaped(&oracle.raw_output[emit_mark..])
            );
        }
        Some(DeferProbeOutcome {
            gated,
            armed,
            clean: input_line == expected_input_line && noise_intact && spliced_rows.is_empty(),
            input_line,
            expected_input_line,
            noise_intact,
            spliced_rows,
            live_clean: live_input_line == live_expected && live_spliced_rows.is_empty(),
            live_spliced_rows,
            live_rows: occupied_rows(&live),
            commits: oracle.commits.clone(),
            commits_while_typing,
            composed_rows: occupied_rows(&composed),
        })
    }

    /// TWO-POLICY ACCEPTANCE PROBE for typed-input ConPTY resize behavior (user rulings 2026-08-04
    /// and 2026-08-06).
    ///
    /// The mitigation was shelved once because nobody could reproduce the corruption and therefore
    /// nobody could prove a mitigation worked. This is that proof, both directions in one run
    /// against one real pwsh child through the product's own resize loop:
    ///
    /// * `gated=false` — the current default policy. The child hears widths narrower than its
    ///   prompt while its buffer is non-empty, PSReadLine's reduced anchor column survives the
    ///   widening, and the redraw splices into the prompt row.
    /// * `gated=true` — the retained policy. The same drag, the same keystroke, the same child. The
    ///   buffer is never empty across the narrowing, so the resize never leaves us, and there is no
    ///   stale anchor to splice from.
    ///
    /// Measured on a 37-cell prompt at 100 columns narrowed to 24 (`BT_DEFER_EMIT=1` prints the
    /// child's own bytes). Ungated, the render that follows the narrow commit addresses
    /// `CSI 4;14H` — column 13, and `37 mod 24 == 13`, the upstream rule stated as a number — and
    /// the render after the pane is 100 columns wide again *still* addresses column 13
    /// (`CSI 3;14H`), which is the splice. Gated, both renders address `CSI 2;38H`: column 37, the
    /// prompt's true width, because the child was never told about the 24.
    ///
    /// Read the `BT_CONPTY_DEFER_PROBE` lines on stderr:
    ///   `cargo test -p bt-pty typed_input_resize_deferral -- --ignored --nocapture`
    #[test]
    #[ignore = "dev probe: drives a real interactive PowerShell through ConPTY; host-timing sensitive"]
    fn typed_input_resize_deferral_probe() {
        let source = conpty_source();
        let mut outcomes = Vec::new();
        for gated in [false, true] {
            let Some(outcome) = run_deferred_resize_probe(gated) else {
                continue;
            };
            eprintln!(
                "BT_CONPTY_DEFER_PROBE source={source} gated={} armed={} live_clean={} \
                 clean={} commits={:?} commits_while_typing={:?} noise_intact={} \
                 spliced={:?} live_spliced={:?} input={:?} expected={:?}",
                outcome.gated,
                outcome.armed,
                outcome.live_clean,
                outcome.clean,
                outcome.commits,
                outcome.commits_while_typing,
                outcome.noise_intact,
                outcome.spliced_rows,
                outcome.live_spliced_rows,
                outcome.input_line,
                outcome.expected_input_line,
            );
            if !outcome.clean || !outcome.live_clean {
                eprintln!(
                    "BT_CONPTY_DEFER_PROBE_ROWS gated={} live={:?} composed={:?}",
                    outcome.gated, outcome.live_rows, outcome.composed_rows
                );
            }
            outcomes.push(outcome);
        }
        assert_eq!(outcomes.len(), 2, "the probe drove no live shell");
        let before = &outcomes[0];
        let after = &outcomes[1];
        assert!(
            after.armed,
            "the mitigation cannot be judged unless the shell integration armed the gate"
        );
        assert!(
            after.commits_while_typing.is_empty(),
            "the child must never be told a new size while its buffer is non-empty, got {:?}",
            after.commits_while_typing
        );
        assert!(
            !before.commits_while_typing.is_empty(),
            "the ungated run must reach the fault's own precondition, or it proves nothing"
        );
        assert!(
            !before.clean || !before.live_clean,
            "the ungated run did not reproduce the splice; the pair proves nothing until it does. \
             live={:?} input={:?} expected={:?} rows={:?}",
            before.live_rows,
            before.input_line,
            before.expected_input_line,
            before.composed_rows
        );
        assert!(
            after.live_clean && after.clean,
            "the gated run must present the prompt, the typed text and the keystroke and nothing \
             else. live={:?} input={:?} expected={:?} rows={:?}",
            after.live_rows,
            after.input_line,
            after.expected_input_line,
            after.composed_rows
        );
    }

    /// MACHINERY ACCEPTANCE PROBE for confirm-then-release, against a live child.
    ///
    /// This pins the retained gate machine, not the default policy. Both arms keep the typed-input
    /// gate enabled and vary only single-sample release versus confirmed release.
    ///
    /// The gate reads the grid, and the grid is written by whatever a single `read` returned — the
    /// reader thread hands the loop up to 16 KiB and wakes it per chunk. PSReadLine redraws a line
    /// by parking the cursor on `B`, erasing, and writing the buffer back, so a redraw cut between
    /// two reads leaves a wake where the grid honestly holds nothing while the *buffer* holds a
    /// command the user is still typing. Chunk boundaries are the operating system's to choose, so
    /// this probe does not try to place one: it types a command, never submits it, and hammers the
    /// keystrokes that force redraw after redraw while a narrowing resize sits queued. Every sample
    /// taken in that span is a sample whose ground truth is "non-empty" by construction.
    ///
    /// * `BT_CONPTY_BLANK_WINDOW_PROBE blank_samples=` counts the wakes where the gate read empty
    ///   anyway. Each one is a release the pre-fix loop would have taken.
    /// * `commits` must stay empty across the whole storm — the confirmation window is what makes a
    ///   count above zero harmless — and then carry exactly the dragged size once the command is
    ///   submitted and the line stays empty for `RESIZE_REQUEST_QUIET`.
    ///
    ///   `cargo test -p bt-pty typed_input_gate_blank_window -- --ignored --nocapture`
    #[derive(Debug)]
    struct BlankWindowOutcome {
        confirmed: bool,
        samples: usize,
        blank_samples: usize,
        /// Sizes the child was told about while the buffer provably held 1200 characters.
        commits_while_typing: Vec<(u16, u16)>,
        commits: Vec<(u16, u16)>,
    }

    /// One run of the storm. `confirmed` selects confirm-then-release; nothing else changes.
    fn run_blank_window_probe(confirmed: bool) -> Option<BlankWindowOutcome> {
        let mut oracle = AppResizeOracle::spawn(
            "pwsh.exe",
            &defer_probe_startup(),
            DEFER_PROBE_WIDE,
            DEFER_PROBE_ROWS,
            false,
        );
        oracle.typed_input_gate = true;
        oracle.confirm_blank_gate = confirmed;
        if !oracle.settle_at_prompt(DEFER_PROBE_PROMPT) {
            eprintln!(
                "BT_CONPTY_BLANK_WINDOW_PROBE confirmed={confirmed} SETUP-FAILED line={:?}",
                oracle.current_line()
            );
            return None;
        }

        // Narrow the pane first, at an *empty* prompt where the gate has nothing to protect. This
        // is the reporting user's geometry: a pane already narrower than its prompt, so the prompt
        // wraps and every redraw of the input line repaints several rows — which is the payload a
        // 16 KiB read can cut in half.
        oracle.project_resize(DEFER_PROBE_NARROW, DEFER_PROBE_ROWS);
        oracle.pump_for(RESIZE_REQUEST_QUIET + Duration::from_millis(40));
        oracle.pump_until_quiet(Duration::from_secs(6));
        assert_eq!(
            oracle.commits,
            vec![(DEFER_PROBE_NARROW, DEFER_PROBE_ROWS)],
            "an idle prompt must still resize, or the probe never reaches the narrow geometry \
             (confirmed={confirmed})"
        );

        // A command long enough to outgrow the pane: at 24 columns this wraps past the seventeen
        // rows the child has, so every redraw repaints the whole visible window *and* the region's
        // own start scrolls off the top of it. Both halves of the fault live here — the payload big
        // enough for a read to cut, and the start anchor the capture transaction re-seats.
        oracle.pty.write(DEFER_PROBE_TYPED.as_bytes()).unwrap();
        oracle.pty.write("a".repeat(1_200).as_bytes()).unwrap();
        oracle.pump_for(Duration::from_millis(1_500));
        oracle.pump_until_quiet(Duration::from_secs(6));
        assert!(
            oracle.session.typed_shell_input_live(),
            "the shell integration must arm the gate, or the probe measures nothing \
             (confirmed={confirmed})"
        );

        // The drag, queued while the buffer is full.
        oracle.project_resize(DEFER_PROBE_WIDE, DEFER_PROBE_ROWS);
        oracle.pump_for(RESIZE_REQUEST_QUIET + Duration::from_millis(40));

        // The storm. Each keystroke is one erase-and-rewrite of the whole line, and the loop is
        // pumped at its own rate throughout — which is exactly where a split lands.
        let mut samples = 0usize;
        let mut blank_samples = 0usize;
        for index in 0..40 {
            // A character and its backspace: the buffer ends where it started, and every one of the
            // forty keystrokes between is a full redraw of a line taller than the pane.
            let keystroke: &[u8] = if index % 2 == 0 { b"1" } else { b"\x7f" };
            oracle.pty.write(keystroke).unwrap();
            let deadline = Instant::now() + Duration::from_millis(50);
            while Instant::now() < deadline {
                oracle.pump_once();
                samples += 1;
                if !oracle.session.typed_shell_input_live() {
                    blank_samples += 1;
                }
            }
        }
        let commits_while_typing = oracle.commits[1..].to_vec();

        // RELEASE: the command is submitted and the line stays empty through the quiet window.
        oracle.pty.write(b"\r").unwrap();
        oracle.pump_for(RESIZE_REQUEST_QUIET + Duration::from_millis(200));
        oracle.pump_until_quiet(Duration::from_secs(6));

        Some(BlankWindowOutcome {
            confirmed,
            samples,
            blank_samples,
            commits_while_typing,
            commits: oracle.commits.clone(),
        })
    }

    #[test]
    #[ignore = "dev probe: drives a real interactive PowerShell through ConPTY; host-timing sensitive"]
    fn typed_input_gate_blank_window_probe() {
        let source = conpty_source();
        let mut outcomes = Vec::new();
        for confirmed in [false, true] {
            let Some(outcome) = run_blank_window_probe(confirmed) else {
                continue;
            };
            eprintln!(
                "BT_CONPTY_BLANK_WINDOW_PROBE source={source} confirmed={} samples={} \
                 blank_samples={} commits_while_typing={:?} commits={:?}",
                outcome.confirmed,
                outcome.samples,
                outcome.blank_samples,
                outcome.commits_while_typing,
                outcome.commits
            );
            outcomes.push(outcome);
        }
        assert_eq!(outcomes.len(), 2, "the probe drove no live shell");
        let before = &outcomes[0];
        let after = &outcomes[1];
        // RED-CHECK: releasing on one sample really does hand the child a size mid-buffer. The
        // gate has no bypass in it and the buffer is never empty in this span — the release comes
        // from the blank instant of a redraw, which is the whole fault.
        assert!(
            !before.commits_while_typing.is_empty(),
            "releasing on a single blank sample must reach the fault's precondition, or the pair \
             proves nothing: blank_samples={} of {}",
            before.blank_samples,
            before.samples
        );
        assert!(
            after.commits_while_typing.is_empty(),
            "no size may reach the child across the redraw storm, got {:?} (blank_samples={} of \
             {})",
            after.commits_while_typing,
            after.blank_samples,
            after.samples
        );
        assert_eq!(
            after.commits,
            vec![
                (DEFER_PROBE_NARROW, DEFER_PROBE_ROWS),
                (DEFER_PROBE_WIDE, DEFER_PROBE_ROWS)
            ],
            "the queued size lands exactly once, after the submitted line stays empty"
        );
    }

    fn invoke_prompt_probe_startup(prompt: &str) -> String {
        format!(
            "Import-Module PSReadLine; \
             [Console]::Write(([string][char]27) + ']777;BT_PSRL_VERSION=' + \
             (Get-Module PSReadLine).Version + [char]7); \
             Set-PSReadLineOption -HistorySaveStyle SaveNothing; \
             function global:prompt {{ '{prompt}' }}; . '{}'",
            integration_script_path().display()
        )
    }

    /// Dev probe: pins the VT-input spelling that ConPTY translates into the physical key used by
    /// the shell integration's private InvokePrompt binding. The sentinel comes from the handler,
    /// not from the startup script, so observing it proves the complete VT -> ConPTY -> PSReadLine
    /// chord path.
    #[test]
    #[ignore = "dev probe: drives real PSReadLine handlers through ConPTY"]
    fn invoke_prompt_key_translation_probe() {
        let startup = "Import-Module PSReadLine; \
            [Console]::Write(([string][char]27) + ']777;BT_PSRL_VERSION=' + \
            (Get-Module PSReadLine).Version + [char]7); \
            Set-PSReadLineOption -HistorySaveStyle SaveNothing; \
            Set-PSReadLineKeyHandler -Chord F24 -ScriptBlock { \
            [Console]::Write(([string][char]27) + ']777;BT_KEY_F24' + [char]7) }; \
            Set-PSReadLineKeyHandler -Chord Ctrl+Alt+F12 -ScriptBlock { \
            [Console]::Write(([string][char]27) + ']777;BT_KEY_CAF12' + [char]7) }; \
            Set-PSReadLineKeyHandler -Chord Ctrl+Alt+Shift+F12 -ScriptBlock { \
            [Console]::Write(([string][char]27) + ']777;BT_KEY_CASF12' + [char]7) }; \
            function global:prompt { 'BTKEY> ' }";
        let candidates: [(&str, &[u8]); 5] = [
            ("xterm-f24", b"\x1b[45~"),
            ("shift-f12", b"\x1b[24;2~"),
            ("ctrl-alt-f12", b"\x1b[24;7~"),
            ("ctrl-alt-shift-f12", b"\x1b[24;8~"),
            ("ctrl-alt-shift-f4", b"\x1b[1;8S"),
        ];

        for shell in ["pwsh.exe", "powershell.exe"] {
            let mut oracle = InteractiveOracle::spawn_shell_with(shell, startup, 80, 10);
            oracle.wait_for_output_since(0, b"BT_PSRL_VERSION=");
            oracle.pump_until_quiet(Duration::from_secs(6));
            let version = psreadline_version(&oracle.raw_output);
            for (label, input) in candidates {
                let mark = oracle.raw_output.len();
                oracle.session.write(input).unwrap();
                oracle.pump_for(Duration::from_millis(400));
                let output = &oracle.raw_output[mark..];
                eprintln!(
                    "BT_CONPTY_INVOKE_KEY_PROBE source={} shell={shell} psreadline={version} \
                     candidate={label} \
                     input={} output={}",
                    conpty_source(),
                    escaped(input),
                    escaped(output)
                );
                if label == "ctrl-alt-shift-f12" {
                    assert!(
                        output
                            .windows(b"BT_KEY_CASF12".len())
                            .any(|window| { window == b"BT_KEY_CASF12" }),
                        "the shipped VT sequence must reach the private chord on {shell} \
                         PSReadLine {version}"
                    );
                }
                if label == "xterm-f24" {
                    assert!(
                        !output
                            .windows(b"BT_KEY_F24".len())
                            .any(|window| window == b"BT_KEY_F24"),
                        "F24 unexpectedly became usable; reconsider the private chord"
                    );
                }
            }
        }
    }

    #[test]
    #[ignore = "dev probe: compares an empty-prompt resize with and without InvokePrompt"]
    fn empty_prompt_resize_invoke_prompt_pair_probe() {
        const PROMPT: &str = "BTINVOKE 012345678901234567890123456789012> ";
        const HISTORY: &str = "Write-Output BT_INVOKE_HISTORY";
        const HISTORY_OUTPUT: &str = "BT_INVOKE_HISTORY";
        let startup = invoke_prompt_probe_startup(PROMPT);
        let wide_tail = PROMPT[38..].trim_end();
        let expected = format!("{PROMPT}{HISTORY}");
        for shell in ["pwsh.exe", "powershell.exe"] {
            let mut outcomes = Vec::new();
            for invoke in [false, true] {
                let mut oracle = AppResizeOracle::spawn(shell, &startup, 38, 14, false);
                // The product-timing arm is the version that actually repaints. For 2.0.0, land
                // the resize first and bracket the no-op injection at one stable geometry so the
                // before/after row comparison measures the handler and nothing else.
                oracle.invoke_prompt_after_resize = invoke && shell == "pwsh.exe";
                assert!(
                    oracle
                        .settle_at_prompt_matching(&|line| line == wide_tail)
                        .is_some()
                );
                oracle.pty.write(HISTORY.as_bytes()).unwrap();
                oracle.pty.write(b"\r").unwrap();
                oracle.pump_for(Duration::from_millis(700));
                assert!(
                    oracle
                        .settle_at_prompt_matching(&|line| line == wide_tail)
                        .is_some()
                );
                let mark = oracle.raw_output.len();
                oracle.project_resize(29, 14);
                oracle.pump_for(RESIZE_REQUEST_QUIET + Duration::from_millis(100));
                oracle.pump_for(Duration::from_millis(1_500));
                oracle.pump_until_quiet(Duration::from_secs(6));

                let mut no_op_output = Vec::new();
                if shell == "powershell.exe" && invoke {
                    let before_injection = oracle.session.terminal().visible_text();
                    let injection_mark = oracle.raw_output.len();
                    oracle.write_invoke_prompt();
                    oracle.pump_for(Duration::from_millis(500));
                    oracle.pump_until_quiet(Duration::from_secs(6));
                    no_op_output.extend_from_slice(&oracle.raw_output[injection_mark..]);
                    let after_injection = oracle.session.terminal().visible_text();
                    assert_eq!(
                        after_injection, before_injection,
                        "PSReadLine 2.0.0 no-op must preserve every visible row"
                    );
                    assert!(
                        no_op_output.is_empty(),
                        "the production no-op emits no repaint or literal input: {}",
                        escaped(&no_op_output)
                    );
                    assert!(
                        !after_injection.concat().contains("24;8~"),
                        "the consumed VT chord must not leak into the PSReadLine buffer"
                    );
                }

                let recall_mark = oracle.raw_output.len();
                oracle.pty.write(b"\x1b[A").unwrap();
                oracle.pump_for(Duration::from_millis(900));
                oracle.pump_until_quiet(Duration::from_secs(6));
                let resize_output = &oracle.raw_output[mark..recall_mark];
                let recall_output = &oracle.raw_output[recall_mark..];
                let recall_addresses = redraw_addresses(recall_output);
                let rows = oracle.session.terminal().visible_text();
                let cursor = oracle.session.terminal().cursor();
                let input = wrapped_input_line(&rows, cursor.row, PROMPT, 29);
                let version = psreadline_version(&oracle.raw_output);
                let history_output_survived =
                    rows.iter().any(|row| row.trim_end() == HISTORY_OUTPUT);
                eprintln!(
                    "BT_CONPTY_EMPTY_INVOKE_PAIR source={} shell={shell} psreadline={version} \
                     invoke={invoke} writes={} resize_addresses={:?} recall_addresses={:?} \
                     history_output_survived={history_output_survived} input={input:?} \
                     expected={expected:?} rows={:?} no_op_output={} resize_output={} \
                     recall_output={}",
                    conpty_source(),
                    oracle.invoke_prompt_writes,
                    cursor_addresses(resize_output),
                    recall_addresses,
                    occupied_rows(&rows),
                    escaped(&no_op_output),
                    escaped(resize_output),
                    escaped(recall_output)
                );
                outcomes.push((
                    invoke,
                    oracle.invoke_prompt_writes,
                    recall_addresses,
                    input,
                    history_output_survived,
                ));
            }

            let red = &outcomes[0];
            let green = &outcomes[1];
            assert!(!red.0 && green.0);
            assert_eq!(red.1, 0, "the red arm injects no repaint chord");
            assert_eq!(green.1, 1, "the green arm injects exactly one chord");
            if shell == "pwsh.exe" {
                assert_eq!(
                    red.2.first().map(|address| address.1),
                    Some(7),
                    "red check: history recall must start at the old prompt tail"
                );
                assert_ne!(
                    red.3, expected,
                    "red check: the stale anchor must overwrite the resized prompt tail"
                );
                assert_eq!(
                    green.2.first().map(|address| address.1),
                    Some(16),
                    "InvokePrompt must move history recall to the new prompt tail"
                );
                assert_eq!(
                    green.3, expected,
                    "the pwsh green arm must leave one clean input line"
                );
                assert!(
                    green.4,
                    "InvokePrompt must preserve the command output printed before injection"
                );
            } else {
                assert_eq!(
                    green.3, red.3,
                    "the 2.0.0 no-op must not change the subsequent redraw outcome"
                );
                assert_eq!(green.4, red.4);
            }
        }
    }

    /// Dev probe for the real divider gesture: every stop is long enough to commit, so the
    /// red arm restores the old unconditional InvokePrompt handler; the green arm drives the
    /// shipped empty-buffer re-anchor-only handler. The prompt deliberately wraps at both narrow
    /// widths; any abandoned repaint is therefore visible both as another `BTCHAIN` opening and as
    /// text outside the one expected logical prompt.
    #[test]
    #[ignore = "dev probe: drives a chain of committed resizes through real PSReadLine"]
    fn empty_prompt_committed_resize_chain_reanchor_pair_probe() {
        const PROMPT: &str = "BTCHAIN 012345678901234567890123456789012> ";
        const WIDTHS: [u16; 4] = [100, 29, 100, 29];
        let startup = invoke_prompt_probe_startup(PROMPT);
        let initial_tail = PROMPT[38..].trim_end();
        let mut outcomes = Vec::new();

        for reanchor_only in [false, true] {
            let arm_startup = if reanchor_only {
                startup.clone()
            } else {
                format!(
                    "{startup}; Set-PSReadLineKeyHandler -Chord Ctrl+Alt+Shift+F12 \
                     -ScriptBlock {{ param($key, $arg) \
                     [Microsoft.PowerShell.PSConsoleReadLine]::InvokePrompt($key, $arg) }}"
                )
            };
            let mut oracle = AppResizeOracle::spawn("pwsh.exe", &arm_startup, 38, 24, false);
            oracle.invoke_prompt_after_resize = true;
            assert!(
                oracle
                    .settle_at_prompt_matching(&|line| line == initial_tail)
                    .is_some()
            );
            let initial_rows = oracle.session.terminal().visible_text();
            let initial_prompt_lines = initial_rows.concat().match_indices("BTCHAIN ").count();
            assert_eq!(initial_prompt_lines, 1);

            for columns in WIDTHS {
                let mark = oracle.raw_output.len();
                oracle.project_resize(columns, 24);
                oracle.pump_for(RESIZE_REQUEST_QUIET * 2 + Duration::from_millis(100));
                oracle.pump_until_quiet(Duration::from_secs(6));
                let rows = oracle.session.terminal().visible_text();
                let prompt_lines = rows.concat().match_indices("BTCHAIN ").count();
                let visible = occupied_rows(&rows);
                let visible_text = visible
                    .iter()
                    .map(|(_, row)| row.as_str())
                    .collect::<String>();
                eprintln!(
                    "BT_CONPTY_EMPTY_CHAIN_STEP source={} psreadline={} \
                     reanchor_only={reanchor_only} \
                     columns={columns} writes={} prompt_lines={prompt_lines} rows={visible:?} \
                     output={}",
                    conpty_source(),
                    psreadline_version(&oracle.raw_output),
                    oracle.invoke_prompt_writes,
                    escaped(&oracle.raw_output[mark..])
                );
                if reanchor_only {
                    assert_eq!(
                        prompt_lines, 1,
                        "every green-arm commit must retain one prompt: {visible:?}"
                    );
                    assert_eq!(
                        visible_text,
                        PROMPT.trim_end(),
                        "every green-arm commit must leave no wrapped fragment: {visible:?}"
                    );
                }
            }

            let rows = oracle.session.terminal().visible_text();
            let visible = occupied_rows(&rows);
            let prompt_lines = rows.concat().match_indices("BTCHAIN ").count();
            let visible_text = visible
                .iter()
                .map(|(_, row)| row.as_str())
                .collect::<String>();
            let clean = prompt_lines == 1 && visible_text == PROMPT.trim_end();
            eprintln!(
                "BT_CONPTY_EMPTY_CHAIN_SUMMARY source={} psreadline={} \
                 reanchor_only={reanchor_only} \
                 writes={} prompt_lines={prompt_lines} clean={clean} rows={visible:?}",
                conpty_source(),
                psreadline_version(&oracle.raw_output),
                oracle.invoke_prompt_writes
            );
            outcomes.push((reanchor_only, prompt_lines, clean, visible));
        }

        let red = &outcomes[0];
        let green = &outcomes[1];
        assert!(!red.0 && green.0);
        assert!(
            red.1 > 1,
            "red arm must reproduce the reported prompt-line growth: {:?}",
            red.3
        );
        assert!(!red.2, "red arm must retain old wrapped prompt fragments");
        assert_eq!(
            green.1, 1,
            "re-anchor arm must retain exactly one visible prompt: {:?}",
            green.3
        );
        assert!(
            green.2,
            "re-anchor arm must leave no old wrapped prompt fragments: {:?}",
            green.3
        );
    }

    #[test]
    #[ignore = "dev probe: proves sessions without OSC 133 receive no private repaint input"]
    fn invoke_prompt_requires_open_osc133_input_region_probe() {
        let startup = "Set-PSReadLineOption -HistorySaveStyle SaveNothing; \
            function global:prompt { 'BTNOINTEGRATION> ' }";
        let mut oracle = AppResizeOracle::spawn("pwsh.exe", startup, 50, 10, false);
        oracle.invoke_prompt_after_resize = true;
        assert!(oracle.settle_at_prompt("BTNOINTEGRATION> "));
        oracle.project_resize(40, 10);
        oracle.pump_for(RESIZE_REQUEST_QUIET + Duration::from_millis(300));
        oracle.pump_until_quiet(Duration::from_secs(6));
        eprintln!(
            "BT_CONPTY_INVOKE_SCOPE source={} shell=pwsh.exe osc133_open={} writes={} commits={:?}",
            conpty_source(),
            oracle.session.shell_input_region_open(),
            oracle.invoke_prompt_writes,
            oracle.commits
        );
        assert_eq!(oracle.commits, vec![(40, 10)]);
        assert_eq!(oracle.invoke_prompt_writes, 0);
    }

    #[test]
    #[ignore = "dev probe: proves the shipped PSReadLine 2.0.0 handler consumes the private chord"]
    fn psreadline_2_no_op_handler_consumes_invoke_prompt_chord_probe() {
        const SENTINEL: &[u8] = b"BT_PSREADLINE_NOOP";
        let startup = format!(
            "$env:BT_PSREADLINE_NOOP_PROBE = '1'; {}",
            invoke_prompt_probe_startup("BTNOOP> ")
        );
        let mut oracle = InteractiveOracle::spawn_shell_with("powershell.exe", &startup, 80, 10);
        oracle.wait_for_output_since(0, b"BT_PSRL_VERSION=2.0.0");
        oracle.wait_for_current_line("BTNOOP>");
        oracle.pump_until_quiet(Duration::from_secs(6));
        let before = oracle.terminal.visible_text();
        let mark = oracle.raw_output.len();
        oracle
            .session
            .write(PSREADLINE_INVOKE_PROMPT_INPUT)
            .unwrap();
        oracle.wait_for_output_since(mark, SENTINEL);
        oracle.pump_until_quiet(Duration::from_secs(6));
        let output = &oracle.raw_output[mark..];
        let after = oracle.terminal.visible_text();
        eprintln!(
            "BT_CONPTY_PSREADLINE2_NOOP source={} shell=powershell.exe psreadline=2.0.0 \
             input={} sentinel_seen={} rows_unchanged={} output={}",
            conpty_source(),
            escaped(PSREADLINE_INVOKE_PROMPT_INPUT),
            output
                .windows(SENTINEL.len())
                .any(|window| window == SENTINEL),
            before == after,
            escaped(output)
        );
        assert!(
            output
                .windows(SENTINEL.len())
                .any(|window| window == SENTINEL),
            "the environment-gated sentinel proves the shipped no-op handler ran"
        );
        assert_eq!(after, before, "the no-op must preserve every visible row");
        assert!(
            !output
                .windows(b"\x1b[2J".len())
                .any(|window| window == b"\x1b[2J"),
            "the 2.0.0 branch must never call InvokePrompt's ED 2 path"
        );
        assert!(
            !output
                .windows(b"24;8~".len())
                .any(|window| window == b"24;8~"),
            "the chord was consumed, not inserted as literal input"
        );
    }

    /// RED/GREEN HISTORY-TRANSPARENCY PROBE for the private resize-anchor chord. PSReadLine's
    /// `InputLoop` treats a private script handler like every other editing command: unless its
    /// history counters advance, post-dispatch cleanup clears the active walk and puts
    /// `_currentHistoryIndex` back at the newest end. This drives that exact sequence through real
    /// ConPTY on both private shapes installed on the development machine:
    ///
    /// * non-empty: Up recalls `echo`, the chord lands, and the next Up must continue to the older
    ///   `Write-Output` entry (the red arm incorrectly recalls `echo` again);
    /// * empty: the chord lands at an ordinary empty prompt, where it must not manufacture a
    ///   history session, and a one-step Up must still recall the newest entry.
    ///
    /// Read the `BT_CONPTY_HISTORY_TRANSPARENCY` lines on stderr:
    ///   `cargo test -p bt-pty psreadline_resize_chord_history_transparency -- --ignored --nocapture`
    #[test]
    #[ignore = "dev probe: drives real PSReadLine 2.4.5 and 2.0.0 history through ConPTY"]
    fn psreadline_resize_chord_history_transparency_pair_probe() {
        const PROMPT: &str = "BTHIST> ";
        const OLDER: &str = "Write-Output BT_HISTORY_OLDER";
        const NEWEST: &str = "echo BT_HISTORY_NEWEST";
        const SEEDED: &[u8] = b"BT_HISTORY_TRANSPARENCY_SEEDED";
        const HISTORY_MARKER: &[u8] = b"BT_PSREADLINE_HISTORY=";
        let mut outcomes = Vec::new();

        for shell in ["pwsh.exe", "powershell.exe"] {
            let fallback_arms: &[bool] = if shell == "pwsh.exe" {
                &[false, true]
            } else {
                &[false]
            };
            for &force_fallback in fallback_arms {
                for buffer_nonempty in [true, false] {
                    for red in [true, false] {
                        let arm = if red {
                            "$env:BT_PSREADLINE_HISTORY_TRANSPARENCY_RED_PROBE = '1'; "
                        } else {
                            ""
                        };
                        let fallback = if force_fallback {
                            "$env:BT_PSREADLINE_REANCHOR_PROBE = '1'; \
                         $env:BT_PSREADLINE_REANCHOR_FORCE_FALLBACK_PROBE = '1'; "
                        } else {
                            ""
                        };
                        let startup = format!(
                            "$env:BT_PSREADLINE_HISTORY_PROBE = '1'; {arm}{fallback}{}; \
                         Set-PSReadLineKeyHandler -Chord Ctrl+g -ScriptBlock {{ \
                         [Microsoft.PowerShell.PSConsoleReadLine]::ClearHistory(); \
                         [Microsoft.PowerShell.PSConsoleReadLine]::AddToHistory('{OLDER}'); \
                         [Microsoft.PowerShell.PSConsoleReadLine]::AddToHistory('{NEWEST}'); \
                         [Console]::Write(([string][char]27) + \
                         ']777;BT_HISTORY_TRANSPARENCY_SEEDED' + [char]7) }}",
                            invoke_prompt_probe_startup(PROMPT)
                        );
                        let mut oracle =
                            InteractiveOracle::spawn_shell_with(shell, &startup, 100, 12);
                        oracle.wait_for_output_since(0, b"BT_PSRL_VERSION=");
                        oracle.wait_for_current_line(PROMPT.trim_end());

                        let seed_mark = oracle.raw_output.len();
                        oracle.session.write(b"\x07").unwrap();
                        oracle.wait_for_output_since(seed_mark, SEEDED);
                        oracle.pump_until_quiet(Duration::from_secs(6));

                        if buffer_nonempty {
                            oracle.session.write(b"\x1b[A").unwrap();
                            oracle.wait_for_current_line(&format!("{PROMPT}{NEWEST}"));
                            oracle.pump_until_quiet(Duration::from_secs(6));
                        }

                        let chord_mark = oracle.raw_output.len();
                        oracle
                            .session
                            .write(PSREADLINE_INVOKE_PROMPT_INPUT)
                            .unwrap();
                        oracle.wait_for_output_since(chord_mark, HISTORY_MARKER);
                        oracle.pump_until_quiet(Duration::from_secs(6));

                        oracle.session.write(b"\x1b[A").unwrap();
                        oracle.pump_for(Duration::from_millis(500));
                        oracle.pump_until_quiet(Duration::from_secs(6));
                        let actual = oracle.current_line();
                        let expected = if buffer_nonempty && !red {
                            format!("{PROMPT}{OLDER}")
                        } else {
                            format!("{PROMPT}{NEWEST}")
                        };
                        let version = psreadline_version(&oracle.raw_output);
                        let chord_output = escaped(&oracle.raw_output[chord_mark..]);
                        eprintln!(
                            "BT_CONPTY_HISTORY_TRANSPARENCY source={} shell={shell} \
                         psreadline={version} red={red} force_fallback={force_fallback} \
                         buffer_nonempty={buffer_nonempty} \
                         actual={actual:?} expected={expected:?} chord_output={chord_output}",
                            conpty_source()
                        );
                        assert_eq!(actual, expected);
                        if force_fallback {
                            assert!(
                                chord_output.contains("BT_PSREADLINE_REANCHOR_FALLBACK="),
                                "the forced fallback arm did not enter InvokePrompt: {chord_output}"
                            );
                        }
                        outcomes.push((
                            shell,
                            force_fallback,
                            buffer_nonempty,
                            red,
                            version,
                            actual,
                        ));
                    }
                }
            }
        }

        assert_eq!(
            outcomes.len(),
            12,
            "every repair/fallback/no-op, buffer, and red-green arm must run"
        );
    }

    /// The user's exact non-empty gesture. The red arm reinstalls the retired unconditional
    /// InvokePrompt branch; the green arm uses the shipped zero-repaint anchor repair. Every stop
    /// is allowed through the app's real quiet gate, including a command that was already longer
    /// than the initial pane. A final edit proves the repaired render baseline agrees with B.
    #[test]
    #[ignore = "dev probe: live-resizes non-empty PSReadLine buffers through real ConPTY"]
    fn nonempty_buffer_committed_resize_chain_reanchor_pair_probe() {
        const PROMPT: &str = "BTINVOKE 012345678901234567890123456789012> ";
        const TYPED: &str =
            "Write-Output 012345678901234567890123456789012345678901234567890123456789";
        const WIDTHS: [u16; 3] = [24, 100, 24];
        let startup = invoke_prompt_probe_startup(PROMPT);
        let expected = format!("{PROMPT}{TYPED}");
        let mut outcomes = Vec::new();

        for reanchor_only in [false, true] {
            let arm_startup = if reanchor_only {
                startup.clone()
            } else {
                format!(
                    "{startup}; Set-PSReadLineKeyHandler -Chord Ctrl+Alt+Shift+F12 \
                     -ScriptBlock {{ param($key, $arg) \
                     [Microsoft.PowerShell.PSConsoleReadLine]::InvokePrompt($key, $arg) }}"
                )
            };
            let mut oracle = AppResizeOracle::spawn("pwsh.exe", &arm_startup, 38, 24, false);
            oracle.invoke_prompt_after_resize = true;
            assert!(
                oracle
                    .settle_at_prompt_matching(&|line| line == PROMPT[38..].trim_end())
                    .is_some()
            );
            oracle.pty.write(TYPED.as_bytes()).unwrap();
            oracle.pump_for(Duration::from_millis(900));
            oracle.pump_until_quiet(Duration::from_secs(6));
            assert!(oracle.session.typed_shell_input_live());
            let version = psreadline_version(&oracle.raw_output);
            assert_eq!(version, "2.4.5");
            let mut maximum_prompt_copies = 1;
            let mut clean_throughout = true;

            for columns in WIDTHS {
                let mark = oracle.raw_output.len();
                let prior_writes = oracle.invoke_prompt_writes;
                oracle.project_resize(columns, 24);
                oracle.pump_for(RESIZE_REQUEST_QUIET * 2 + Duration::from_millis(100));
                oracle.pump_for(Duration::from_millis(1_500));
                oracle.pump_until_quiet(Duration::from_secs(6));
                let emitted = &oracle.raw_output[mark..];
                let rows = oracle.session.terminal().visible_text();
                let cursor = oracle.session.terminal().cursor();
                let input = wrapped_input_line(&rows, cursor.row, PROMPT, columns);
                let prompt_copies = rows.concat().match_indices("BTINVOKE ").count();
                let clean = input == expected && prompt_copies == 1;
                maximum_prompt_copies = maximum_prompt_copies.max(prompt_copies);
                clean_throughout &= clean;
                eprintln!(
                    "BT_CONPTY_NONEMPTY_CHAIN_STEP source={} psreadline={version} \
                     reanchor_only={reanchor_only} columns={columns} writes={} \
                     prompt_copies={prompt_copies} clean={clean} input={input:?} \
                     expected={expected:?} rows={:?} output={}",
                    conpty_source(),
                    oracle.invoke_prompt_writes,
                    occupied_rows(&rows),
                    escaped(emitted)
                );
                assert_eq!(oracle.invoke_prompt_writes, prior_writes + 1);
                if reanchor_only {
                    assert_eq!(prompt_copies, 1, "zero-output repair duplicated the prompt");
                    assert_eq!(
                        input, expected,
                        "zero-output repair moved or copied the input"
                    );
                    assert!(
                        !emitted
                            .windows(b"\x1b]133;A\x07".len())
                            .any(|window| window == b"\x1b]133;A\x07"),
                        "the green handler must not invoke the prompt function"
                    );
                    assert!(
                        !emitted
                            .windows(b"BT_PSREADLINE_REANCHOR_FALLBACK".len())
                            .any(|window| window == b"BT_PSREADLINE_REANCHOR_FALLBACK"),
                        "the green handler unexpectedly used its reflection fallback"
                    );
                }
            }

            let edit_mark = oracle.raw_output.len();
            oracle.pty.write(b"Z").unwrap();
            oracle.pump_for(Duration::from_millis(900));
            oracle.pump_until_quiet(Duration::from_secs(6));
            let rows = oracle.session.terminal().visible_text();
            let cursor = oracle.session.terminal().cursor();
            let input = wrapped_input_line(&rows, cursor.row, PROMPT, 24);
            let expected_after_edit = format!("{expected}Z");
            let prompt_copies = rows.concat().match_indices("BTINVOKE ").count();
            let edit_clean = input == expected_after_edit && prompt_copies == 1;
            clean_throughout &= edit_clean;
            maximum_prompt_copies = maximum_prompt_copies.max(prompt_copies);
            eprintln!(
                "BT_CONPTY_NONEMPTY_CHAIN_SUMMARY source={} psreadline={version} \
                 reanchor_only={reanchor_only} writes={} maximum_prompt_copies={} \
                 clean_throughout={clean_throughout} edit_clean={edit_clean} input={input:?} \
                 expected={expected_after_edit:?} rows={:?} edit_output={}",
                conpty_source(),
                oracle.invoke_prompt_writes,
                maximum_prompt_copies,
                occupied_rows(&rows),
                escaped(&oracle.raw_output[edit_mark..])
            );
            if reanchor_only {
                assert!(
                    edit_clean,
                    "the next character did not land at the repaired cursor"
                );
            }
            outcomes.push((
                reanchor_only,
                maximum_prompt_copies,
                clean_throughout,
                input,
            ));
        }

        let red = &outcomes[0];
        let green = &outcomes[1];
        assert!(!red.0 && green.0);
        assert!(
            red.1 > 1,
            "red InvokePrompt arm must reproduce prompt growth, got {:?}",
            red
        );
        assert!(!red.2, "red InvokePrompt arm unexpectedly stayed clean");
        assert_eq!(green.1, 1, "green arm must retain one prompt");
        assert!(green.2, "green arm must stay clean through the final edit");
        assert_eq!(green.3, format!("{expected}Z"));
    }

    /// Regression for `.tmp-repaint-capture/line-anchor-verify.vt`: prompt plus input is exactly
    /// 108 cells at width 54, so D is the wrap-pending next-row column zero. Widening to 56 retains
    /// that physical D even though the painted input's B stays at column 44. The retired arm trusts
    /// `D - cursor`, installs B at column 48 and the later PSReadLine repaint leaves the old `echo`
    /// in front of the new one. The shipped arm recognizes the exact-edge sentinel and carries B.
    #[test]
    #[ignore = "dev probe: reproduces exact-right-edge widening through real PSReadLine and ConPTY"]
    fn exact_right_edge_widen_reanchor_pair_probe() {
        const PROMPT: &str = "(base) PS D:\\Developer\\BetterTerminal\\dist> ";
        const TYPED: &str = "echo D:\\Developer\\BetterTerminal\\.tmp-repaint-capture\\sunset.svg";
        const INITIAL_COLUMNS: u16 = 54;
        const WIDE_COLUMNS: u16 = 56;
        assert_eq!(PROMPT.len(), 44);
        assert_eq!(TYPED.len(), 64);
        assert_eq!(
            (PROMPT.len() + TYPED.len()) % usize::from(INITIAL_COLUMNS),
            0
        );
        let expected = format!("{PROMPT}{TYPED}");
        let startup = invoke_prompt_probe_startup(PROMPT);
        let mut outcomes = Vec::new();

        for retired_exact_edge in [true, false] {
            let arm_startup = if retired_exact_edge {
                format!(
                    "$env:BT_PSREADLINE_REANCHOR_PROBE = '1'; \
                     $env:BT_PSREADLINE_REANCHOR_EXACT_EDGE_PROBE = '1'; {startup}"
                )
            } else {
                format!("$env:BT_PSREADLINE_REANCHOR_PROBE = '1'; {startup}")
            };
            let mut oracle =
                AppResizeOracle::spawn("pwsh.exe", &arm_startup, INITIAL_COLUMNS, 20, false);
            oracle.invoke_prompt_after_resize = true;
            assert!(oracle.settle_at_prompt(PROMPT));
            oracle.pty.write(TYPED.as_bytes()).unwrap();
            oracle.pump_for(Duration::from_millis(900));
            oracle.pump_until_quiet(Duration::from_secs(6));
            assert_eq!(oracle.session.terminal().cursor().column, 0);

            let mark = oracle.raw_output.len();
            oracle.project_resize(WIDE_COLUMNS, 20);
            oracle.pump_for(RESIZE_REQUEST_QUIET * 2 + Duration::from_millis(100));
            oracle.pump_for(Duration::from_millis(1_700));
            oracle.pump_until_quiet(Duration::from_secs(6));
            // The recording's PSReadLine resize poll eventually issued the full repaint on its own.
            // A final edit deterministically asks the same render cache for its next diff, exposing
            // the installed B without relying on that private poll's wall-clock cadence.
            oracle.pty.write(b"Z").unwrap();
            oracle.pump_for(Duration::from_millis(900));
            oracle.pump_until_quiet(Duration::from_secs(6));
            let rows = oracle.composed_rows();
            let cursor = oracle.session.terminal().cursor();
            let input = wrapped_input_line(&rows, cursor.row, PROMPT, WIDE_COLUMNS);
            let output = &oracle.raw_output[mark..];
            let exact_mode = output
                .windows(b"BT_PSREADLINE_REANCHOR=exact-right-edge-widen".len())
                .any(|window| window == b"BT_PSREADLINE_REANCHOR=exact-right-edge-widen");
            let expected_after_edit = format!("{expected}Z");
            let spliced = rows.concat().contains("echoecho") || input != expected_after_edit;
            eprintln!(
                "BT_CONPTY_EXACT_EDGE_PAIR source={} psreadline={} retired_exact_edge={} \
                 spliced={spliced} exact_mode={exact_mode} input={input:?} \
                 expected={expected_after_edit:?} \
                 cursor=({}, {}) rows={:?} output={}",
                conpty_source(),
                psreadline_version(&oracle.raw_output),
                retired_exact_edge,
                cursor.row,
                cursor.column,
                occupied_rows(&rows),
                escaped(output)
            );
            outcomes.push((
                retired_exact_edge,
                spliced,
                exact_mode,
                input,
                expected_after_edit,
            ));
        }

        let red = &outcomes[0];
        let green = &outcomes[1];
        assert!(red.0 && !green.0);
        assert!(
            red.1,
            "the retired physical-D arm must reproduce the splice: {red:?}"
        );
        assert!(!red.2);
        assert!(
            !green.1,
            "the exact-edge guard left the line spliced: {green:?}"
        );
        assert!(
            green.2,
            "the dev marker must prove the exact-edge branch ran"
        );
        assert_eq!(green.3, green.4);
    }

    /// Regression for `.tmp-repaint-capture/render-ledger-verify.vt`: the handler's Console cursor
    /// read emits `CSI 6 n` before the child cursor has settled after a committed resize. The
    /// immediate red arm receives a width-consistent but early CPR at every stop and installs those
    /// intermediate positions into PSReadLine. The green arm retains one repair debt through the
    /// storm and sends it only when the final resize transaction has been quiet after child output.
    #[test]
    #[ignore = "dev probe: races real PSReadLine cursor reads against chained ConPTY resizes"]
    fn reanchor_cursor_cpr_resize_storm_pair_probe() {
        const PROMPT: &str = "(base) PS D:\\Developer\\BetterTerminal\\dist> ";
        const FIRST_HISTORY: &str = "Write-Output ('BT_APP_' + 'INPUT_OK')";
        const SECOND_HISTORY: &str = "echo \"[Image: x]\"";
        const FINAL_COLUMNS: u16 = 54;
        let first_history_literal = FIRST_HISTORY.replace('\'', "''");
        let startup = format!(
            "$env:BT_PSREADLINE_REANCHOR_PROBE = '1'; {}; \
             Set-PSReadLineKeyHandler -Chord Ctrl+g -ScriptBlock {{ \
             if ($global:BT_STORM_HISTORY_STAGE -eq 1) {{ \
             [Microsoft.PowerShell.PSConsoleReadLine]::AddToHistory('{SECOND_HISTORY}') \
             }} else {{ \
             [Microsoft.PowerShell.PSConsoleReadLine]::AddToHistory('{first_history_literal}'); \
             $global:BT_STORM_HISTORY_STAGE = 1 \
             }}; \
             [Console]::Write(([string][char]27) + ']777;BT_STORM_HISTORY_SEEDED' + [char]7) }}",
            invoke_prompt_probe_startup(PROMPT)
        );
        let first_expected = format!("{PROMPT}{FIRST_HISTORY}");
        let second_expected = format!("{PROMPT}{SECOND_HISTORY}");
        let mut outcomes = Vec::new();

        for delayed in [false, true] {
            let arm_startup = if delayed {
                startup.clone()
            } else {
                format!("$env:BT_PSREADLINE_REANCHOR_WHOLE_SCREEN_PROBE = '1'; {startup}")
            };
            let mut oracle =
                AppResizeOracle::spawn("pwsh.exe", &arm_startup, FINAL_COLUMNS, 20, false);
            oracle.invoke_prompt_after_resize = true;
            oracle.reanchor_after_resize_quiescence = delayed;
            assert!(oracle.settle_at_prompt(PROMPT));

            oracle.pty.write(b"\x07").unwrap();
            oracle.pump_for(Duration::from_millis(500));
            oracle.pty.write(b"\x1b[A").unwrap();
            oracle.pump_for(Duration::from_millis(900));
            oracle.pump_until_quiet(Duration::from_secs(6));
            let rows = oracle.session.terminal().visible_text();
            let cursor = oracle.session.terminal().cursor();
            assert_eq!(
                wrapped_input_line(&rows, cursor.row, PROMPT, FINAL_COLUMNS),
                first_expected
            );
            // Put the shorter entry at the top without changing the currently displayed first
            // recall. The repair chord itself is a non-history key and PSReadLine resets its
            // history-navigation index, so the final Up deterministically selects this entry.
            oracle.pty.write(b"\x07").unwrap();
            oracle.pump_for(Duration::from_millis(500));

            let resize_mark = oracle.raw_output.len();
            oracle.cpr_log.clear();
            // Each burst is dense projection with no output pump. Stop long enough to commit its
            // final width; the red arm then gets only its immediate handler exchange before the
            // pointer resumes, never the editor's later resize poll/redraw.
            for widths in [
                &[48u16, 40, 33, 27][..],
                &[35u16, 44, 58, 70][..],
                &[66u16, 62, 58, FINAL_COLUMNS][..],
            ] {
                for columns in widths {
                    oracle.project_resize(*columns, 20);
                }
                oracle.pump_until_committed(Duration::from_secs(3));
                // Let this commit's immediate handler complete its DSR/CPR at the committed grid,
                // but resume well before PSReadLine's later resize poll/redraw. This is the capture's
                // three `6n -> CUP` exchanges without the stronger wrong-local-grid fallback case.
                oracle.pump_for(Duration::from_millis(100));
            }
            oracle.pump_for(Duration::from_millis(1_500));
            oracle.pump_until_quiet(Duration::from_secs(6));

            let recall_mark = oracle.raw_output.len();
            oracle.pty.write(b"\x1b[A").unwrap();
            oracle.pump_for(Duration::from_millis(900));
            oracle.pump_until_quiet(Duration::from_secs(6));

            let rows = oracle.session.terminal().visible_text();
            let cursor = oracle.session.terminal().cursor();
            let input = wrapped_input_line(&rows, cursor.row, PROMPT, FINAL_COLUMNS);
            let joined = rows.concat();
            let residue = joined.contains("Write-Output (")
                || joined.contains("BT_APP_")
                || joined.contains("INPUT_OK");
            let prompt_row = (0..=cursor.row as usize)
                .rev()
                .find(|row| rows[*row].starts_with(PROMPT.trim_end()));
            let total_cells = PROMPT.len() + SECOND_HISTORY.len();
            let expected_cursor = prompt_row.map(|row| {
                (
                    row as u32 + (total_cells / usize::from(FINAL_COLUMNS)) as u32,
                    (total_cells % usize::from(FINAL_COLUMNS)) as u32,
                )
            });
            let cursor_correct = expected_cursor == Some((cursor.row, cursor.column));
            eprintln!(
                "BT_CONPTY_REANCHOR_CPR_STORM source={} psreadline={} delayed={delayed} \
                 commits={:?} writes={} residue={residue} cursor_correct={cursor_correct} \
                 cursor=({}, {}) expected_cursor={expected_cursor:?} input={input:?} \
                 expected={second_expected:?} cprs={:?} rows={:?} resize_output={} \
                 recall_output={}",
                conpty_source(),
                psreadline_version(&oracle.raw_output),
                oracle.commits,
                oracle.invoke_prompt_writes,
                cursor.row,
                cursor.column,
                oracle.cpr_log,
                occupied_rows(&rows),
                escaped(&oracle.raw_output[resize_mark..recall_mark]),
                escaped(&oracle.raw_output[recall_mark..])
            );
            outcomes.push((
                delayed,
                oracle.invoke_prompt_writes,
                residue,
                cursor_correct,
                input,
                oracle
                    .cpr_log
                    .iter()
                    .map(|exchange| {
                        (
                            exchange.reply.clone(),
                            exchange.local_columns,
                            exchange.conpty_columns,
                            exchange.cursor.row,
                            exchange.cursor.column,
                        )
                    })
                    .collect::<Vec<_>>(),
            ));
        }

        let red = &outcomes[0];
        let green = &outcomes[1];
        assert!(!red.0 && green.0);
        assert_eq!(red.1, 3, "the red arm injects beside every commit");
        assert!(
            red.2 || !red.3 || red.4 != second_expected,
            "the red arm must reproduce residue, offset drawing, or a wrong cursor: {red:?}"
        );
        assert!(
            red.5.iter().all(|(_, local, conpty, _, _)| local == conpty),
            "the red arm must prove that width-consistent early CPRs are still unsafe: {red:?}"
        );
        assert_eq!(green.1, 1, "the storm must coalesce to one repair");
        assert!(
            !green.2,
            "the delayed arm left old-history residue: {green:?}"
        );
        assert!(green.3, "the delayed arm left a cursor offset: {green:?}");
        assert_eq!(green.4, second_expected);
        assert!(
            green
                .5
                .iter()
                .all(|(_, local, conpty, _, _)| local == conpty),
            "the delayed arm answered CPR from an unsettled grid: {green:?}"
        );
    }

    /// Regression for `.tmp-repaint-capture/silent-reanchor-verify.vt`: after a committed resize,
    /// Up replaces a non-empty line with a shorter, completely different history entry. The
    /// retired arm leaves `_previousRender` at `_initialPrevRender`; the green arm retains its
    /// screen-matching lines and repairs only the cached console geometry.
    #[test]
    #[ignore = "dev probe: verifies shorter history recall through real PSReadLine and ConPTY"]
    fn nonempty_resize_shorter_history_recall_render_memory_pair_probe() {
        const PROMPT: &str = "BTHISTORY 012345678901234567890123456789> ";
        const HISTORY: &str = "Write-Output BT_SHORT";
        const OLD_BUFFER: &str = "echo D:\\Developer\\BetterTerminal\\.tmp-repaint-capture\\BT_OLD_RESIDUE_MUST_DISAPPEAR";
        // Keep the old width an exact multiple of the new one. That isolates render-memory loss:
        // B retains the same column through ConPTY reflow, so the red residue cannot be blamed on
        // the separate anchor-column calculation.
        const INITIAL_COLUMNS: u16 = 104;
        const RESIZED_COLUMNS: u16 = 52;
        let startup = format!(
            "{}; Set-PSReadLineKeyHandler -Chord Ctrl+g -ScriptBlock {{ \
             [Microsoft.PowerShell.PSConsoleReadLine]::AddToHistory('{HISTORY}'); \
             [Console]::Write(([string][char]27) + ']777;BT_HISTORY_SEEDED' + [char]7) }}",
            invoke_prompt_probe_startup(PROMPT)
        );
        let expected = format!("{PROMPT}{HISTORY}");
        let mut outcomes = Vec::new();

        for retired_empty_baseline in [true, false] {
            let arm_startup = if retired_empty_baseline {
                format!("$env:BT_PSREADLINE_REANCHOR_EMPTY_BASELINE_PROBE = '1'; {startup}")
            } else {
                startup.clone()
            };
            let mut oracle =
                AppResizeOracle::spawn("pwsh.exe", &arm_startup, INITIAL_COLUMNS, 20, false);
            oracle.invoke_prompt_after_resize = true;
            assert!(oracle.settle_at_prompt(PROMPT));
            let seed_mark = oracle.raw_output.len();
            oracle.pty.write(b"\x07").unwrap();
            oracle.pump_for(Duration::from_millis(500));
            assert!(
                oracle.raw_output[seed_mark..]
                    .windows(b"BT_HISTORY_SEEDED".len())
                    .any(|window| window == b"BT_HISTORY_SEEDED"),
                "the in-editor history seeding handler did not run"
            );
            oracle.pty.write(OLD_BUFFER.as_bytes()).unwrap();
            oracle.pump_for(Duration::from_millis(900));
            oracle.pump_until_quiet(Duration::from_secs(6));
            assert!(oracle.session.typed_shell_input_live());

            let resize_mark = oracle.raw_output.len();
            oracle.project_resize(RESIZED_COLUMNS, 20);
            oracle.pump_for(RESIZE_REQUEST_QUIET * 2 + Duration::from_millis(100));
            oracle.pump_for(Duration::from_millis(1_500));
            oracle.pump_until_quiet(Duration::from_secs(6));
            let recall_mark = oracle.raw_output.len();
            oracle.pty.write(b"\x1b[A").unwrap();
            oracle.pump_for(Duration::from_millis(900));
            oracle.pump_until_quiet(Duration::from_secs(6));

            let rows = oracle.session.terminal().visible_text();
            let cursor = oracle.session.terminal().cursor();
            let input = wrapped_input_line(&rows, cursor.row, PROMPT, RESIZED_COLUMNS);
            let residue = rows.concat().contains("BT_OLD_RESIDUE_MUST_DISAPPEAR");
            let opening = &PROMPT[..usize::from(RESIZED_COLUMNS).min(PROMPT.len())];
            let prompt_row = (0..=cursor.row as usize)
                .rev()
                .find(|row| rows[*row].starts_with(opening));
            let total_cells = PROMPT.len() + HISTORY.len();
            let expected_cursor = prompt_row.map(|row| {
                (
                    row as u32 + (total_cells / usize::from(RESIZED_COLUMNS)) as u32,
                    (total_cells % usize::from(RESIZED_COLUMNS)) as u32,
                )
            });
            let cursor_correct = expected_cursor == Some((cursor.row, cursor.column));
            let recall_output = &oracle.raw_output[recall_mark..];
            eprintln!(
                "BT_CONPTY_SHORTER_HISTORY_PAIR source={} psreadline={} \
                 retired_empty_baseline={retired_empty_baseline} writes={} residue={residue} \
                 cursor_correct={cursor_correct} cursor=({}, {}) expected_cursor={expected_cursor:?} \
                 input={input:?} expected={expected:?} rows={:?} resize_output={} \
                 recall_addresses={:?} recall_output={}",
                conpty_source(),
                psreadline_version(&oracle.raw_output),
                oracle.invoke_prompt_writes,
                cursor.row,
                cursor.column,
                occupied_rows(&rows),
                escaped(&oracle.raw_output[resize_mark..recall_mark]),
                redraw_addresses(recall_output),
                escaped(recall_output)
            );
            outcomes.push((
                retired_empty_baseline,
                residue,
                cursor_correct,
                input,
                (cursor.row, cursor.column),
            ));
        }

        let red = &outcomes[0];
        let green = &outcomes[1];
        assert!(red.0 && !green.0);
        assert!(
            red.1 || red.3 != expected || !red.2,
            "red arm must reproduce residue, an incomplete line, or a wrong cursor: {red:?}"
        );
        assert!(!green.1, "green arm left old-buffer glyphs: {green:?}");
        assert_eq!(
            green.3, expected,
            "green arm did not recall one complete line"
        );
        assert!(
            green.2,
            "green arm left the cursor at the wrong cell: {green:?}"
        );
    }

    /// CJK uses PSReadLine's reflected cell-width routine, not Rust's terminal width or an
    /// integration-local Unicode table. This arm crosses the same three committed geometries and
    /// then edits once more, so a one-cell anchor error cannot hide at the final cursor.
    #[test]
    #[ignore = "dev probe: verifies CJK cell math through real PSReadLine and ConPTY"]
    fn nonempty_cjk_buffer_committed_resize_chain_reanchor_probe() {
        const PROMPT: &str = "BTCJK 012345678901234567890123456789012> ";
        const TYPED: &str =
            "Write-Output '中文宽字符锚点验证中文宽字符锚点验证01234567890123456789'";
        const WIDTHS: [u16; 3] = [24, 100, 24];
        let startup = invoke_prompt_probe_startup(PROMPT);
        let expected = format!("{PROMPT}{TYPED}");
        // The terminal text oracle represents a wide cell's continuation half as a blank. Compare
        // the glyph stream without those placeholders; the probe input itself contains no
        // semantically significant repeated whitespace, and prompt copy count is checked apart.
        let glyphs = |text: &str| text.replace(' ', "");
        let mut oracle = AppResizeOracle::spawn("pwsh.exe", &startup, 38, 24, false);
        oracle.invoke_prompt_after_resize = true;
        assert!(
            oracle
                .settle_at_prompt_matching(&|line| line == PROMPT[38..].trim_end())
                .is_some()
        );
        oracle.pty.write(TYPED.as_bytes()).unwrap();
        oracle.pump_for(Duration::from_millis(900));
        oracle.pump_until_quiet(Duration::from_secs(6));

        for columns in WIDTHS {
            let mark = oracle.raw_output.len();
            oracle.project_resize(columns, 24);
            oracle.pump_for(RESIZE_REQUEST_QUIET * 2 + Duration::from_millis(100));
            oracle.pump_for(Duration::from_millis(1_500));
            oracle.pump_until_quiet(Duration::from_secs(6));
            let rows = oracle.composed_rows();
            let cursor = oracle.session.terminal().cursor();
            let input = wrapped_input_line(&rows, cursor.row, PROMPT, columns);
            let prompt_copies = rows.concat().match_indices("BTCJK ").count();
            eprintln!(
                "BT_CONPTY_NONEMPTY_CJK_STEP source={} psreadline={} columns={columns} \
                 writes={} prompt_copies={prompt_copies} input={input:?} expected={expected:?} \
                 rows={:?} output={}",
                conpty_source(),
                psreadline_version(&oracle.raw_output),
                oracle.invoke_prompt_writes,
                occupied_rows(&rows),
                escaped(&oracle.raw_output[mark..])
            );
            assert_eq!(prompt_copies, 1);
            assert_eq!(glyphs(&input), glyphs(&expected));
        }

        oracle.pty.write("界".as_bytes()).unwrap();
        oracle.pump_for(Duration::from_millis(900));
        oracle.pump_until_quiet(Duration::from_secs(6));
        let rows = oracle.composed_rows();
        let cursor = oracle.session.terminal().cursor();
        let input = wrapped_input_line(&rows, cursor.row, PROMPT, 24);
        let expected_after_edit = format!("{expected}界");
        let prompt_copies = rows.concat().match_indices("BTCJK ").count();
        eprintln!(
            "BT_CONPTY_NONEMPTY_CJK_SUMMARY source={} psreadline={} writes={} \
             prompt_copies={prompt_copies} input={input:?} expected={expected_after_edit:?} \
             rows={:?}",
            conpty_source(),
            psreadline_version(&oracle.raw_output),
            oracle.invoke_prompt_writes,
            occupied_rows(&rows)
        );
        assert_eq!(prompt_copies, 1);
        assert_eq!(glyphs(&input), glyphs(&expected_after_edit));
    }
}
