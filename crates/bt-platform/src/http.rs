//! **One GET, over the operating system's own HTTP stack.**
//!
//! A fifth unsafe boundary, against a fifth thing. `windows_impl` is Win32 for
//! the window's sake, [`crate::webview`] is WebView2, [`crate::hang`] is Win32
//! turned on this process, [`crate::attention_pipe`] is a channel other
//! processes speak into — and this is the one place in the whole product where
//! *this* program opens a socket.
//!
//! # Why WinHTTP and not a crate
//!
//! The one thing the update check needs is a `GET` over TLS, and the two ways
//! to get one are a Rust HTTP client or the stack Windows already has. The
//! second was chosen, and the reasons are in this order:
//!
//! 1. **It adds no package.** `Win32_Networking_WinHttp` is a *feature* of the
//!    `windows` crate this crate already carries, so the lock file does not gain
//!    a line — `docs/DESIGN.md` §8's bar, and the same door
//!    `Win32_Media_MediaFoundation` came through for the video block. A blocking
//!    Rust client is roughly forty packages once its TLS stack and its
//!    certificate store are counted, and every one of them would land in
//!    `THIRD-PARTY-NOTICES.md` and in the audit surface of a terminal that
//!    otherwise reaches the network exactly never.
//! 2. **It is the machine's own configuration.** Proxy (including PAC and
//!    WPAD), the certificate store, revocation, and the enterprise policy a
//!    managed laptop carries are all the operating system's answers here, not
//!    ours. A bundled root store would be a second, staler opinion about who the
//!    user trusts.
//! 3. **The failure mode we want is silence**, and this returns a `String` to
//!    throw away rather than a family of typed errors nobody reads.
//!
//! # What it deliberately is not
//!
//! Not a client. There is no keep-alive, no connection reuse, no POST, no
//! request body, no header the caller can name, and no `http://`. It is one
//! function, it is shaped like the one call the product makes, and the next
//! caller that needs something else should widen it on purpose rather than find
//! it already widened.
//!
//! **Redirects are the one exception, and they are WinHTTP's own** — this
//! module writes no redirect code, which is not the same thing as a request
//! that does not follow one, and M4-10 had to find out which (the macOS arm has
//! to be given a policy explicitly, so the policy had to be named). Left alone,
//! WinHTTP follows redirects automatically up to
//! `WINHTTP_OPTION_MAX_HTTP_AUTOMATIC_REDIRECTS`, which defaults to ten, under
//! `WINHTTP_OPTION_REDIRECT_POLICY`, which defaults to
//! `DISALLOW_HTTPS_TO_HTTP`: a redirect that would take an `https` request to
//! `http` is **not** followed, and the `30x` is handed back as the response
//! instead — so `WinHttpQueryHeaders` reports it and the line below answers
//! `the server answered 302`. Neither option is set here, deliberately: the
//! defaults are the policy this door wants, and
//! `crates/bt-platform/src/macos_http.rs` is written to be them.
//!
//! # The bound on how long it can take
//!
//! WinHTTP's four timeouts are **per phase** — resolve, connect, send, receive
//! — so setting all four to five seconds bounds four separate waits and not the
//! whole. A body that arrives one byte at a time is inside the receive timeout
//! forever. So the loop that reads it carries a deadline of its own
//! ([`HttpsGet::budget`]), and that deadline is the number a caller can reason
//! about: nothing here outlives it by more than one phase timeout.
//!
//! # The download, and why it is asynchronous when the check is not
//!
//! [`https_download`] (U-7) streams one release asset to a file under the rules
//! [`crate::https_download`] states once for both real arms: a ceiling known
//! before the first byte, a `Content-Length` over it refused at the headers, a
//! count checked before each chunk is written, a temporary name renamed only
//! when the body ended where the transport said, and every failure named by
//! its stage. **The bound on a download is restated for it**: `idle_timeout` of
//! silence ([`crate::https_download::DOWNLOAD_IDLE_TIMEOUT`], 30 s) and
//! `budget` end to end ([`crate::https_download::DOWNLOAD_BUDGET`], 10 min),
//! both on the monotonic clock, and a cancel lands within
//! [`crate::https_download::DOWNLOAD_CANCEL_LATENCY`] in every phase —
//! connecting, waiting for headers, and inside the body.
//!
//! That last sentence is why the download runs WinHTTP in **asynchronous
//! mode** and the check above does not. A synchronous call blocks inside
//! WinHTTP for up to a phase timeout, and the only way to interrupt it would
//! be to close its handle from another thread — which Microsoft's
//! `WinHttpCloseHandle` page forbids outright: *"An application should never
//! call WinHttpCloseHandle on a synchronous request. This can create a race
//! condition."* In asynchronous mode every call returns at once, the
//! completion arrives on a status callback, and the thread that made the call
//! waits for it in slices it can end — and closing an in-flight asynchronous
//! request is the documented way to stop it. Same session options otherwise:
//! the machine's proxy, `WINHTTP_FLAG_SECURE`, no header but the agent, and
//! WinHTTP's own redirect policy (ten hops, never `https` to `http`).

