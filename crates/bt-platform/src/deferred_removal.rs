//! **The uninstall remover** — a native copy of the running executable which
//! outlives every process using the installed image, verifies each file again
//! at the destructive boundary, and reports a truthful final result.
//!
//! `schedule` creates an unpredictable directory below the caller's per-user
//! Folio directory, copies the running executable there, holds the copy open
//! against replacement until the child acknowledges readiness, and starts it
//! as `--uninstall-remove`. No script or command interpreter is involved.
//! The child waits by `(pid, start time)`, for at most five minutes, and then
//! retries held removals with bounded backoff. A file is removed only when its
//! current length and SHA-256 still equal the expected identity. Any failure is
//! written beside the remover as `result.txt` and shown through
//! `standalone_alert`; the alert names both the remaining paths and that file.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::admission::WorkerCtx;
use crate::install_flip::Running;

/// The private argv door served by the copied executable.
pub const REMOVE_FLAG: &str = "--uninstall-remove";

/// Every process wait, including a persistent parent shell, ends here.
pub const REMOVAL_TIMEOUT: Duration = Duration::from_secs(5 * 60);

const WAIT_POLL: Duration = Duration::from_millis(100);
const READY_WITHIN: Duration = Duration::from_secs(10);
const DELETE_ATTEMPTS: usize = 10;
const FIRST_DELETE_BACKOFF: Duration = Duration::from_millis(50);
const MAX_DELETE_BACKOFF: Duration = Duration::from_secs(2);
const READY_LINE: &str = "ready";
const RESULT_NAME: &str = "result.txt";
const COPY_NAME_WINDOWS: &str = "folio-remover.exe";
const COPY_NAME_UNIX: &str = "folio-remover";
const ENV_PREFIX: &str = "FOLIO_NATIVE_REMOVAL_";

/// SHA-256 and length expected of one regular file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileIdentity {
    pub size: u64,
    pub sha256: String,
}

impl FileIdentity {
    /// Read a regular, single-link file and identify its bytes.
    ///
    /// # Errors
    /// The path is not a regular file, is shared by hard links, or could not be
    /// read. The opened file, rather than a second path lookup, supplies all
    /// three facts.
    pub fn of(path: &Path) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        if crate::cleanup::is_link(&metadata) {
            return Err(io::Error::other(format!(
                "{} is not a regular unlinked file",
                path.display()
            )));
        }
        let file = File::open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::other(format!(
                "{} is not a regular unlinked file",
                path.display()
            )));
        }
        if crate::file_replace::file_link_count(&file)? != 1 {
            return Err(io::Error::other(format!(
                "{} has more than one hard link",
                path.display()
            )));
        }
        let size = metadata.len();
        Ok(Self {
            size,
            sha256: digest(file)?,
        })
    }
}

/// One thing the native remover may remove.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// A regular, single-link file with the identity established by the plan.
    File {
        path: PathBuf,
        expected: FileIdentity,
    },
    /// A directory removed only when empty, after its separately identified
    /// files and deeper directories. There is no recursive deletion here.
    Directory(PathBuf),
}

impl Item {
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::File { path, .. } | Self::Directory(path) => path,
        }
    }

    #[must_use]
    fn kind(&self) -> &'static str {
        match self {
            Self::File { .. } => "file",
            Self::Directory(_) => "directory",
        }
    }
}

/// A process identity and the name used if the five-minute bound finds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaitFor {
    pub process: Running,
    pub name: OsString,
}

/// Localized words which the windowless remover can use without reopening the
/// settings after the cleanup has removed them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailureWords {
    pub title: String,
    pub still_running: String,
    pub files_left: String,
    pub result_at: String,
}

/// The complete immutable removal handed to the copied executable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Removal {
    /// The running executable copied to become the remover.
    pub program: PathBuf,
    /// The identity the copied bytes must have before they may be executed.
    pub program_identity: FileIdentity,
    pub after: Vec<WaitFor>,
    pub items: Vec<Item>,
    /// Removed after the items, and only when empty.
    pub folder: Option<PathBuf>,
    pub words: FailureWords,
}

