//! **What a download is, apart from the stack that carries it** (U-7,
//! `docs/plans/design/self-update-2026-09-16.md` §E and revision (b)).
//!
//! [`crate::http::https_download`] has three arms, like `https_get` beside it:
//! WinHTTP on Windows, `NSURLSession` on macOS, and a refusal on a build with
//! neither. Everything a download promises that is *not* about the stack lives
//! here, once, so the two real arms cannot come to disagree about it:
//!
//! * **the ceiling** — a byte count known before the first byte, from the offer
//!   ([`HttpsDownload::ceiling`]), never above [`DOWNLOAD_CEILING_LIMIT`]. A
//!   `Content-Length` over it is refused before the body is read
//!   ([`admit_length`]); a body with no length, or one that lies, is refused the
//!   moment the count would pass it, **before** the chunk that passes it is
//!   written ([`Partial::accept`]);
//! * **the file** — the body goes to `<file_name>.partial` under the caller's
//!   directory and is renamed to `<file_name>` only when it ended where the
//!   transport said it would ([`Partial::finish`]). A failure at any stage
//!   removes the temporary file, so **a partial body never stands under the
//!   final name** and nothing of it survives the call;
//! * **the deadlines** — [`DOWNLOAD_IDLE_TIMEOUT`] of silence and
//!   [`DOWNLOAD_BUDGET`] for the whole call, both on the monotonic clock, so a
//!   body that trickles one byte at a time still ends at the budget
//!   ([`Deadlines`]);
//! * **the vocabulary** — every failure names its stage ([`DownloadStage`]:
//!   connect, status, headers, body, rename) and a reason that never quotes the
//!   body;
//! * **progress and cancellation** — [`DownloadMonitor`], shared with whoever
//!   draws the card.
//!
//! # What it deliberately is not
//!
//! **No retries and no resumption.** One call is one `GET`; a failure deletes
//! what arrived and says why, and the next attempt is the caller's decision and
//! starts from zero. Revision (b) puts the recovery of an interrupted update in
//! the transaction journal (W1/M1: a partial download is deleted, never
//! continued), and a `Range` request would be a second request shape through a
//! door whose rule is one.
//!
//! **No integrity check.** The digest is not computed here: the file this
//! writes is checked against the release's checksum document by Prepare (U-20
//! on Windows, U-27 on macOS), which is where the design puts the hash. This
//! door promises only that the file is the whole body the server sent, within
//! the ceiling. A body with no `Content-Length` that the server ends by closing
//! the connection early cannot be told from a whole one by any transport; the
//! digest is what catches it.
//!
//! **Not on the window thread.** A download blocks its caller for as long as
//! the transfer takes — up to [`DOWNLOAD_BUDGET`]. It runs on the update job's
//! own worker (U-18). Nothing here can tell which thread called it today; the
//! thread door's `WorkerCtx` (A1b) and its source prohibitions (A1e) are what
//! will make "a worker only" a type rather than this sentence.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// How long a download may go without a byte, a header or a completed phase
/// before it is abandoned (§E: "30 s idle").
pub const DOWNLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// How long one download may take, end to end, however steadily bytes arrive
/// (§E: "10 min end-to-end").
pub const DOWNLOAD_BUDGET: Duration = Duration::from_secs(10 * 60);

/// The most any download may be, whatever the offer says (§E: "against a
/// 200 MB cap"). A release archive is about a tenth of it; the number is a
/// bound on what a wrong or hostile offer can make this machine write, not an
/// estimate of an asset.
pub const DOWNLOAD_CEILING_LIMIT: u64 = 200 * 1024 * 1024;

/// The fixed read buffer both real arms read into, and so the most one chunk
/// can be. The file sink never holds more than one chunk in memory.
pub const DOWNLOAD_CHUNK_BYTES: usize = 64 * 1024;

/// How often a waiting download looks at its cancel flag and its deadlines —
/// the stated latency of [`DownloadMonitor::cancel`] in every phase in which
/// the waiting thread is this module's (connect, headers, body).
pub const DOWNLOAD_CANCEL_LATENCY: Duration = Duration::from_millis(50);