use std::{
    ffi::c_void,
    sync::{Arc, Condvar, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use windows::{
    Win32::Networking::WinHttp::{
        API_QUERY_DATA_AVAILABLE, API_READ_DATA, API_RECEIVE_RESPONSE,
        ERROR_WINHTTP_HEADER_NOT_FOUND, INTERNET_DEFAULT_HTTPS_PORT,
        WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ASYNC_RESULT,
        WINHTTP_CALLBACK_FLAG_ALL_NOTIFICATIONS, WINHTTP_CALLBACK_STATUS_DATA_AVAILABLE,
        WINHTTP_CALLBACK_STATUS_HANDLE_CLOSING, WINHTTP_CALLBACK_STATUS_HEADERS_AVAILABLE,
        WINHTTP_CALLBACK_STATUS_READ_COMPLETE, WINHTTP_CALLBACK_STATUS_REQUEST_ERROR,
        WINHTTP_CALLBACK_STATUS_REQUEST_SENT, WINHTTP_CALLBACK_STATUS_SENDREQUEST_COMPLETE,
        WINHTTP_FLAG_ASYNC, WINHTTP_FLAG_SECURE, WINHTTP_OPEN_REQUEST_FLAGS,
        WINHTTP_OPTION_CONTEXT_VALUE, WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_FLAG_NUMBER,
        WINHTTP_QUERY_FLAG_NUMBER64, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
        WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpQueryHeaders,
        WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption,
        WinHttpSetStatusCallback, WinHttpSetTimeouts,
    },
    core::{HRESULT, PCWSTR},
};

use crate::https_download::{DOWNLOAD_CHUNK_BYTES, Deadlines, Destination, Partial, admit_length};

/// The one request this module knows how to make.
///
/// A struct rather than eight positional arguments because six of the eight are
/// strings and numbers of the same shape, and a caller that transposed `host`
/// and `path` would get a compiling program that asks the wrong server.
#[derive(Clone, Copy, Debug)]
pub struct HttpsGet<'a> {
    /// The host, with no scheme and no slash: `api.github.com`.
    pub host: &'a str,
    /// The path with its leading slash, query string included.
    pub path: &'a str,
    /// The `User-Agent` this request travels under.
    ///
    /// Not optional, and not defaulted: WinHTTP will happily send no agent at
    /// all, and what a request says about the program making it is exactly the
    /// kind of decision that must be visible at the call site rather than
    /// buried here. `docs/PRIVACY.md` names the string the product actually
    /// sends.
    pub user_agent: &'a str,
    /// Each of WinHTTP's four phase timeouts.
    pub phase_timeout: Duration,
    /// The whole call's own deadline, enforced across the read loop.
    pub budget: Duration,
    /// The most body this will read. A response longer than this is an error
    /// and not a truncation: half a JSON document is not a smaller answer, it is
    /// a different one.
    pub cap: usize,
}

/// A WinHTTP handle that closes itself.
///
/// Four handles are open at the deepest point of [`https_get`] and every early
/// return between them would otherwise have to close the right subset in the
/// right order. It is null-safe because the constructor is the thing that can
/// fail.
struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // The only failure a close can report is a handle that was already
            // invalid, and there is nothing above this to tell.
            let _ = unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

/// A NUL-terminated UTF-16 copy, which is the only string shape these calls take.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Fetch one `https://{host}{path}` and return its body as text.
///
/// `Ok` only for a `200` whose body is valid UTF-8 and within [`HttpsGet::cap`].
/// Every other outcome — no network, DNS failure, a proxy that refuses, an
/// expired certificate, a `403`, a `502`, a body that is too long, a body that
/// is not text — is an `Err` carrying a sentence for a log and nothing a caller
/// is expected to match on.
///
/// # Errors
///
/// Returns the reason as a sentence whenever the body of a `200` response did
/// not arrive whole.
pub fn https_get(request: &HttpsGet<'_>) -> Result<String, String> {
    let started = Instant::now();

    let agent = wide(request.user_agent);
    let host = wide(request.host);
    let path = wide(request.path);

    // `AUTOMATIC_PROXY` is the access type that consults the machine's own proxy
    // configuration — static settings, PAC file and WPAD alike. The alternative,
    // `NO_PROXY`, would work on a home machine and fail on every managed one,
    // which is the worst of the two failure distributions: it would look correct
    // to whoever wrote it.
    let session = Handle(unsafe {
        WinHttpOpen(
            PCWSTR(agent.as_ptr()),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        )
    });
    if session.0.is_null() {
        return Err("WinHttpOpen refused a session".to_owned());
    }

    let phase = i32::try_from(request.phase_timeout.as_millis()).unwrap_or(i32::MAX);
    unsafe { WinHttpSetTimeouts(session.0, phase, phase, phase, phase) }
        .map_err(|error| format!("WinHttpSetTimeouts: {error}"))?;

    let connection = Handle(unsafe {
        WinHttpConnect(
            session.0,
            PCWSTR(host.as_ptr()),
            INTERNET_DEFAULT_HTTPS_PORT,
            0,
        )
    });
    if connection.0.is_null() {
        return Err(format!("WinHttpConnect refused {}", request.host));
    }

    // `WINHTTP_FLAG_SECURE` is the whole of the TLS decision: there is no
    // `http://` path through this function, so a caller cannot downgrade one by
    // passing a different string.
    let verb = wide("GET");
    let exchange = Handle(unsafe {
        WinHttpOpenRequest(
            connection.0,
            PCWSTR(verb.as_ptr()),
            PCWSTR(path.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        )
    });
    if exchange.0.is_null() {
        return Err(format!("WinHttpOpenRequest refused {}", request.path));
    }

    // No headers, no body: `None` for both, and the two lengths that go with
    // them are zero. Everything this request says about itself, it says in the
    // agent string set on the session above.
    unsafe { WinHttpSendRequest(exchange.0, None, None, 0, 0, 0) }
        .map_err(|error| format!("WinHttpSendRequest: {error}"))?;
    unsafe { WinHttpReceiveResponse(exchange.0, std::ptr::null_mut()) }
        .map_err(|error| format!("WinHttpReceiveResponse: {error}"))?;

    let mut status: u32 = 0;
    let mut length = u32::try_from(std::mem::size_of::<u32>()).unwrap_or(4);
    unsafe {
        WinHttpQueryHeaders(
            exchange.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(std::ptr::from_mut(&mut status).cast::<c_void>()),
            &raw mut length,
            std::ptr::null_mut(),
        )
    }
    .map_err(|error| format!("WinHttpQueryHeaders: {error}"))?;
    if status != 200 {
        return Err(format!("the server answered {status}"));
    }

    let mut body: Vec<u8> = Vec::new();
    loop {
        if started.elapsed() > request.budget {
            return Err("the body did not arrive inside its budget".to_owned());
        }

        let mut available: u32 = 0;
        unsafe { WinHttpQueryDataAvailable(exchange.0, &raw mut available) }
            .map_err(|error| format!("WinHttpQueryDataAvailable: {error}"))?;
        if available == 0 {
            break;
        }

        let want = usize::try_from(available).unwrap_or(usize::MAX);
        if body.len().saturating_add(want) > request.cap {
            return Err(format!("the body is longer than {} bytes", request.cap));
        }

        let already = body.len();
        body.resize(already + want, 0);
        let mut read: u32 = 0;
        unsafe {
            WinHttpReadData(
                exchange.0,
                body.as_mut_ptr().add(already).cast::<c_void>(),
                available,
                &raw mut read,
            )
        }
        .map_err(|error| format!("WinHttpReadData: {error}"))?;
        // A short read is normal; the loop asks again. A zero read after a
        // non-zero `available` is the end of the body arriving as a surprise,
        // and truncating the buffer to it is what makes the next
        // `QueryDataAvailable` answer zero and the loop finish.
        body.truncate(already + usize::try_from(read).unwrap_or(0));
        if read == 0 {
            break;
        }
    }

    String::from_utf8(body).map_err(|_| "the body is not text".to_owned())
}

pub use crate::https_download::{
    DownloadError, DownloadMonitor, DownloadProgress, DownloadStage, Downloaded, HttpsDownload,
};

/// **Stream `https://{host}{path}` to `directory/file_name`, bounded** (U-7).
///
/// `Ok` only for a `200` whose body arrived whole, within the ceiling, flushed
/// and renamed to its name. Every other outcome is an `Err` naming the stage it
/// stopped at, with nothing left in the directory — neither the final name nor
/// the temporary one. No retries and no resumption: see
/// [`crate::https_download`].
///
/// Blocks for as long as the transfer takes, up to `budget`: a worker's call,
/// never the window thread's.
///
/// # Errors
///
/// A [`DownloadError`] whenever the file is not whole under its name.
pub fn https_download(request: &HttpsDownload<'_>) -> Result<Downloaded, DownloadError> {
    download_from(
        &Target {
            host: request.host,
            port: INTERNET_DEFAULT_HTTPS_PORT,
            secure: true,
        },
        request,
    )
}

/// **Where a download is sent.** [`https_download`] builds the only one the
/// product uses — the caller's host, port 443, TLS — and the tests build a
/// loopback one, which is the seam `macos_http.rs` splits `fetch` at for the
/// same reason: the door itself cannot be pointed at this machine.
struct Target<'a> {
    host: &'a str,
    port: u16,
    secure: bool,
}

/// What the status callback hands the waiting thread: one completion, of the
/// one call outstanding.
#[derive(Clone, Copy, Debug)]
enum Completion {
    Sent,
    Headers,
    Available(u32),
    Read(u32),
    Failed { api: usize, code: u32 },
}

/// The part of an asynchronous request the callback and the waiting thread
/// share.
///
/// One allocation, reference-counted: the waiting thread holds one count and
/// the request handle holds another as its context value, given back by the
/// callback at `WINHTTP_CALLBACK_STATUS_HANDLE_CLOSING` — Microsoft's rule is
/// that a handle's context must live until that notification, because WinHTTP
/// may call back after `WinHttpCloseHandle` has returned. The read buffer is
/// here for the same reason: an abandoned `WinHttpReadData` may still be
/// writing into it after the waiting thread has gone.
struct Shared {
    signals: Mutex<Signals>,
    raised: Condvar,
    /// `DOWNLOAD_CHUNK_BYTES` bytes, written by WinHTTP between a
    /// `WinHttpReadData` and its `READ_COMPLETE`, read by the waiting thread
    /// only after that completion.
    buffer: *mut u8,
}

#[derive(Default)]
struct Signals {
    completion: Option<Completion>,
    /// `WINHTTP_CALLBACK_STATUS_REQUEST_SENT` has arrived: the request is on
    /// the wire. WinHTTP holds `SENDREQUEST_COMPLETE` back until the server
    /// starts to answer (observed on Windows 11 against a loopback server
    /// that reads the request and says nothing), so this is what moves a
    /// silent wait from the connect stage to the headers stage.
    sent: bool,
    /// A notification other than a completion arrived since the waiting
    /// thread last looked — the idle clock's input.
    stirred: bool,
    closed: bool,
}

// SAFETY: `buffer` is owned by `Shared` and freed only in its `Drop`; every
// other field is `Sync`. Who touches the buffer when is the protocol stated on
// the field: one outstanding read at a time, handed over through `signals`.
unsafe impl Send for Shared {}
// SAFETY: as above.
unsafe impl Sync for Shared {}

impl Shared {
    fn new() -> Self {
        let buffer = Box::into_raw(vec![0u8; DOWNLOAD_CHUNK_BYTES].into_boxed_slice());
        Self {
            signals: Mutex::new(Signals::default()),
            raised: Condvar::new(),
            buffer: buffer.cast::<u8>(),
        }
    }

    fn locked(&self) -> MutexGuard<'_, Signals> {
        self.signals
            .lock()
            .expect("the download's signals are not held across a panic")
    }

    fn raise(&self, completion: Completion) {
        self.locked().completion = Some(completion);
        self.raised.notify_all();
    }

    /// **Wait for the outstanding call's completion**, in slices short enough
    /// that a cancel or a deadline is seen within `DOWNLOAD_CANCEL_LATENCY`.
    fn wait(
        &self,
        stage: DownloadStage,
        deadlines: &mut Deadlines,
        monitor: &DownloadMonitor,
    ) -> Result<Completion, DownloadError> {
        let mut signals = self.locked();
        loop {
            if let Some(completion) = signals.completion.take() {
                deadlines.heard();
                return Ok(completion);
            }
            if std::mem::take(&mut signals.stirred) {
                deadlines.heard();
            }
            let stage = if stage == DownloadStage::Connect && signals.sent {
                DownloadStage::Headers
            } else {
                stage
            };
            deadlines.check(stage, monitor)?;
            let (next, _) = self
                .raised
                .wait_timeout(signals, deadlines.slice())
                .expect("the download's signals are not held across a panic");
            signals = next;
        }
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        let slice = std::ptr::slice_from_raw_parts_mut(self.buffer, DOWNLOAD_CHUNK_BYTES);
        // SAFETY: the pointer came from `Box::into_raw` of a boxed slice of
        // exactly this length in `Shared::new`, and nothing else frees it.
        drop(unsafe { Box::from_raw(slice) });
    }
}