/// Create the native remover under `private_root`, start it with breakaway
/// semantics, and return only after it has parsed its immutable inherited
/// environment and acknowledged readiness.
///
/// # Errors
/// The private directory/copy could not be made, the copy differed from the
/// running executable, the job object refused breakaway, or the child did not
/// reach its readiness handshake. Nothing has been removed in those cases.
pub fn schedule(worker: &WorkerCtx, removal: &Removal, private_root: &Path) -> io::Result<()> {
    validate(removal, private_root)?;
    fs::create_dir_all(private_root)?;
    validate_private_root(private_root)?;
    let private = private_directory(private_root)?;
    let copy = private.join(if cfg!(windows) {
        COPY_NAME_WINDOWS
    } else {
        COPY_NAME_UNIX
    });
    if let Err(error) = copy_new(worker, &removal.program, &copy) {
        discard_private(&copy, &private);
        return Err(error);
    }
    match FileIdentity::of(&copy) {
        Ok(identity) if identity == removal.program_identity => {}
        Ok(_) => {
            discard_private(&copy, &private);
            return Err(io::Error::other(
                "the private remover copy differs from the running executable",
            ));
        }
        Err(error) => {
            discard_private(&copy, &private);
            return Err(error);
        }
    }

    // On Windows this handle denies write and delete sharing until the child
    // says it is running from the copy. After that the image section holds the
    // executable. It closes the create-to-execute replacement window.
    let copy_guard = match guard_copy(&copy) {
        Ok(guard) => guard,
        Err(error) => {
            discard_private(&copy, &private);
            return Err(error);
        }
    };
    #[cfg(not(test))]
    let mut command = crate::quiet_breakaway_command(&copy);
    // libtest on Windows is commonly placed in a job which deliberately
    // forbids breakaway. The product arm above treats that refusal as a failed
    // schedule; this harness arm keeps the native-copy exercise runnable.
    #[cfg(test)]
    let mut command = crate::quiet_command(&copy);
    command
        // The copied process removes `private` and, when empty, its Folio
        // parent before it exits. Its current directory must therefore live
        // outside both of them.
        .current_dir(private_root.parent().unwrap_or(private_root))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Not the terminal's foreground process group: closing the terminal
        // must not hang up the native remover before it reports its result.
        command.process_group(0);
    }
    put_environment(&mut command, removal, &private);
    #[cfg(not(test))]
    command.arg(REMOVE_FLAG);
    #[cfg(test)]
    command.args([
        "--exact",
        "deferred_removal::tests::a_native_remover_copy_runs_the_plan",
        "--nocapture",
    ]);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            drop(copy_guard);
            discard_private(&copy, &private);
            return Err(error);
        }
    };
    let Some(ready) = child.stdout.take() else {
        drop(copy_guard);
        return stop_child(
            worker,
            child,
            &copy,
            &private,
            io::Error::other("the remover has no readiness pipe"),
        );
    };
    let Some(mut release) = child.stdin.take() else {
        drop(copy_guard);
        return stop_child(
            worker,
            child,
            &copy,
            &private,
            io::Error::other("the remover has no release pipe"),
        );
    };
    let (sent, received) = mpsc::sync_channel(1);
    if let Err(error) = crate::spawn_at_priority(
        "folio-remover-ready",
        crate::ThreadPriority::BelowNormal,
        move |_| {
            let mut reader = BufReader::new(ready);
            let answer = loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break Ok(line),
                    Ok(_) if line.trim_end() == READY_LINE => break Ok(line),
                    Ok(_) if line.starts_with("refused:") => break Ok(line),
                    Ok(_) => {}
                    Err(error) => break Err(error),
                }
            };
            let _ = sent.send(answer);
        },
    ) {
        drop(copy_guard);
        return stop_child(worker, child, &copy, &private, error);
    }
    let answer = received.recv_timeout(READY_WITHIN);
    match answer {
        Ok(Ok(line)) if line.trim_end() == READY_LINE => {
            match child.try_wait() {
                Ok(Some(status)) => {
                    drop(copy_guard);
                    return stop_child(
                        worker,
                        child,
                        &copy,
                        &private,
                        io::Error::other(format!(
                            "the remover exited after readiness with {status}"
                        )),
                    );
                }
                Ok(None) => {}
                Err(error) => {
                    drop(copy_guard);
                    return stop_child(worker, child, &copy, &private, error);
                }
            }
            // The child has parsed and authenticated the inherited plan but
            // cannot touch it until this byte arrives. This makes readiness a
            // real hand-off: the scheduler observes a live remover while the
            // executable copy is still held against replacement.
            if let Err(error) = release.write_all(b"go\n").and_then(|()| release.flush()) {
                drop(copy_guard);
                return stop_child(worker, child, &copy, &private, error);
            }
            drop(release);
            drop(copy_guard);
            Ok(())
        }
        Ok(Ok(line)) => {
            drop(copy_guard);
            stop_child(
                worker,
                child,
                &copy,
                &private,
                io::Error::other(format!("unexpected remover readiness: {line:?}")),
            )
        }
        Ok(Err(error)) => {
            drop(copy_guard);
            stop_child(worker, child, &copy, &private, error)
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            drop(copy_guard);
            stop_child(
                worker,
                child,
                &copy,
                &private,
                io::Error::new(io::ErrorKind::TimedOut, "the remover did not become ready"),
            )
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            drop(copy_guard);
            stop_child(
                worker,
                child,
                &copy,
                &private,
                io::Error::other("the remover readiness pipe closed"),
            )
        }
    }
}