/// The one download this door knows how to make.
///
/// `host`, `path` and `user_agent` are [`crate::http::HttpsGet`]'s, with the
/// same meaning and the same discipline: one `GET` of `https://{host}{path}`,
/// no header the caller can name, the platform's own proxy and trust store.
#[derive(Clone, Copy, Debug)]
pub struct HttpsDownload<'a> {
    /// The host, with no scheme and no slash.
    pub host: &'a str,
    /// The path with its leading slash, query string included.
    pub path: &'a str,
    /// The `User-Agent` this request travels under.
    pub user_agent: &'a str,
    /// The directory the file is written in. It exists; the caller made it.
    pub directory: &'a Path,
    /// The file's name inside [`Self::directory`]: one path component, chosen
    /// by the caller. The body is written to `<file_name>.partial` beside it
    /// first.
    pub file_name: &'a str,
    /// The most body this will write — the offer's size of the asset. Taken as
    /// no more than [`DOWNLOAD_CEILING_LIMIT`].
    pub ceiling: u64,
    /// [`DOWNLOAD_IDLE_TIMEOUT`] in the product.
    pub idle_timeout: Duration,
    /// [`DOWNLOAD_BUDGET`] in the product.
    pub budget: Duration,
    /// Progress out, cancellation in.
    pub monitor: &'a Arc<DownloadMonitor>,
}

impl HttpsDownload<'_> {
    /// The ceiling this download is held to: the offer's, never above the
    /// door's own limit.
    #[must_use]
    pub fn effective_ceiling(&self) -> u64 {
        self.ceiling.min(DOWNLOAD_CEILING_LIMIT)
    }
}

/// A file that arrived whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Downloaded {
    /// `directory/file_name`.
    pub path: PathBuf,
    /// How many bytes it holds.
    pub bytes: u64,
}

/// Where a download was when it failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadStage {
    /// Before a response: name resolution, the connection, TLS, the request.
    Connect,
    /// A response arrived and its status was not `200`.
    Status,
    /// The response's headers: waiting for them, reading them, or a
    /// `Content-Length` over the ceiling.
    Headers,
    /// The body: reading it, counting it, writing it, flushing it, and whether
    /// it ended where the transport said it would.
    Body,
    /// Moving the finished file to its name.
    Rename,
}

impl DownloadStage {
    /// The stage's one word, as a log line says it.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Connect => "connect",
            Self::Status => "status",
            Self::Headers => "headers",
            Self::Body => "body",
            Self::Rename => "rename",
        }
    }
}

/// Why a download did not produce a file.
///
/// `reason` is a sentence for a log. It names numbers and the stack's own
/// error, never bytes of the body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadError {
    /// Where it stopped.
    pub stage: DownloadStage,
    /// Whether it stopped because [`DownloadMonitor::cancel`] asked it to.
    pub cancelled: bool,
    /// What happened, as a sentence.
    pub reason: String,
}

impl DownloadError {
    /// A failure at `stage`.
    #[must_use]
    pub fn at(stage: DownloadStage, reason: impl Into<String>) -> Self {
        Self {
            stage,
            cancelled: false,
            reason: reason.into(),
        }
    }

    /// The caller's own cancellation, observed at `stage`.
    #[must_use]
    pub fn cancelled_at(stage: DownloadStage) -> Self {
        Self {
            stage,
            cancelled: true,
            reason: "the download was cancelled".to_owned(),
        }
    }
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.stage.word(), self.reason)
    }
}

/// How far a download has got, as the card reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownloadProgress {
    /// Bytes written so far.
    pub received: u64,
    /// What the server announced, when it announced a length within the
    /// ceiling. `None` draws an indeterminate bar: an absent length is never a
    /// number, and macOS's negative "unknown" sentinel never becomes a huge
    /// one.
    pub expected: Option<u64>,
}

/// The value `expected` holds while no length is known.
const UNKNOWN: u64 = u64::MAX;