/// **The status callback**: one completion in, one wake out.
///
/// Runs on a WinHTTP thread, or on the calling thread inside the call when an
/// operation completes at once — which is why the waiting thread never holds
/// the signals' lock across a WinHTTP call.
unsafe extern "system" fn on_status(
    _handle: *mut c_void,
    context: usize,
    status: u32,
    information: *mut c_void,
    length: u32,
) {
    if context == 0 {
        return;
    }
    let shared = context as *const Shared;
    // SAFETY: a non-zero context is the count `Request::arm` gave the handle,
    // alive until the `HANDLE_CLOSING` arm below takes it back.
    let borrowed = unsafe { &*shared };
    match status {
        WINHTTP_CALLBACK_STATUS_SENDREQUEST_COMPLETE => borrowed.raise(Completion::Sent),
        WINHTTP_CALLBACK_STATUS_HEADERS_AVAILABLE => borrowed.raise(Completion::Headers),
        WINHTTP_CALLBACK_STATUS_DATA_AVAILABLE => {
            // SAFETY: for this status the information is the DWORD count.
            let available = unsafe { *information.cast::<u32>() };
            borrowed.raise(Completion::Available(available));
        }
        WINHTTP_CALLBACK_STATUS_READ_COMPLETE => borrowed.raise(Completion::Read(length)),
        WINHTTP_CALLBACK_STATUS_REQUEST_SENT => {
            let mut signals = borrowed.locked();
            signals.sent = true;
            signals.stirred = true;
        }
        WINHTTP_CALLBACK_STATUS_REQUEST_ERROR => {
            // SAFETY: for this status the information is a WINHTTP_ASYNC_RESULT.
            let result = unsafe { *information.cast::<WINHTTP_ASYNC_RESULT>() };
            borrowed.raise(Completion::Failed {
                api: result.dwResult,
                code: result.dwError,
            });
        }
        WINHTTP_CALLBACK_STATUS_HANDLE_CLOSING => {
            borrowed.locked().closed = true;
            borrowed.raised.notify_all();
            // SAFETY: the last callback for this handle; the count the handle
            // held is given back exactly once, here.
            drop(unsafe { Arc::from_raw(shared) });
        }
        _ => {}
    }
}