fn validate_private_root(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if crate::cleanup::is_link(&metadata) || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the private remover root is not an ordinary directory",
        ));
    }
    let account = crate::install_evidence::current_account()?;
    let owner = crate::install_evidence::owner_of(path)?;
    if !account.owns(&owner) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the private remover root is not owned by this account",
        ));
    }
    Ok(())
}

fn stop_child(
    _worker: &WorkerCtx,
    mut child: Child,
    copy: &Path,
    private: &Path,
    error: io::Error,
) -> io::Result<()> {
    let _ = child.kill();
    let _ = child.wait();
    discard_private(copy, private);
    Err(error)
}

fn discard_private(copy: &Path, private: &Path) {
    let _ = fs::remove_file(copy);
    let _ = fs::remove_dir(private);
}

fn validate(removal: &Removal, private_root: &Path) -> io::Result<()> {
    if !private_root.is_absolute() || !removal.program.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the remover and its private root must be absolute",
        ));
    }
    if removal.items.iter().any(|item| !item.path().is_absolute())
        || removal
            .folder
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "every removal path must be absolute",
        ));
    }
    Ok(())
}

fn private_directory(root: &Path) -> io::Result<PathBuf> {
    for _ in 0..32 {
        let nonce = crate::attention_pipe::unguessable_bits();
        let path = root.join(format!("uninstall-{nonce:032x}"));
        #[cfg(unix)]
        let made = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            builder.create(&path)
        };
        #[cfg(not(unix))]
        let made = fs::create_dir(&path);
        match made {
            Ok(()) => {
                let account = crate::install_evidence::current_account()?;
                let owner = crate::install_evidence::owner_of(&path)?;
                if !account.owns(&owner) {
                    let _ = fs::remove_dir(&path);
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "the private remover directory is not owned by this account",
                    ));
                }
                return Ok(path);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create a fresh private remover directory",
    ))
}

fn copy_new(_worker: &WorkerCtx, from: &Path, to: &Path) -> io::Result<()> {
    let mut source = File::open(from)?;
    let mut target = OpenOptions::new().write(true).create_new(true).open(to)?;
    io::copy(&mut source, &mut target)?;
    target.sync_all()?;
    fs::set_permissions(to, source.metadata()?.permissions())?;
    Ok(())
}

#[cfg(windows)]
fn guard_copy(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 1;
    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)
}

#[cfg(not(windows))]
fn guard_copy(path: &Path) -> io::Result<File> {
    File::open(path)
}

fn key(name: &str) -> String {
    format!("{ENV_PREFIX}{name}")
}

fn indexed(name: &str, index: usize) -> String {
    key(&format!("{name}_{index}"))
}

fn put_environment(command: &mut std::process::Command, removal: &Removal, private: &Path) {
    command
        .env(key("PRIVATE"), private)
        .env(key("PROGRAM"), &removal.program)
        .env(
            key("PROGRAM_SIZE"),
            removal.program_identity.size.to_string(),
        )
        .env(key("PROGRAM_SHA256"), &removal.program_identity.sha256)
        .env(key("WAIT_COUNT"), removal.after.len().to_string())
        .env(key("ITEM_COUNT"), removal.items.len().to_string())
        .env(key("TITLE"), &removal.words.title)
        .env(key("STILL_RUNNING"), &removal.words.still_running)
        .env(key("FILES_LEFT"), &removal.words.files_left)
        .env(key("RESULT_AT"), &removal.words.result_at);
    if let Some(folder) = &removal.folder {
        command.env(key("FOLDER"), folder);
    }
    for (index, waited) in removal.after.iter().enumerate() {
        command
            .env(indexed("WAIT_PID", index), waited.process.pid.to_string())
            .env(
                indexed("WAIT_STARTED", index),
                waited.process.started.to_string(),
            )
            .env(indexed("WAIT_NAME", index), &waited.name);
    }
    for (index, item) in removal.items.iter().enumerate() {
        command
            .env(indexed("ITEM_KIND", index), item.kind())
            .env(indexed("ITEM_PATH", index), item.path());
        if let Item::File { expected, .. } = item {
            command
                .env(indexed("ITEM_SIZE", index), expected.size.to_string())
                .env(indexed("ITEM_SHA256", index), &expected.sha256);
        }
    }
}

/// Run the private remover described by the inherited environment. This is the
/// entire body of `folio.exe --uninstall-remove`.
pub fn run_from_environment(worker: &WorkerCtx) -> i32 {
    let parsed = removal_from_environment(worker);
    let (removal, private) = match parsed {
        Ok(value) => value,
        Err(error) => {
            return failure_without_plan(worker, error);
        }
    };
    if writeln!(io::stdout(), "{READY_LINE}")
        .and_then(|_| io::stdout().flush())
        .is_err()
    {
        return 1;
    }
    let mut release = String::new();
    if io::stdin().read_line(&mut release).is_err() || release.trim_end() != "go" {
        return 1;
    }
    let result = perform(worker, &removal, REMOVAL_TIMEOUT);
    finish(
        worker,
        &removal.words,
        &private,
        &removal.program_identity,
        result,
    )
}