/// **Progress out, cancellation in, shared by the download and whoever draws
/// it.**
///
/// Progress carries **at most one pending wake** (§E): the download calls the
/// wake only when it raises the pending flag from clear, and the reader clears
/// it by [`Self::take`]. However many chunks arrive while the window thread is
/// busy, it is owed one wake, and the numbers it then reads are the latest —
/// so a stalled reader cannot make the stream queue anything, and the stream
/// never waits on the reader.
pub struct DownloadMonitor {
    received: AtomicU64,
    expected: AtomicU64,
    pending: AtomicBool,
    cancelled: AtomicBool,
    wake: Box<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for DownloadMonitor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DownloadMonitor")
            .field("progress", &self.peek())
            .field("cancelled", &self.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl DownloadMonitor {
    /// A monitor whose `wake` is called when progress becomes pending.
    ///
    /// `wake` runs on the download's thread (on macOS, the session's delegate
    /// queue). It must not block: posting a user event is the shape it is
    /// written for.
    #[must_use]
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            received: AtomicU64::new(0),
            expected: AtomicU64::new(UNKNOWN),
            pending: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            wake: Box::new(wake),
        }
    }

    /// Ask the download to stop. It stops within [`DOWNLOAD_CANCEL_LATENCY`]
    /// of a wait, deletes what it wrote, and answers
    /// [`DownloadError::cancelled`].
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Whether [`Self::cancel`] has been called.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Read the progress and clear the pending wake, so the next chunk wakes
    /// the reader again.
    ///
    /// The flag is cleared **before** the numbers are read: a chunk that lands
    /// between the two raises it again and is owed its own wake, rather than
    /// being read now and then woken for with nothing new.
    pub fn take(&self) -> DownloadProgress {
        self.pending.store(false, Ordering::SeqCst);
        self.peek()
    }

    /// The progress, without touching the wake.
    #[must_use]
    pub fn peek(&self) -> DownloadProgress {
        let expected = self.expected.load(Ordering::SeqCst);
        DownloadProgress {
            received: self.received.load(Ordering::SeqCst),
            expected: (expected != UNKNOWN).then_some(expected),
        }
    }

    /// A new download started on this monitor: its length, and nothing
    /// received yet.
    fn begin(&self, expected: Option<u64>) {
        self.expected
            .store(expected.unwrap_or(UNKNOWN), Ordering::SeqCst);
        self.report(0);
    }

    /// `received` bytes are written. Wakes the reader only if it is not
    /// already owed a wake.
    fn report(&self, received: u64) {
        self.received.store(received, Ordering::SeqCst);
        if !self.pending.swap(true, Ordering::SeqCst) {
            (self.wake)();
        }
    }
}

/// **A `Content-Length` over the ceiling is refused before the body.**
///
/// `announced` is `None` when the response carries no length (chunked, or
/// ended by closing); such a body is bounded by [`Partial::accept`] instead.
///
/// # Errors
///
/// A [`DownloadStage::Headers`] failure when `announced` passes `ceiling`.
pub fn admit_length(announced: Option<u64>, ceiling: u64) -> Result<(), DownloadError> {
    match announced {
        Some(length) if length > ceiling => Err(DownloadError::at(
            DownloadStage::Headers,
            format!("the server announced {length} bytes, more than the ceiling of {ceiling}"),
        )),
        _ => Ok(()),
    }
}

/// **The two deadlines, on the monotonic clock.**
///
/// `budget` runs from the start and nothing moves it; `idle` runs from the last
/// thing the transport reported and every report moves it. So a trickle defeats
/// the idle timeout and still meets the budget.
#[derive(Clone, Copy, Debug)]
pub struct Deadlines {
    started: Instant,
    last_heard: Instant,
    idle: Duration,
    budget: Duration,
}

impl Deadlines {
    /// Both clocks start now.
    #[must_use]
    pub fn start(idle: Duration, budget: Duration) -> Self {
        let now = Instant::now();
        Self {
            started: now,
            last_heard: now,
            idle,
            budget,
        }
    }

    /// The transport reported something.
    pub fn heard(&mut self) {
        self.last_heard = Instant::now();
    }

    /// Whether the download may keep waiting at `stage`.
    ///
    /// # Errors
    ///
    /// The cancellation, the budget or the idle timeout, in that order, named
    /// at `stage`.
    pub fn check(
        &self,
        stage: DownloadStage,
        monitor: &DownloadMonitor,
    ) -> Result<(), DownloadError> {
        if monitor.is_cancelled() {
            return Err(DownloadError::cancelled_at(stage));
        }
        if self.started.elapsed() >= self.budget {
            return Err(DownloadError::at(
                stage,
                format!(
                    "the download did not finish inside its budget of {} s",
                    self.budget.as_secs_f64()
                ),
            ));
        }
        if self.last_heard.elapsed() >= self.idle {
            return Err(DownloadError::at(
                stage,
                format!("nothing arrived for {} s", self.idle.as_secs_f64()),
            ));
        }
        Ok(())
    }