/// How long closing an abandoned request may wait for WinHTTP to say the
/// handle is gone. Past it the handle's count is left to WinHTTP, which is a
/// leak of one buffer and never a use after free.
const CLOSE_WAIT: Duration = Duration::from_secs(5);

/// **An asynchronous request handle, with its callback and its context.**
///
/// Dropping it closes the handle — the documented way to stop an in-flight
/// asynchronous request — and waits for `HANDLE_CLOSING`, so no callback for
/// it runs after the download has returned.
struct Request {
    handle: *mut c_void,
    shared: Arc<Shared>,
    /// The context value the handle carries, which `WinHttpSendRequest` is
    /// given again: its `dwContext` replaces the handle's context for the
    /// callbacks that follow, so passing `0` there would silence them.
    context: usize,
}

impl Request {
    /// Give `handle` its context and its callback. On failure the handle is
    /// closed here.
    fn arm(handle: *mut c_void) -> Result<Self, DownloadError> {
        let shared = Arc::new(Shared::new());
        let context = Arc::into_raw(Arc::clone(&shared)) as usize;
        let bytes = context.to_ne_bytes();
        // SAFETY: a live request handle; the option is a DWORD_PTR.
        let given = unsafe {
            WinHttpSetOption(
                Some(handle.cast_const()),
                WINHTTP_OPTION_CONTEXT_VALUE,
                Some(&bytes),
            )
        };
        // SAFETY: a live request handle and a callback of the right shape.
        let previous = given.is_ok().then(|| unsafe {
            WinHttpSetStatusCallback(
                handle,
                Some(on_status),
                WINHTTP_CALLBACK_FLAG_ALL_NOTIFICATIONS,
                0,
            )
        });
        // `WINHTTP_INVALID_STATUS_CALLBACK` is the pointer value -1.
        let refused = previous
            .flatten()
            .is_some_and(|callback| callback as usize == usize::MAX);
        if given.is_err() || refused {
            // No callback will ever give the context's count back: close the
            // handle with none installed, then take the count back here.
            // SAFETY: a live handle, closed once; the count was handed to
            // nothing that will release it.
            unsafe {
                let _ = WinHttpCloseHandle(handle);
                drop(Arc::from_raw(context as *const Shared));
            }
            return Err(DownloadError::at(
                DownloadStage::Connect,
                "WinHTTP refused the request's callback".to_owned(),
            ));
        }
        Ok(Self {
            handle,
            shared,
            context,
        })
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        // SAFETY: a live handle, closed once.
        let _ = unsafe { WinHttpCloseHandle(self.handle) };
        let started = Instant::now();
        let mut signals = self.shared.locked();
        while !signals.closed {
            let left = CLOSE_WAIT.saturating_sub(started.elapsed());
            if left.is_zero() {
                return;
            }
            let (next, _) = self
                .shared
                .raised
                .wait_timeout(signals, left)
                .expect("the download's signals are not held across a panic");
            signals = next;
        }
    }
}