fn failure_without_plan(_worker: &WorkerCtx, error: io::Error) -> i32 {
    let _ = writeln!(io::stdout(), "refused: {error}");
    let _ = io::stdout().flush();
    1
}

fn removal_from_environment(_worker: &WorkerCtx) -> io::Result<(Removal, PathBuf)> {
    let private = required_path("PRIVATE")?;
    if !private
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("uninstall-") && name.len() > "uninstall-".len())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the remover directory has an invalid name",
        ));
    }
    let program = required_path("PROGRAM")?;
    let program_identity = FileIdentity {
        size: required_number("PROGRAM_SIZE")?,
        sha256: required_string("PROGRAM_SHA256")?,
    };
    let wait_count: usize = required_number("WAIT_COUNT")?;
    let item_count: usize = required_number("ITEM_COUNT")?;
    if wait_count > 1024 || item_count > 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the removal plan is too large",
        ));
    }
    let mut after = Vec::with_capacity(wait_count);
    for index in 0..wait_count {
        after.push(WaitFor {
            process: Running {
                pid: required_indexed_number("WAIT_PID", index)?,
                started: required_indexed_number("WAIT_STARTED", index)?,
            },
            name: required_indexed("WAIT_NAME", index)?,
        });
    }
    let mut items = Vec::with_capacity(item_count);
    for index in 0..item_count {
        let kind = required_indexed("ITEM_KIND", index)?;
        let path = PathBuf::from(required_indexed("ITEM_PATH", index)?);
        match kind.to_str() {
            Some("file") => items.push(Item::File {
                path,
                expected: FileIdentity {
                    size: required_indexed_number("ITEM_SIZE", index)?,
                    sha256: required_indexed("ITEM_SHA256", index)?
                        .into_string()
                        .map_err(|_| io::Error::other("a digest is not Unicode"))?,
                },
            }),
            Some("directory") => items.push(Item::Directory(path)),
            _ => return Err(io::Error::other("an item has an unknown kind")),
        }
    }
    let removal = Removal {
        program,
        program_identity,
        after,
        items,
        folder: std::env::var_os(key("FOLDER")).map(PathBuf::from),
        words: FailureWords {
            title: required_string("TITLE")?,
            still_running: required_string("STILL_RUNNING")?,
            files_left: required_string("FILES_LEFT")?,
            result_at: required_string("RESULT_AT")?,
        },
    };
    validate(&removal, &private)?;
    let current = std::env::current_exe()?;
    let current_parent = current
        .parent()
        .ok_or_else(|| io::Error::other("the remover executable has no parent"))?;
    if fs::canonicalize(current_parent)? != fs::canonicalize(&private)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the remover is not running from its private directory",
        ));
    }
    if FileIdentity::of(&current)? != removal.program_identity {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the remover copy no longer matches the planned program",
        ));
    }
    Ok((removal, private))
}

fn required(name: &str) -> io::Result<OsString> {
    std::env::var_os(key(name))
        .ok_or_else(|| io::Error::other(format!("missing remover field {name}")))
}

fn required_string(name: &str) -> io::Result<String> {
    required(name)?
        .into_string()
        .map_err(|_| io::Error::other(format!("remover field {name} is not Unicode")))
}

fn required_path(name: &str) -> io::Result<PathBuf> {
    Ok(PathBuf::from(required(name)?))
}

fn required_number<T: std::str::FromStr>(name: &str) -> io::Result<T> {
    required_string(name)?
        .parse()
        .map_err(|_| io::Error::other(format!("remover field {name} is not a number")))
}

fn required_indexed(name: &str, index: usize) -> io::Result<OsString> {
    std::env::var_os(indexed(name, index))
        .ok_or_else(|| io::Error::other(format!("missing remover field {name}_{index}")))
}

fn required_indexed_number<T: std::str::FromStr>(name: &str, index: usize) -> io::Result<T> {
    required_indexed(name, index)?
        .into_string()
        .map_err(|_| io::Error::other(format!("remover field {name}_{index} is not Unicode")))?
        .parse()
        .map_err(|_| io::Error::other(format!("remover field {name}_{index} is not a number")))
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Removed,
    TimedOut(Vec<OsString>),
    Left(Vec<PathBuf>),
}

fn perform(worker: &WorkerCtx, removal: &Removal, within: Duration) -> Outcome {
    perform_with(
        worker,
        removal,
        within,
        DELETE_ATTEMPTS,
        FIRST_DELETE_BACKOFF,
    )
}