    /// How long the next wait may block before [`Self::check`] must run again.
    #[must_use]
    pub fn slice(&self) -> Duration {
        let to_budget = self.budget.saturating_sub(self.started.elapsed());
        let to_idle = self.idle.saturating_sub(self.last_heard.elapsed());
        DOWNLOAD_CANCEL_LATENCY
            .min(to_budget)
            .min(to_idle)
            .max(Duration::from_millis(1))
    }
}

/// Where the body's bytes go. [`File`] in the product; a fault-injecting
/// wrapper in the tests, which is how short writes, a full disk and a failed
/// flush are made to happen.
pub trait Store {
    /// Write all of `bytes`, or fail.
    ///
    /// # Errors
    ///
    /// Whatever the file system answered, including a short write.
    fn put(&mut self, bytes: &[u8]) -> io::Result<()>;

    /// Flush the bytes to the device.
    ///
    /// # Errors
    ///
    /// Whatever the file system answered.
    fn seal(&mut self) -> io::Result<()>;
}

impl Store for File {
    fn put(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.write_all(bytes)
    }

    fn seal(&mut self) -> io::Result<()> {
        self.flush()?;
        self.sync_all()
    }
}

/// **The body on its way to a file: the temporary name, the count and the
/// ceiling.**
///
/// Dropping one that has not [finished](Self::finish) closes and removes the
/// temporary file — the one cleanup every failure path shares, including the
/// ones a transport reaches by returning early.
pub struct Partial<S: Store = File> {
    store: Option<S>,
    temporary: PathBuf,
    destination: PathBuf,
    ceiling: u64,
    written: u64,
    monitor: Arc<DownloadMonitor>,
}

/// The temporary name beside `file_name`.
fn temporary_name(file_name: &str) -> String {
    format!("{file_name}.partial")
}

/// **Where a download's bytes go, owned** — what a [`Partial`] is made from.
///
/// The request borrows its strings for the length of the call; the macOS arm
/// creates its partial on the session's delegate queue, which may outlive a
/// borrow, so the parts a partial needs are copied out once.
#[derive(Clone, Debug)]
pub struct Destination {
    directory: PathBuf,
    file_name: String,
    ceiling: u64,
    monitor: Arc<DownloadMonitor>,
}

impl Destination {
    /// The request's directory, name, effective ceiling and monitor.
    #[must_use]
    pub fn of(request: &HttpsDownload<'_>) -> Self {
        Self {
            directory: request.directory.to_path_buf(),
            file_name: request.file_name.to_owned(),
            ceiling: request.effective_ceiling(),
            monitor: Arc::clone(request.monitor),
        }
    }

    /// `directory/<file_name>.partial`.
    #[must_use]
    pub fn temporary(&self) -> PathBuf {
        self.directory.join(temporary_name(&self.file_name))
    }

    /// The ceiling the body is held to.
    #[must_use]
    pub fn ceiling(&self) -> u64 {
        self.ceiling
    }

    /// The monitor progress goes to and cancellation comes from.
    #[must_use]
    pub fn monitor(&self) -> &DownloadMonitor {
        &self.monitor
    }
}

impl Partial<File> {
    /// Create `<file_name>.partial` under the request's directory, after the
    /// response was admitted.
    ///
    /// A `.partial` left by an earlier process that died mid-download is
    /// removed first and the file is then created exclusively, so this never
    /// appends to, or writes through, something it did not make.
    ///
    /// # Errors
    ///
    /// A [`DownloadStage::Body`] failure when the file cannot be created.
    pub fn create(destination: &Destination, expected: Option<u64>) -> Result<Self, DownloadError> {
        let temporary = destination.temporary();
        // A leftover is the only thing that can stand here; if it cannot be
        // removed, the exclusive create below fails and says so.
        let _ = fs::remove_file(&temporary);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| {
                DownloadError::at(
                    DownloadStage::Body,
                    format!("the temporary file could not be created: {error}"),
                )
            })?;
        Ok(Self::over(file, destination, expected))
    }
}