/// The stage a failed asynchronous call belongs to.
fn stage_of(api: usize) -> DownloadStage {
    match u32::try_from(api).unwrap_or(u32::MAX) {
        API_RECEIVE_RESPONSE => DownloadStage::Headers,
        API_QUERY_DATA_AVAILABLE | API_READ_DATA => DownloadStage::Body,
        _ => DownloadStage::Connect,
    }
}

/// A completion that is not the one the call waits for — in practice an
/// error, named at the stage of the call that failed.
fn unexpected(stage: DownloadStage, completion: Completion) -> DownloadError {
    match completion {
        Completion::Failed { api, code } => {
            DownloadError::at(stage_of(api), format!("WinHTTP answered error {code}"))
        }
        other => DownloadError::at(stage, format!("WinHTTP answered {other:?} out of turn")),
    }
}

/// The response's `Content-Length`, or `None` when it carries none.
fn content_length(handle: *mut c_void) -> Result<Option<u64>, DownloadError> {
    let mut length: u64 = 0;
    let mut size = u32::try_from(std::mem::size_of::<u64>()).unwrap_or(8);
    // SAFETY: a live request whose headers are available; the buffer is a u64
    // and its size says so.
    let answer = unsafe {
        WinHttpQueryHeaders(
            handle,
            WINHTTP_QUERY_CONTENT_LENGTH | WINHTTP_QUERY_FLAG_NUMBER64,
            PCWSTR::null(),
            Some(std::ptr::from_mut(&mut length).cast::<c_void>()),
            &raw mut size,
            std::ptr::null_mut(),
        )
    };
    match answer {
        Ok(()) => Ok(Some(length)),
        Err(error) if error.code() == HRESULT::from_win32(ERROR_WINHTTP_HEADER_NOT_FOUND) => {
            Ok(None)
        }
        Err(error) => Err(DownloadError::at(
            DownloadStage::Headers,
            format!("reading Content-Length: {error}"),
        )),
    }
}