fn perform_with(
    worker: &WorkerCtx,
    removal: &Removal,
    within: Duration,
    attempts: usize,
    first_backoff: Duration,
) -> Outcome {
    let until = Instant::now() + within;
    loop {
        let mut running: Vec<OsString> = removal
            .after
            .iter()
            .filter(|waited| crate::install_flip::still_running(waited.process))
            .map(|waited| waited.name.clone())
            .collect();
        let images = match crate::install_flip::running_from(&removal.program) {
            Ok(images) => images,
            Err(error) if error.kind() == io::ErrorKind::Unsupported => Vec::new(),
            Err(_) => return Outcome::Left(vec![removal.program.clone()]),
        };
        for process in images {
            let name = crate::install_flip::image_name(process).unwrap_or_else(|| {
                removal
                    .program
                    .file_name()
                    .unwrap_or_else(|| std::ffi::OsStr::new("folio"))
                    .to_os_string()
            });
            if !running.contains(&name) {
                running.push(name);
            }
        }
        if running.is_empty() {
            break;
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Outcome::TimedOut(running);
        }
        crate::wait::sleep_within(worker, WAIT_POLL.min(left));
    }

    let mut remaining: Vec<&Item> = removal.items.iter().collect();
    let mut folder_left = removal.folder.as_deref();
    let mut backoff = first_backoff;
    for attempt in 0..attempts {
        remaining.retain(|item| !remove_item(item).unwrap_or(false));
        if remaining.is_empty()
            && let Some(folder) = folder_left
            && remove_empty_folder(folder).unwrap_or(false)
        {
            folder_left = None;
        }
        if remaining.is_empty() && folder_left.is_none() {
            return Outcome::Removed;
        }
        if attempt + 1 < attempts {
            crate::wait::sleep_within(worker, backoff);
            backoff = (backoff * 2).min(MAX_DELETE_BACKOFF);
        }
    }
    let mut left: Vec<PathBuf> = remaining
        .into_iter()
        .filter(|item| item.path().exists())
        .map(|item| item.path().to_path_buf())
        .collect();
    if let Some(folder) = folder_left.filter(|folder| folder.exists()) {
        left.push(folder.to_path_buf());
    }
    if left.is_empty() {
        Outcome::Removed
    } else {
        Outcome::Left(left)
    }
}

fn remove_item(item: &Item) -> io::Result<bool> {
    match item {
        Item::File { path, expected } => remove_verified_file(path, expected),
        Item::Directory(path) => remove_empty_folder(path),
    }
}

fn remove_verified_file(path: &Path, expected: &FileIdentity) -> io::Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(error),
    };
    if crate::cleanup::is_link(&metadata) || !metadata.is_file() {
        return Ok(false);
    }
    let file = File::open(path)?;
    if crate::file_replace::file_link_count(&file)? != 1 {
        return Ok(false);
    }
    if file.metadata()?.len() != expected.size || digest(file)? != expected.sha256 {
        return Ok(false);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const READ_ONLY: u32 = 1;
        if metadata.file_attributes() & READ_ONLY != 0 {
            crate::file_replace::set_file_attributes(
                path,
                metadata.file_attributes() & !READ_ONLY,
            )?;
        }
    }
    fs::remove_file(path)?;
    Ok(!path.exists())
}

fn remove_empty_folder(path: &Path) -> io::Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(error),
    };
    if crate::cleanup::is_link(&metadata) || !metadata.is_dir() {
        return Ok(false);
    }
    match fs::remove_dir(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
    }
}

fn digest(mut file: File) -> io::Result<String> {
    let mut hasher = bt_winres::digest::Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(bt_winres::digest::hex(&hasher.finish()))
}

fn finish(
    _worker: &WorkerCtx,
    words: &FailureWords,
    private: &Path,
    remover_identity: &FileIdentity,
    outcome: Outcome,
) -> i32 {
    let result = private.join(RESULT_NAME);
    let failure = failure_summary(words, outcome);
    if let Some(failure) = failure {
        let text = format!("{failure}\n{}\n{}", words.result_at, result.display());
        let written = fs::write(&result, format!("{text}\n"));
        let self_result = retire_self(private, remover_identity);
        let mut shown = match written {
            Ok(()) => text,
            Err(error) => format!("{failure}\n{error}"),
        };
        if let Err(error) = self_result {
            shown = format!("{shown}\n{}\n{error}", words.files_left);
        };
        show_failure(&words.title, &shown);
        1
    } else {
        match retire_self(private, remover_identity) {
            Ok(()) => 0,
            Err(error) => {
                let executable = std::env::current_exe()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|_| "folio-remover".to_owned());
                let text = format!(
                    "{}\n{}\n{}\n{}\n{error}",
                    words.files_left,
                    executable,
                    words.result_at,
                    result.display()
                );
                let _ = fs::write(&result, format!("{text}\n"));
                show_failure(&words.title, &text);
                1
            }
        }
    }
}