impl<S: Store> Partial<S> {
    /// A partial writing into `store`, which already stands at
    /// [`Destination::temporary`].
    pub fn over(store: S, destination: &Destination, expected: Option<u64>) -> Self {
        destination.monitor.begin(expected);
        Self {
            store: Some(store),
            temporary: destination.temporary(),
            destination: destination.directory.join(&destination.file_name),
            ceiling: destination.ceiling,
            written: 0,
            monitor: Arc::clone(&destination.monitor),
        }
    }

    /// How many bytes are written.
    #[must_use]
    pub fn written(&self) -> u64 {
        self.written
    }

    /// **One chunk, counted against the ceiling before it is written.**
    ///
    /// # Errors
    ///
    /// A [`DownloadStage::Body`] failure when the chunk would pass the ceiling
    /// (nothing of it is written) or the write fails.
    pub fn accept(&mut self, chunk: &[u8]) -> Result<(), DownloadError> {
        let length = u64::try_from(chunk.len()).unwrap_or(u64::MAX);
        let after = self.written.saturating_add(length);
        if after > self.ceiling {
            return Err(DownloadError::at(
                DownloadStage::Body,
                format!("the body passed the ceiling of {} bytes", self.ceiling),
            ));
        }
        let store = self
            .store
            .as_mut()
            .expect("a partial is written only until it finishes");
        store.put(chunk).map_err(|error| {
            DownloadError::at(
                DownloadStage::Body,
                format!(
                    "writing the body failed after {} bytes: {error}",
                    self.written
                ),
            )
        })?;
        self.written = after;
        self.monitor.report(after);
        Ok(())
    }

    /// **The transport says the body is over: check it ended where it said it
    /// would, flush it, and give it its name.**
    ///
    /// `expected` is the length the response announced, if it announced one.
    ///
    /// # Errors
    ///
    /// [`DownloadStage::Body`] for a body shorter than announced or a failed
    /// flush; [`DownloadStage::Rename`] when the file cannot take its name. In
    /// every case the temporary file is removed and the final name is not
    /// written.
    pub fn finish(mut self, expected: Option<u64>) -> Result<Downloaded, DownloadError> {
        if let Some(expected) = expected
            && self.written != expected
        {
            return Err(DownloadError::at(
                DownloadStage::Body,
                format!(
                    "the body ended after {} of the {expected} bytes the server announced",
                    self.written
                ),
            ));
        }
        let mut store = self.store.take().expect("a partial finishes once");
        store.seal().map_err(|error| {
            DownloadError::at(
                DownloadStage::Body,
                format!("flushing the body failed: {error}"),
            )
        })?;
        // Closed before the rename: Windows will not rename a file this process
        // still has open for writing.
        drop(store);
        fs::rename(&self.temporary, &self.destination).map_err(|error| {
            DownloadError::at(
                DownloadStage::Rename,
                format!("the finished file could not take its name: {error}"),
            )
        })?;
        Ok(Downloaded {
            path: self.destination.clone(),
            bytes: self.written,
        })
    }
}

impl<S: Store> Drop for Partial<S> {
    fn drop(&mut self) {
        if let Some(store) = self.store.take() {
            drop(store);
        }
        // After a successful rename there is nothing at this name, and after
        // a failure there is nobody to tell: the error that got us here is the
        // one the caller reads.
        let _ = fs::remove_file(&self.temporary);
    }
}