/// **The whole download, once the target is settled** — the session, the
/// request, the four waits, and the file.
fn download_from(
    target: &Target<'_>,
    request: &HttpsDownload<'_>,
) -> Result<Downloaded, DownloadError> {
    let monitor: &DownloadMonitor = request.monitor;
    let mut deadlines = Deadlines::start(request.idle_timeout, request.budget);
    let connect = |why: String| DownloadError::at(DownloadStage::Connect, why);
    let headers = |why: String| DownloadError::at(DownloadStage::Headers, why);
    let body = |why: String| DownloadError::at(DownloadStage::Body, why);

    let agent = wide(request.user_agent);
    let host = wide(target.host);
    let path = wide(request.path);

    // The check's session, in asynchronous mode — see this module's header.
    let session = Handle(unsafe {
        WinHttpOpen(
            PCWSTR(agent.as_ptr()),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            WINHTTP_FLAG_ASYNC,
        )
    });
    if session.0.is_null() {
        return Err(connect("WinHttpOpen refused a session".to_owned()));
    }
    let phase = i32::try_from(request.idle_timeout.as_millis()).unwrap_or(i32::MAX);
    unsafe { WinHttpSetTimeouts(session.0, phase, phase, phase, phase) }
        .map_err(|error| connect(format!("WinHttpSetTimeouts: {error}")))?;

    let connection =
        Handle(unsafe { WinHttpConnect(session.0, PCWSTR(host.as_ptr()), target.port, 0) });
    if connection.0.is_null() {
        return Err(connect(format!("WinHttpConnect refused {}", target.host)));
    }

    let verb = wide("GET");
    let flags = if target.secure {
        WINHTTP_FLAG_SECURE
    } else {
        WINHTTP_OPEN_REQUEST_FLAGS(0)
    };
    let handle = unsafe {
        WinHttpOpenRequest(
            connection.0,
            PCWSTR(verb.as_ptr()),
            PCWSTR(path.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            flags,
        )
    };
    if handle.is_null() {
        return Err(connect(format!(
            "WinHttpOpenRequest refused {}",
            request.path
        )));
    }
    // Declared after `session` and `connection`, so it is dropped — closed,
    // and waited for — before either of them.
    let exchange = Request::arm(handle)?;
    let shared = Arc::clone(&exchange.shared);

    deadlines.check(DownloadStage::Connect, monitor)?;
    unsafe { WinHttpSendRequest(exchange.handle, None, None, 0, 0, exchange.context) }
        .map_err(|error| connect(format!("WinHttpSendRequest: {error}")))?;
    match shared.wait(DownloadStage::Connect, &mut deadlines, monitor)? {
        Completion::Sent => {}
        other => return Err(unexpected(DownloadStage::Connect, other)),
    }

    unsafe { WinHttpReceiveResponse(exchange.handle, std::ptr::null_mut()) }
        .map_err(|error| headers(format!("WinHttpReceiveResponse: {error}")))?;
    match shared.wait(DownloadStage::Headers, &mut deadlines, monitor)? {
        Completion::Headers => {}
        other => return Err(unexpected(DownloadStage::Headers, other)),
    }

    let mut status: u32 = 0;
    let mut size = u32::try_from(std::mem::size_of::<u32>()).unwrap_or(4);
    unsafe {
        WinHttpQueryHeaders(
            exchange.handle,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(std::ptr::from_mut(&mut status).cast::<c_void>()),
            &raw mut size,
            std::ptr::null_mut(),
        )
    }
    .map_err(|error| headers(format!("WinHttpQueryHeaders: {error}")))?;
    if status != 200 {
        return Err(DownloadError::at(
            DownloadStage::Status,
            format!("the server answered {status}"),
        ));
    }
    let announced = content_length(exchange.handle)?;
    admit_length(announced, request.effective_ceiling())?;

    let mut partial = Partial::create(&Destination::of(request), announced)?;
    let most = u32::try_from(DOWNLOAD_CHUNK_BYTES).unwrap_or(u32::MAX);
    loop {
        // In asynchronous mode the count arrives by callback, so the
        // out-pointer is null, as the documentation requires.
        unsafe { WinHttpQueryDataAvailable(exchange.handle, std::ptr::null_mut()) }
            .map_err(|error| body(format!("WinHttpQueryDataAvailable: {error}")))?;
        let available = match shared.wait(DownloadStage::Body, &mut deadlines, monitor)? {
            Completion::Available(available) => available,
            other => return Err(unexpected(DownloadStage::Body, other)),
        };
        if available == 0 {
            break;
        }
        let want = available.min(most);
        // SAFETY: the buffer is `DOWNLOAD_CHUNK_BYTES` long and owned by
        // `shared` until the handle's last callback; `want` is no more than
        // that; the out-pointer is null in asynchronous mode.
        unsafe {
            WinHttpReadData(
                exchange.handle,
                shared.buffer.cast::<c_void>(),
                want,
                std::ptr::null_mut(),
            )
        }
        .map_err(|error| body(format!("WinHttpReadData: {error}")))?;
        let read = match shared.wait(DownloadStage::Body, &mut deadlines, monitor)? {
            Completion::Read(read) => read,
            other => return Err(unexpected(DownloadStage::Body, other)),
        };
        if read == 0 {
            break;
        }
        let read = usize::try_from(read.min(want)).unwrap_or(0);
        // SAFETY: the read completed, so WinHTTP is done with the buffer until
        // the next `WinHttpReadData`, and it wrote `read` bytes of it.
        let chunk = unsafe { std::slice::from_raw_parts(shared.buffer, read) };
        partial.accept(chunk)?;
        deadlines.check(DownloadStage::Body, monitor)?;
    }
    drop(exchange);
    partial.finish(announced)
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
        time::{Duration, Instant},
    };

    use super::{
        DownloadError, DownloadMonitor, DownloadStage, Downloaded, HttpsDownload, Target,
        download_from,
    };
    use crate::https_download::{
        DOWNLOAD_BUDGET, DOWNLOAD_IDLE_TIMEOUT,
        loopback::{Step, begun, body, head, ok, serve},
    };

    const IDLE: Duration = Duration::from_secs(5);
    const BUDGET: Duration = Duration::from_secs(20);

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir()
            .join("bt-platform-http-download")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        directory
    }

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

    /// The product's download, put to a server on this machine's loopback —
    /// `download_from` and not `https_download`, because the door builds only
    /// `https` on port 443.
    fn fetch(
        port: u16,
        directory: &Path,
        ceiling: u64,
        deadlines: (Duration, Duration),
        monitor: &Arc<DownloadMonitor>,
    ) -> Result<Downloaded, DownloadError> {
        download_from(
            &Target {
                host: "127.0.0.1",
                port,
                secure: false,
            },
            &HttpsDownload {
                host: "127.0.0.1",
                path: "/folio-0.4.6-x64.zip",
                user_agent: "Folio",
                directory,
                file_name: "folio.zip",
                ceiling,
                idle_timeout: deadlines.0,
                budget: deadlines.1,
                monitor,
            },
        )
    }

    fn quiet() -> Arc<DownloadMonitor> {
        Arc::new(DownloadMonitor::new(|| {}))
    }

    /// RED (U-7) — **a body with its length, served over the real stack, lands
    /// under its name byte for byte, and nothing else is left.**
    ///
    /// The happy path: WinHTTP in asynchronous mode, the callback, the fixed
    /// buffer, the count and the rename, end to end, with a body several
    /// chunks long so the read loop turns.
    ///
    /// MUTATION: skip `partial.accept` for one chunk in the read loop and the
    /// length check refuses the body; read from a stale slice of the buffer
    /// and the bytes differ.
    #[test]
    fn a_whole_body_lands_under_its_name_byte_for_byte() {
        let directory = scratch("whole");
        let bytes = body(300_000);
        let served = bytes.clone();
        let port = serve(1, move |_| {
            vec![Step::Send(ok(&served, Some(served.len())))]
        });
        let monitor = quiet();
        let done = fetch(port, &directory, 1 << 20, (IDLE, BUDGET), &monitor).expect("the body");
        assert_eq!(done.bytes, 300_000);
        assert_eq!(std::fs::read(&done.path).expect("the file"), bytes);
        assert_eq!(entries(&directory), vec!["folio.zip".to_owned()]);
        let seen = monitor.take();
        assert_eq!(seen.received, 300_000);
        assert_eq!(seen.expected, Some(300_000));
    }

    /// RED (U-7) — **a body exactly at the ceiling is accepted.**
    ///
    /// MUTATION: make the ceiling test `>=` in `Partial::accept` or in
    /// `admit_length`.
    #[test]
    fn a_body_exactly_at_the_ceiling_is_accepted() {
        let directory = scratch("at-ceiling");
        let bytes = body(70_000);
        let served = bytes.clone();
        let port = serve(1, move |_| {
            vec![Step::Send(ok(&served, Some(served.len())))]
        });
        let done =
            fetch(port, &directory, 70_000, (IDLE, BUDGET), &quiet()).expect("at the ceiling");
        assert_eq!(std::fs::read(&done.path).expect("the file"), bytes);
    }

    /// RED (U-7) — **a stream with no length is bounded: one byte over the
    /// ceiling is refused mid-stream, and neither name is left.**
    ///
    /// §F Transport: `unknown_length_stream_is_bounded`. The server announces
    /// nothing and ends the body by closing, so no header can refuse it; the
    /// count is the only bound, and it holds before the chunk is written.
    ///
    /// MUTATION: return `Ok(())` early from `Partial::accept`'s ceiling test,
    /// and the file lands with 70,001 bytes.
    #[test]
    fn unknown_length_stream_is_bounded() {
        let directory = scratch("over-unknown");
        let served = body(70_001);
        let port = serve(1, move |_| vec![Step::Send(ok(&served, None))]);
        let monitor = quiet();
        let error =
            fetch(port, &directory, 70_000, (IDLE, BUDGET), &monitor).expect_err("one byte over");
        assert_eq!(error.stage, DownloadStage::Body, "{error}");
        assert_eq!(error.reason, "the body passed the ceiling of 70000 bytes");
        assert!(entries(&directory).is_empty(), "{:?}", entries(&directory));
        assert_eq!(monitor.peek().expected, None, "no length was announced");
    }

    /// RED (U-7) — **a `Content-Length` over the ceiling is refused at the
    /// headers, before any body is read or any file made.**
    ///
    /// The server announces a gigabyte, sends its first KiB and then says
    /// nothing; the refusal must not wait for the body to prove the header
    /// right, and not one byte of it reaches a file.
    ///
    /// MUTATION: skip `admit_length` in `download_from`, and this waits for
    /// the idle timeout and fails at the body stage instead.
    #[test]
    fn an_announced_length_over_the_ceiling_is_refused_before_the_body() {
        let directory = scratch("announced-over");
        let port = serve(1, |_| {
            vec![
                Step::Send(begun(&["Content-Length: 1073741824".to_owned()])),
                Step::Pause(Duration::from_secs(10)),
            ]
        });
        let started = Instant::now();
        let error =
            fetch(port, &directory, 70_000, (IDLE, BUDGET), &quiet()).expect_err("too long");
        assert_eq!(error.stage, DownloadStage::Headers, "{error}");
        assert_eq!(
            error.reason,
            "the server announced 1073741824 bytes, more than the ceiling of 70000"
        );
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        assert!(entries(&directory).is_empty());
    }

    /// RED (U-7) — **a body cut short of its announced length is refused, and
    /// no file stands under the final name.**
    ///
    /// The server announces 100,000 bytes, sends 40,000 and closes — the
    /// transport's own end of the body is not the end the response promised.
    ///
    /// MUTATION: pass `None` instead of `announced` to `Partial::finish`, and
    /// a 40,000-byte `folio.zip` appears.
    #[test]
    fn a_truncated_body_never_takes_the_name() {
        let directory = scratch("truncated");
        let port = serve(1, |_| {
            let mut bytes = head(200, "OK", &["Content-Length: 100000".to_owned()]);
            bytes.extend_from_slice(&body(40_000));
            vec![Step::Send(bytes)]
        });
        let error =
            fetch(port, &directory, 1 << 20, (IDLE, BUDGET), &quiet()).expect_err("cut short");
        assert_eq!(error.stage, DownloadStage::Body, "{error}");
        assert!(entries(&directory).is_empty(), "{:?}", entries(&directory));
    }

    /// RED (U-7) — **a status that is not `200` is the status stage, in the
    /// check's own sentence, and makes no file.**
    ///
    /// MUTATION: drop the status test in `download_from`, and the 404's page
    /// is saved as the asset.
    #[test]
    fn a_status_that_is_not_200_makes_no_file() {
        let directory = scratch("status");
        let port = serve(1, |_| {
            let page = b"<h1>Not Found</h1>";
            let mut bytes = head(
                404,
                "Not Found",
                &[format!("Content-Length: {}", page.len())],
            );
            bytes.extend_from_slice(page);
            vec![Step::Send(bytes)]
        });
        let error = fetch(port, &directory, 1 << 20, (IDLE, BUDGET), &quiet()).expect_err("404");
        assert_eq!(error.stage, DownloadStage::Status);
        assert_eq!(error.reason, "the server answered 404");
        assert!(
            !error.to_string().contains("<h1>"),
            "the body is never echoed"
        );
        assert!(entries(&directory).is_empty());
    }

    /// RED (U-7) — **a redirect is the stack's own: WinHTTP follows it and the
    /// file is the final body.**
    ///
    /// The door writes no redirect code, as `https_get` writes none; this pins
    /// that the asynchronous session kept WinHTTP's default (follow, up to ten
    /// hops) — GitHub serves every release asset through a redirect. What a
    /// loopback server cannot show is the other half of §F's
    /// `https_redirects_work_and_http_redirects_refuse`: an `https` origin sent
    /// to `http`, which `WINHTTP_OPTION_REDIRECT_POLICY`'s default refuses.
    /// That needs TLS on this machine; this module sets no redirect option, so
    /// the default is what runs.
    ///
    /// MUTATION: set `WINHTTP_OPTION_REDIRECT_POLICY_NEVER` on the request, and
    /// this answers `the server answered 302`.
    #[test]
    fn a_redirect_the_stack_follows_lands_the_final_body() {
        let directory = scratch("redirect");
        let bytes = body(5_000);
        let served = bytes.clone();
        let port = serve(2, move |path| {
            if path == "/folio-0.4.6-x64.zip" {
                vec![Step::Send(head(
                    302,
                    "Found",
                    &[
                        "Location: /objects/asset".to_owned(),
                        "Content-Length: 0".to_owned(),
                    ],
                ))]
            } else {
                vec![Step::Send(ok(&served, Some(served.len())))]
            }
        });
        let done = fetch(port, &directory, 1 << 20, (IDLE, BUDGET), &quiet()).expect("followed");
        assert_eq!(std::fs::read(&done.path).expect("the file"), bytes);
    }

    /// RED (U-7) — **a trickling body and a redirect loop each end inside the
    /// budget, and neither leaves a file.**
    ///
    /// §F Transport: `redirect_loop_and_trickle_body_hit_deadlines`. The
    /// trickle sends a byte every 50 ms, so the idle timeout never fires; the
    /// budget, which nothing resets, is what ends it.
    ///
    /// MUTATION: reset `started` in `Deadlines::heard`, and the trickle runs
    /// until the server gives up.
    #[test]
    fn redirect_loop_and_trickle_body_hit_deadlines() {
        let directory = scratch("trickle");
        let port = serve(1, |_| {
            vec![
                Step::Send(begun(&[])),
                Step::Trickle(Duration::from_millis(50), Duration::from_secs(8)),
            ]
        });
        let started = Instant::now();
        let budget = Duration::from_millis(1_500);
        let error =
            fetch(port, &directory, 1 << 20, (IDLE, budget), &quiet()).expect_err("the budget");
        let took = started.elapsed();
        assert_eq!(error.stage, DownloadStage::Body, "{error}");
        assert!(error.reason.contains("inside its budget"), "{error}");
        assert!(took < budget + Duration::from_secs(1), "{took:?}");
        assert!(entries(&directory).is_empty(), "{:?}", entries(&directory));

        let directory = scratch("loop");
        let port = serve(40, |_| {
            vec![Step::Send(head(
                302,
                "Found",
                &[
                    "Location: /again".to_owned(),
                    "Content-Length: 0".to_owned(),
                ],
            ))]
        });
        let started = Instant::now();
        let error = fetch(port, &directory, 1 << 20, (IDLE, BUDGET), &quiet()).expect_err("a loop");
        assert!(started.elapsed() < BUDGET, "{:?}", started.elapsed());
        assert!(entries(&directory).is_empty(), "{error}");
    }

    /// RED (U-7) — **a cancel interrupts a wait the server is not ending: the
    /// download answers cancelled within the stated latency, not at the idle
    /// timeout.**
    ///
    /// §F Transport: `cancel_interrupts_idle_wait`. The server reads the
    /// request and says nothing; the deadlines are the product's (30 s idle,
    /// 10 min). A synchronous WinHTTP call could not be interrupted here —
    /// which is why the download is asynchronous.
    ///
    /// MUTATION: drop the cancel test from `Deadlines::check`, and this waits
    /// the full thirty seconds.
    #[test]
    fn cancel_interrupts_idle_wait() {
        let directory = scratch("cancel");
        // The cancel is sent once the server holds the whole request, so the
        // wait it interrupts is the wait for the response's headers.
        let (asked, heard) = std::sync::mpsc::channel();
        let port = serve(1, move |_| {
            let _ = asked.send(());
            vec![Step::Pause(Duration::from_secs(40))]
        });
        let monitor = quiet();
        let canceller = Arc::clone(&monitor);
        std::thread::spawn(move || {
            if heard.recv_timeout(Duration::from_secs(20)).is_ok() {
                std::thread::sleep(Duration::from_millis(300));
            }
            canceller.cancel();
        });
        let started = Instant::now();
        let error = fetch(
            port,
            &directory,
            1 << 20,
            (DOWNLOAD_IDLE_TIMEOUT, DOWNLOAD_BUDGET),
            &monitor,
        )
        .expect_err("cancelled");
        let took = started.elapsed();
        assert!(error.cancelled, "{error}");
        assert_eq!(error.stage, DownloadStage::Headers, "{error}");
        assert!(took < Duration::from_secs(2), "{took:?}");
        assert!(entries(&directory).is_empty());
    }
}