#[cfg(not(test))]
fn show_failure(title: &str, text: &str) {
    crate::standalone_alert(title, text);
}

// A failing remover test must never open an ownerless native window. The text
// and result-file contract are exercised through `failure_text` instead.
#[cfg(test)]
fn show_failure(_title: &str, _text: &str) {}

fn failure_summary(words: &FailureWords, outcome: Outcome) -> Option<String> {
    match outcome {
        Outcome::Removed => None,
        Outcome::TimedOut(names) => Some(format!(
            "{}\n{}",
            words.still_running,
            names
                .iter()
                .map(|name| name.to_string_lossy())
                .collect::<Vec<_>>()
                .join("\n")
        )),
        Outcome::Left(paths) => Some(format!(
            "{}\n{}",
            words.files_left,
            paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join("\n")
        )),
    }
}

#[cfg(test)]
fn failure_text(words: &FailureWords, result: &Path, outcome: Outcome) -> Option<String> {
    failure_summary(words, outcome)
        .map(|failure| format!("{failure}\n{}\n{}", words.result_at, result.display()))
}

fn retire_self(private: &Path, expected: &FileIdentity) -> io::Result<()> {
    let executable = std::env::current_exe()?;
    if FileIdentity::of(&executable)? != *expected {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the remover executable was replaced",
        ));
    }
    self_delete(&executable)?;
    let _ = fs::remove_dir(private);
    if let Some(per_user_folio) = private.parent() {
        // The default keep-data road leaves this non-empty. The remove-data
        // road may have emptied it before scheduling us; do not recreate an
        // otherwise-deleted data folder merely to host the remover.
        let _ = fs::remove_dir(per_user_folio);
    }
    Ok(())
}

#[cfg(unix)]
fn self_delete(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

#[cfg(windows)]
fn self_delete(path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, DELETE, FILE_ATTRIBUTE_NORMAL, FILE_DISPOSITION_FLAG_DELETE,
        FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO_EX,
        FILE_DISPOSITION_INFO_EX_FLAGS, FILE_RENAME_INFO, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FileDispositionInfoEx, FileRenameInfo, OPEN_EXISTING,
        SetFileInformationByHandle,
    };
    use windows::core::PCWSTR;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let open = || {
        // SAFETY: the terminated path lives across the call; the returned
        // handle is immediately placed in `OwnedHandle`.
        let opened = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                DELETE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        }
        .map_err(|_| io::Error::last_os_error())?;
        // SAFETY: `opened` is owned by this call alone.
        Ok::<_, io::Error>(unsafe { OwnedHandle::from_raw_handle(opened.0) })
    };
    // A mapped Windows image cannot be given POSIX delete disposition at its
    // ordinary stream. Renaming that stream first detaches the mapped image
    // from the file's deletable name without introducing another filesystem
    // entry. The containing directory is unpredictable and account-owned, so
    // this fixed stream name belongs to this fresh copy alone.
    let owned = open()?;
    let stream: Vec<u16> = std::ffi::OsStr::new(":folio-remover")
        .encode_wide()
        .collect();
    let header = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let length = header + stream.len() * std::mem::size_of::<u16>();
    let mut buffer = vec![0u64; length.div_ceil(std::mem::size_of::<u64>()) + 1];
    let rename = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: `buffer` is aligned for `FILE_RENAME_INFO`, has `length` bytes,
    // and `FileNameLength` describes the copied UTF-16 stream name.
    unsafe {
        (*rename).Anonymous.ReplaceIfExists = false;
        (*rename).RootDirectory = HANDLE::default();
        (*rename).FileNameLength = u32::try_from(stream.len() * 2)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a stream name too long"))?;
        std::ptr::copy_nonoverlapping(
            stream.as_ptr(),
            buffer.as_mut_ptr().cast::<u8>().add(header).cast::<u16>(),
            stream.len(),
        );
        SetFileInformationByHandle(
            HANDLE(owned.as_raw_handle()),
            FileRenameInfo,
            rename.cast(),
            u32::try_from(length)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "rename data too long"))?,
        )
    }
    .map_err(|_| io::Error::last_os_error())?;
    drop(owned);

    let owned = open()?;
    let disposition = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_INFO_EX_FLAGS(
            FILE_DISPOSITION_FLAG_DELETE.0 | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS.0,
        ),
    };
    // SAFETY: the handle and fixed-size structure are live for the call.
    unsafe {
        SetFileInformationByHandle(
            HANDLE(owned.as_raw_handle()),
            FileDispositionInfoEx,
            &raw const disposition as *const _,
            u32::try_from(std::mem::size_of::<FILE_DISPOSITION_INFO_EX>())
                .expect("FILE_DISPOSITION_INFO_EX fits u32"),
        )
    }
    .map_err(|_| io::Error::last_os_error())?;
    drop(owned);
    Ok(())
}