/// **A server on this machine's own loopback, scripted connection by
/// connection** — the fake transport the two real arms' download tests talk
/// to (§F: "over a local fake transport"; the brief: tests never leave
/// loopback).
///
/// Neither door can be pointed at it (each composes `https://{host}` on port
/// 443), so each arm splits its exchange at the seam its `https_get` already
/// has — *where to send it* and *sending it* — and its tests drive the second
/// half at `http://127.0.0.1:{port}`.
#[cfg(test)]
pub(crate) mod loopback {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        time::{Duration, Instant},
    };

    /// One thing the server does on a connection, in order.
    pub(crate) enum Step {
        /// Write these bytes.
        Send(Vec<u8>),
        /// Write one byte every `.0`, for at most `.1` or until the client goes
        /// away — bounded, so a download whose budget does not hold ends in a
        /// failed assertion rather than a hung test.
        Trickle(Duration, Duration),
        /// Say nothing for this long.
        Pause(Duration),
    }

    /// How long a server waits for connections it was told to expect before
    /// its thread ends, so a test that fails early leaves no listener behind
    /// for long.
    const LISTEN_FOR: Duration = Duration::from_secs(60);

    /// Serve up to `connections` connections, each answered by `script` given
    /// the request line's path. Returns the port.
    pub(crate) fn serve(
        connections: usize,
        script: impl Fn(&str) -> Vec<Step> + Send + 'static,
    ) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener.local_addr().expect("the chosen port").port();
        listener
            .set_nonblocking(true)
            .expect("a listener that can be polled");
        std::thread::spawn(move || {
            let until = Instant::now() + LISTEN_FOR;
            let mut served = 0;
            while served < connections && Instant::now() < until {
                let Ok((mut connection, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                served += 1;
                let _ = connection.set_nonblocking(false);
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if connection.read(&mut byte).unwrap_or(0) != 1 {
                        break;
                    }
                    head.push(byte[0]);
                }
                let line = String::from_utf8_lossy(&head);
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
                for step in script(&path) {
                    let gone = match step {
                        Step::Send(bytes) => connection
                            .write_all(&bytes)
                            .and_then(|()| connection.flush())
                            .is_err(),
                        Step::Pause(pause) => {
                            std::thread::sleep(pause);
                            false
                        }
                        Step::Trickle(every, most) => {
                            let until = Instant::now() + most;
                            loop {
                                if Instant::now() >= until
                                    || connection
                                        .write_all(b"x")
                                        .and_then(|()| connection.flush())
                                        .is_err()
                                {
                                    break true;
                                }
                                std::thread::sleep(every);
                            }
                        }
                    };
                    if gone {
                        break;
                    }
                }
                // The connection closes here, which ends a body that carried no
                // length and cuts one that carried a longer one.
            }
        });
        port
    }

    /// A status line, `Connection: close`, and the headers given.
    pub(crate) fn head(status: u16, reason: &str, headers: &[String]) -> Vec<u8> {
        let mut out = format!("HTTP/1.1 {status} {reason}\r\nConnection: close\r\n");
        for header in headers {
            out.push_str(header);
            out.push_str("\r\n");
        }
        out.push_str("\r\n");
        out.into_bytes()
    }

    /// **A `200` that has begun**: its head, typed as a release asset is
    /// (`application/octet-stream`), and the first KiB of its body, with the
    /// connection left open by the steps that follow.
    ///
    /// The KiB is there for the macOS arm: `NSURLSession` does not hand a
    /// response to its delegate until the first bytes of the body have
    /// arrived — against this server a head followed by silence reached
    /// `didReceiveResponse:` only after the idle timeout (observed on macOS 26,
    /// 2026-09-26), where a real asset's body follows its head at once.
    pub(crate) fn begun(headers: &[String]) -> Vec<u8> {
        let mut all = vec!["Content-Type: application/octet-stream".to_owned()];
        all.extend_from_slice(headers);
        let mut out = head(200, "OK", &all);
        out.extend_from_slice(&body(1_024));
        out
    }

    /// A `200` carrying `body`, with its length announced when `announce`.
    pub(crate) fn ok(body: &[u8], announce: Option<usize>) -> Vec<u8> {
        let headers: Vec<String> = announce
            .map(|length| vec![format!("Content-Length: {length}")])
            .unwrap_or_default();
        let mut out = head(200, "OK", &headers);
        out.extend_from_slice(body);
        out
    }

    /// Bytes that are not text and not a repetition, so a dropped, doubled or
    /// reordered chunk changes them.
    pub(crate) fn body(length: usize) -> Vec<u8> {
        (0..length)
            .map(|at| u8::try_from((at * 31 + at / 251) % 256).unwrap_or(0))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        path::{Path, PathBuf},
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use super::{
        DOWNLOAD_CEILING_LIMIT, Destination, DownloadMonitor, DownloadStage, HttpsDownload,
        Partial, Store, admit_length,
    };

    /// A fresh directory under the system's temporary folder, named for the
    /// test, empty.
    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir()
            .join("bt-platform-https-download")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        directory
    }

    fn request<'a>(
        directory: &'a Path,
        ceiling: u64,
        monitor: &'a Arc<DownloadMonitor>,
    ) -> HttpsDownload<'a> {
        HttpsDownload {
            host: "example.invalid",
            path: "/asset.zip",
            user_agent: "Folio",
            directory,
            file_name: "asset.zip",
            ceiling,
            idle_timeout: Duration::from_secs(5),
            budget: Duration::from_secs(15),
            monitor,
        }
    }

    fn quiet() -> Arc<DownloadMonitor> {
        Arc::new(DownloadMonitor::new(|| {}))
    }

    /// Only the directory's own entries, sorted.
    fn entries(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(directory)
            .expect("the scratch directory")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    /// A store that fails on command: after `accept` bytes, or at the flush.
    struct Faulty {
        file: std::fs::File,
        accept: usize,
        short: bool,
        fail_seal: bool,
    }

    impl Store for Faulty {
        fn put(&mut self, bytes: &[u8]) -> io::Result<()> {
            if bytes.len() > self.accept {
                // A short write that the file system reports as such, and the
                // disk-full it becomes when `write_all` asks for the rest.
                if self.short {
                    let taken = self.accept;
                    std::io::Write::write_all(&mut self.file, &bytes[..taken])?;
                    self.accept = 0;
                    return Err(io::Error::new(io::ErrorKind::WriteZero, "wrote zero bytes"));
                }
                return Err(io::Error::new(
                    io::ErrorKind::StorageFull,
                    "no space left on device",
                ));
            }
            self.accept -= bytes.len();
            std::io::Write::write_all(&mut self.file, bytes)
        }

        fn seal(&mut self) -> io::Result<()> {
            if self.fail_seal {
                return Err(io::Error::other("the device refused the flush"));
            }
            self.file.sync_all()
        }
    }

    fn faulty(destination: &Destination, accept: usize, short: bool, fail_seal: bool) -> Faulty {
        let file = std::fs::File::create(destination.temporary()).expect("the temporary file");
        Faulty {
            file,
            accept,
            short,
            fail_seal,
        }
    }

    /// RED (U-7) — **a short write, a full disk and a failed flush each end in
    /// a failure, never in a file under the final name.**
    ///
    /// §F Transport: `short_write_disk_full_and_flush_failure_never_verify`.
    /// A file that is short because the disk filled is exactly the file a later
    /// hash would catch — but "later" is not this door's promise: a download
    /// that did not write every byte it read is a failure *here*, at the body
    /// stage, and leaves nothing behind.
    ///
    /// MUTATION: map `put`'s error to `Ok(())` in `Partial::accept`, or ignore
    /// `seal`'s result in `Partial::finish`, and a file appears under
    /// `asset.zip`.
    #[test]
    fn short_write_disk_full_and_flush_failure_never_verify() {
        let body = super::loopback::body(4_096);
        for (case, accept, short, fail_seal) in [
            ("short", 1_000, true, false),
            ("full", 1_000, false, false),
            ("flush", usize::MAX, false, true),
        ] {
            let directory = scratch(&format!("fault-{case}"));
            let monitor = quiet();
            let asked = request(&directory, 10_000, &monitor);
            let destination = Destination::of(&asked);
            let store = faulty(&destination, accept, short, fail_seal);
            let mut partial = Partial::over(store, &destination, Some(4_096));
            let outcome = body
                .chunks(512)
                .try_for_each(|chunk| partial.accept(chunk))
                .and_then(|()| partial.finish(Some(4_096)).map(|_| ()));
            let error = outcome.expect_err(case);
            assert_eq!(error.stage, DownloadStage::Body, "{case}: {error}");
            assert!(!error.cancelled, "{case}");
            assert!(
                entries(&directory).is_empty(),
                "{case}: nothing is left behind: {:?}",
                entries(&directory)
            );
        }
    }

    /// RED (U-7) — **however many chunks arrive while nobody reads, the reader
    /// is owed one wake, and reading makes the next chunk wake it again.**
    ///
    /// §F Transport: `progress_queue_has_one_pending_wake`. The wake posts to
    /// the window thread; a download that woke it per chunk would queue
    /// thousands of events behind a busy frame, and one that waited for the
    /// reader would let a stalled window stall the stream.
    ///
    /// MUTATION: call the wake on every `report` (drop the `swap` test), and
    /// the count is 64.
    #[test]
    fn progress_queue_has_one_pending_wake() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&wakes);
        let monitor = Arc::new(DownloadMonitor::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        let directory = scratch("wake");
        let asked = request(&directory, 1 << 20, &monitor);
        let mut partial =
            Partial::create(&Destination::of(&asked), None).expect("the temporary file");
        for chunk in super::loopback::body(64 * 100).chunks(100) {
            partial.accept(chunk).expect("under the ceiling");
        }
        assert_eq!(wakes.load(Ordering::SeqCst), 1, "one pending wake, not 64");
        let seen = monitor.take();
        assert_eq!(seen.received, 6_400);
        assert_eq!(seen.expected, None, "no length was announced");
        partial.accept(b"more").expect("under the ceiling");
        assert_eq!(
            wakes.load(Ordering::SeqCst),
            2,
            "a read makes the next chunk wake"
        );
        drop(partial);
        assert!(
            entries(&directory).is_empty(),
            "an unfinished partial is removed"
        );
    }

    /// RED (U-7) — **the ceiling is counted before a chunk is written: a body
    /// exactly at it is whole, one byte over is refused and leaves nothing.**
    ///
    /// The pure half of `unknown_length_stream_is_bounded`: no length was
    /// announced, so the count is the only bound, and the chunk that would pass
    /// it is refused whole rather than written and regretted.
    ///
    /// MUTATION: compare `self.written > self.ceiling` after the write instead
    /// of `after > self.ceiling` before it, and the over-case file holds the
    /// byte.
    #[test]
    fn the_ceiling_is_counted_before_each_write() {
        let directory = scratch("ceiling");
        let monitor = quiet();
        let asked = request(&directory, 1_000, &monitor);
        let mut partial =
            Partial::create(&Destination::of(&asked), None).expect("the temporary file");
        partial.accept(&[7; 1_000]).expect("exactly at the ceiling");
        let error = partial.accept(&[7; 1]).expect_err("one byte over");
        assert_eq!(error.stage, DownloadStage::Body);
        assert_eq!(error.reason, "the body passed the ceiling of 1000 bytes");
        assert_eq!(
            partial.written(),
            1_000,
            "the refused chunk was not written"
        );
        drop(partial);
        assert!(entries(&directory).is_empty());

        let mut partial =
            Partial::create(&Destination::of(&asked), None).expect("the temporary file");
        partial.accept(&[7; 1_000]).expect("exactly at the ceiling");
        let done = partial
            .finish(None)
            .expect("a body at the ceiling is whole");
        assert_eq!(done.bytes, 1_000);
        assert_eq!(entries(&directory), vec!["asset.zip".to_owned()]);
    }

    /// RED (U-7) — **a `Content-Length` over the ceiling is refused at the
    /// headers, and no offer can lift the ceiling past the door's limit.**
    ///
    /// MUTATION: return `Ok(())` from `admit_length`, or drop the `min` in
    /// `effective_ceiling`.
    #[test]
    fn an_announced_length_over_the_ceiling_is_refused_at_the_headers() {
        assert!(admit_length(Some(1_000), 1_000).is_ok());
        assert!(admit_length(None, 1_000).is_ok());
        let error = admit_length(Some(1_001), 1_000).expect_err("over");
        assert_eq!(error.stage, DownloadStage::Headers);
        let monitor = quiet();
        let directory = scratch("limit");
        assert_eq!(
            request(&directory, u64::MAX, &monitor).effective_ceiling(),
            DOWNLOAD_CEILING_LIMIT
        );
    }

    /// RED (U-7) — **a body that ends short of its announced length is a body
    /// failure, and the temporary file goes with it.**
    ///
    /// MUTATION: drop the length comparison in `Partial::finish`.
    #[test]
    fn a_body_shorter_than_announced_never_takes_the_name() {
        let directory = scratch("short-body");
        let monitor = quiet();
        let asked = request(&directory, 10_000, &monitor);
        let mut partial =
            Partial::create(&Destination::of(&asked), Some(2_000)).expect("the temporary file");
        partial.accept(&[1; 1_500]).expect("under the ceiling");
        let error = partial.finish(Some(2_000)).expect_err("500 bytes short");
        assert_eq!(error.stage, DownloadStage::Body);
        assert_eq!(
            error.reason,
            "the body ended after 1500 of the 2000 bytes the server announced"
        );
        assert!(entries(&directory).is_empty());
    }
}