#[cfg(not(any(windows, unix)))]
fn self_delete(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native self-removal is unsupported on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    fn sandbox(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "folio-native-removal-{tag}-{}-{:032x}",
            std::process::id(),
            crate::attention_pipe::unguessable_bits()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn identity(path: &Path) -> FileIdentity {
        FileIdentity::of(path).unwrap()
    }

    fn words() -> FailureWords {
        FailureWords {
            title: "Folio".to_owned(),
            still_running: "Still running:".to_owned(),
            files_left: "Folio could not remove:".to_owned(),
            result_at: "Details were saved to:".to_owned(),
        }
    }

    fn removal(program: &Path, item: Item, folder: Option<PathBuf>) -> Removal {
        Removal {
            program: program.to_path_buf(),
            program_identity: identity(program),
            after: Vec::new(),
            items: vec![item],
            folder,
            words: words(),
        }
    }

    fn on_worker<T: Send + 'static>(body: impl FnOnce(&WorkerCtx) -> T + Send + 'static) -> T {
        match crate::spawn_at_priority(
            "folio-native-removal-test",
            crate::ThreadPriority::BelowNormal,
            body,
        )
        .unwrap()
        .join()
        {
            Ok(answer) => answer,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    /// RED (T-UNINSTALL-UX round 2, mutation `skip_boundary_digest`) — a file
    /// replaced after planning is not deleted and is named by the outcome.
    #[test]
    fn a_post_plan_replacement_is_not_deleted() {
        let root = sandbox("replacement");
        let file = root.join("folio.exe");
        fs::write(&file, b"planned").unwrap();
        let expected = identity(&file);
        fs::write(&file, b"personal replacement").unwrap();
        let item = Item::File {
            path: file.clone(),
            expected,
        };
        assert!(!remove_item(&item).unwrap());
        assert_eq!(fs::read(&file).unwrap(), b"personal replacement");
        fs::remove_dir_all(root).unwrap();
    }

    /// RED (T-UNINSTALL-UX round 2, mutation `accept_multiple_links`) — the
    /// identity reader refuses a hard-linked file before it can enter a plan.
    #[test]
    fn a_hard_link_is_not_an_owned_file() {
        let root = sandbox("hard-link");
        let file = root.join("folio.exe");
        let twin = root.join("mine.exe");
        fs::write(&file, b"same object").unwrap();
        fs::hard_link(&file, &twin).unwrap();
        let error = FileIdentity::of(&file).unwrap_err().to_string();
        assert!(error.contains("hard link"), "{error}");
        assert_eq!(fs::read(&twin).unwrap(), b"same object");
        fs::remove_dir_all(root).unwrap();
    }

    /// RED (T-UNINSTALL-UX round 2, mutation `drop_copy_guard`) — while the
    /// scheduler holds the copy through the readiness hand-off, a replacement
    /// attempt is refused. The barrier is the attempt's handshake; the timeout
    /// is only an outer deadlock cap.
    #[cfg(windows)]
    #[test]
    fn the_private_remover_cannot_be_replaced_before_readiness() {
        let root = sandbox("guard");
        let copy = root.join(COPY_NAME_WINDOWS);
        fs::write(&copy, b"remover").unwrap();
        let guard = guard_copy(&copy).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let entered = Arc::clone(&barrier);
        let target = copy.clone();
        let attempt = crate::spawn_at_priority(
            "folio-remover-replacement-test",
            crate::ThreadPriority::BelowNormal,
            move |_| {
                entered.wait();
                let replacement = target.with_extension("replacement");
                fs::write(&replacement, b"attacker").unwrap();
                let answer =
                    fs::remove_file(&target).and_then(|()| fs::rename(&replacement, &target));
                let _ = fs::remove_file(replacement);
                answer
            },
        )
        .unwrap();
        barrier.wait();
        let answer = attempt.join().unwrap();
        assert!(
            answer.is_err(),
            "the held executable name cannot be replaced"
        );
        drop(guard);
        fs::remove_dir_all(root).unwrap();
    }

    /// RED (T-UNINSTALL-UX round 2, mutation `one_shot_held_delete`) — a
    /// Windows image/file held without delete sharing is left for the bounded
    /// retry loop; it is deleted only after that handle closes.
    #[cfg(windows)]
    #[test]
    fn a_held_folio_file_is_not_reported_removed() {
        use std::os::windows::fs::OpenOptionsExt;

        let root = sandbox("held-image");
        let file = root.join("folio.exe");
        fs::write(&file, b"owned image").unwrap();
        let expected = identity(&file);
        let held = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&file)
            .unwrap();
        let plan = removal(
            &file,
            Item::File {
                path: file.clone(),
                expected: expected.clone(),
            },
            None,
        );
        let outcome =
            on_worker(move |worker| perform_with(worker, &plan, Duration::ZERO, 1, Duration::ZERO));
        assert_eq!(outcome, Outcome::Left(vec![file.clone()]));
        assert!(file.exists());
        drop(held);
        assert!(remove_verified_file(&file, &expected).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    /// RED (T-UNINSTALL-UX round 2, mutation `trust_planned_identity`) — the
    /// destructive boundary checks both length and digest, not either alone.
    #[test]
    fn equal_length_changed_bytes_are_left() {
        let root = sandbox("equal-length");
        let file = root.join("conpty.dll");
        fs::write(&file, b"owned").unwrap();
        let expected = identity(&file);
        fs::write(&file, b"mine!").unwrap();
        let item = Item::File {
            path: file.clone(),
            expected,
        };
        assert!(!remove_item(&item).unwrap());
        assert_eq!(fs::read(&file).unwrap(), b"mine!");
        fs::remove_dir_all(root).unwrap();
    }

    /// RED (T-UNINSTALL-UX round 2, mutation `restore_recursive_directory_delete`) —
    /// a file inserted into a planned tree is not in the byte-identity plan,
    /// so the directory remains non-empty and the personal file survives.
    #[test]
    fn a_post_plan_file_keeps_its_directory() {
        let root = sandbox("late-tree-file");
        let tree = root.join("Folio.app");
        fs::create_dir(&tree).unwrap();
        let personal = tree.join("mine.txt");
        fs::write(&personal, b"mine").unwrap();
        assert!(remove_item(&Item::Directory(tree.clone())).is_err());
        assert_eq!(fs::read(&personal).unwrap(), b"mine");
        fs::remove_dir_all(root).unwrap();
    }

    /// RED (T-UNINSTALL-UX round 2, mutation `wait_by_pid_only`) — a reused
    /// pid/start pair is not waited for. `still_running` is the one identity
    /// authority; changing the recorded instant makes it false.
    #[test]
    fn a_pid_with_another_start_instant_is_not_the_waited_process() {
        let pid = std::process::id();
        let started = crate::install_flip::started_of(pid).unwrap();
        assert!(crate::install_flip::still_running(Running { pid, started }));
        assert!(!crate::install_flip::still_running(Running {
            pid,
            started: started.wrapping_add(1),
        }));
    }

    /// RED (T-UNINSTALL-UX round 2, mutation `omit_result_path_from_failure`) —
    /// a final failure names the live process or remaining path and the result
    /// file where the same answer can be found.
    #[test]
    fn a_final_failure_names_what_remains_and_the_result_file() {
        let result = Path::new("private/result.txt");
        let running = failure_text(
            &words(),
            result,
            Outcome::TimedOut(vec![OsString::from("folio.exe")]),
        )
        .unwrap();
        assert!(running.contains("Still running:\nfolio.exe"), "{running}");
        assert!(
            running.contains(&format!("Details were saved to:\n{}", result.display())),
            "{running}"
        );

        let path = PathBuf::from("install/folio.exe");
        let left = failure_text(&words(), result, Outcome::Left(vec![path.clone()])).unwrap();
        assert!(left.contains("Folio could not remove:"), "{left}");
        assert!(left.contains(&path.display().to_string()), "{left}");
        assert!(left.contains(&result.display().to_string()), "{left}");
    }

    /// RED (T-UNINSTALL-UX round 2, mutation `do_not_release_ready_remover`) —
    /// the test helper's native copy uses the same product handshake and then
    /// removes the sandbox file and the empty folder. The only clock is the
    /// outer deadlock ceiling around observing the already-produced result.
    #[test]
    fn a_native_remover_copy_runs_the_plan() {
        if std::env::var_os(key("PRIVATE")).is_some() {
            crate::admission::enter_standalone_main("folio-uninstall-remove-test", |worker| {
                assert_eq!(run_from_environment(worker), 0);
            })
            .unwrap();
            return;
        }
        if cfg!(not(any(windows, unix))) {
            return;
        }
        let root = sandbox("native");
        let install = root.join("install");
        let private = root.join("local/Folio");
        fs::create_dir_all(&install).unwrap();
        let file = install.join("sidecar.bin");
        fs::write(&file, b"owned").unwrap();
        let program = root.join("source-test-program.exe");
        fs::copy(std::env::current_exe().unwrap(), &program).unwrap();
        let plan = removal(
            &program,
            Item::File {
                path: file.clone(),
                expected: identity(&file),
            },
            Some(install.clone()),
        );
        let private_after = private.clone();
        on_worker(move |worker| schedule(worker, &plan, &private)).unwrap();
        let until = Instant::now() + Duration::from_secs(30);
        while (install.exists() || private_after.exists()) && Instant::now() < until {
            std::thread::yield_now();
        }
        assert!(!install.exists(), "the native remover completed its plan");
        assert!(
            !private_after.exists(),
            "the native remover removed its private copy last"
        );
        let _ = fs::remove_dir_all(root);
    }
}
