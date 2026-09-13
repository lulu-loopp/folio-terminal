//! **One `GET`, over the operating system's own HTTP stack — the macOS arm**
//! (M4-10).
//!
//! The Windows arm ([`crate::http`]'s other file) says why this is a platform
//! door at all rather than a Rust HTTP client, and every word of it is true
//! here: what the operating system's stack buys is its TLS, its certificate
//! store, its revocation and the proxy configuration a managed laptop is handed
//! — including the PAC file — rather than forty packages and a trust store of
//! our own. `NSURLSession` is that stack on a Mac, it is in Foundation, and it
//! adds not one line to `Cargo.lock`.
//!
//! # Why an ephemeral configuration and not `+[NSURLSession sharedSession]`
//!
//! The shared session is the obvious call and it is the wrong one, for two
//! reasons that are both about state this request must not have.
//!
//! 1. **It is three process-wide stores.** The shared session uses the shared
//!    `NSURLCache`, the shared `NSHTTPCookieStorage` and the shared
//!    `NSURLCredentialStorage`, each of which persists under the container — so
//!    one update check would both read from and write to them.
//!    `bt_app::update`'s whole contract is that the request carries nothing
//!    about the machine and leaves nothing behind; a cookie jar that survives
//!    the process is the opposite of that.
//!    `+[NSURLSessionConfiguration ephemeralSessionConfiguration]` is Apple's
//!    own name for "no persistent storage for caches, cookies, or credentials",
//!    and the session is invalidated before this function returns, so the
//!    in-memory ones go with it.
//! 2. **Its configuration is read-only.** `-[NSURLSession configuration]`
//!    answers a copy, so the shared session cannot be given the two timeouts
//!    this call is bounded by. A door whose deadline cannot be set is not this
//!    door.
//!
//! # Why the delegate and not `dataTaskWithRequest:completionHandler:`
//!
//! The convenience method hands back the whole body at once, and **two of the
//! things this contract promises are decisions that have to be taken while the
//! transfer is still running**:
//!
//! * **The size cap.** [`HttpsGet::cap`] is not a truncation, it is a refusal —
//!   and a refusal that arrives after a gigabyte has already been read is a
//!   refusal that did not do its job. The Windows arm checks before each
//!   `WinHttpReadData`; this checks in `URLSession:dataTask:didReceiveData:`
//!   and cancels the task, which is the same moment.
//! * **The redirect policy.** WinHTTP follows redirects by default and refuses
//!   exactly one kind — `https://` to `http://`
//!   (`WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP`), up to
//!   `WINHTTP_OPTION_MAX_HTTP_AUTOMATIC_REDIRECTS`, which is ten.
//!   `NSURLSession` follows them too, and follows the downgrade as well unless
//!   a delegate says otherwise.
//!   `URLSession:task:willPerformHTTPRedirection:newRequest:` is where it says
//!   otherwise, and refusing there hands the `30x` back to this caller as the
//!   final response — which is exactly what WinHTTP does with a redirect it
//!   will not follow, and why both arms then answer `the server answered 302`.
//!
//! Handing the same `NSURLSession` a completion handler *and* a data delegate
//! is not an option: Apple documents that the response and data delegate
//! methods are not called when a completion handler is supplied. So the whole
//! exchange is the delegate's.
//!
//! # The wait, and what bounds it
//!
//! The call is synchronous on the caller's own thread — `bt_app::update::begin`
//! runs it on a thread it started at the background band, and that thread is
//! the one that must block. The delegate runs on a serial queue `NSURLSession`
//! makes for it, so the two never share a thread and there is nothing to
//! deadlock.
//!
//! **A `Mutex` and a `Condvar` rather than a `dispatch_semaphore`**, and the
//! reason is that the body, the status and the refusal have to cross the same
//! boundary as the signal: the delegate is accumulating a `Vec<u8>` that this
//! thread reads, so a lock is there whatever else is. A semaphore beside it
//! would be a second primitive saying the same thing, and
//! `Condvar::wait_timeout` already carries the deadline this call needs.
//!
//! **The deadline is the Windows arm's, restated.**
//! `timeoutIntervalForRequest` is the per-response idle timeout and takes
//! [`HttpsGet::phase_timeout`], which is what WinHTTP's four phase timeouts
//! are; `timeoutIntervalForResource` takes [`HttpsGet::budget`], which is what
//! the read loop's own deadline is over there. This thread then waits
//! `budget + phase_timeout`, because the Windows loop checks its budget
//! *before* a read and so may overshoot by one phase — "nothing here outlives
//! it by more than one phase timeout" is the sentence both arms keep.
//!
//! # The refusals, and which of them are word for word the Windows arm's
//!
//! Four sentences are shared verbatim, because they are statements about the
//! answer rather than about the stack that fetched it, and
//! `update_check_transport_tests` in `lib.rs` pins each of them in both files:
//!
//! | Sentence | When |
//! |---|---|
//! | `the server answered {status}` | any status that is not `200` |
//! | `the body is longer than {cap} bytes` | the cap |
//! | `the body did not arrive inside its budget` | the deadline |
//! | `the body is not text` | a body that is not UTF-8 |
//!
//! The rest name this stack the way the Windows arm names its own: a transport
//! failure is `NSURLSession: {what Foundation called it}` where the twin is
//! `WinHttpSendRequest: {error}`. Neither is anything a caller matches on —
//! `bt_app::update` turns every `Err` into the same silence.
//!
//! # Where the answers this module is asked about come from
//!
//! **A server on this machine's own loopback, and never `api.github.com`**
//! (T-CI-PORTABLE). Three of the cases below used to put their questions to the
//! real releases API, which made them a statement about somebody else's rate
//! limit: a shared CI runner is one of a great many machines behind one address,
//! GitHub's unauthenticated limit is per address, and the three answered `403`
//! there and `200` on a developer's Mac for two days. Each now binds a
//! `TcpListener` on `127.0.0.1`, serves one canned HTTP/1.1 response and points
//! the transport at it.
//!
//! The door itself cannot be pointed anywhere but `https://{host}{path}` — that
//! is `address`'s whole job and two cases below pin it — so the exchange is
//! split at the seam it already had: `address` answers *where to send it* and
//! `fetch` does the sending, and it is `fetch` that the loopback cases drive.
//! App Transport Security has nothing to say about a plaintext request to an IP
//! literal, which is what makes that possible at all; `ask_this_machine` in this
//! file's own tests carries what was measured, and what would name itself if it
//! ever changed.

use std::{
    ptr,
    sync::{Arc, Condvar, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use block2::DynBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_foundation::{
    NSData, NSError, NSHTTPURLResponse, NSInteger, NSMutableURLRequest, NSObject, NSObjectProtocol,
    NSString, NSURL, NSURLRequest, NSURLRequestCachePolicy, NSURLResponse, NSURLSession,
    NSURLSessionConfiguration, NSURLSessionDataDelegate, NSURLSessionDataTask,
    NSURLSessionDelegate, NSURLSessionResponseDisposition, NSURLSessionTask,
    NSURLSessionTaskDelegate, ns_string,
};

/// The one request this module knows how to make.
///
/// **The same six fields as the Windows arm, in the same order, with the same
/// names and the same types** — `update_check_transport_tests` compares the two
/// declarations as text. A struct rather than six positional arguments because
/// four of the six are strings and numbers of the same shape, and a caller that
/// transposed `host` and `path` would get a compiling program that asks the
/// wrong server.
#[derive(Clone, Copy, Debug)]
pub struct HttpsGet<'a> {
    /// The host, with no scheme and no slash: `api.github.com`.
    pub host: &'a str,
    /// The path with its leading slash, query string included.
    pub path: &'a str,
    /// The `User-Agent` this request travels under.
    ///
    /// Not optional, and not defaulted: what a request says about the program
    /// making it is exactly the kind of decision that must be visible at the
    /// call site rather than buried here. `docs/PRIVACY.md` names the string the
    /// product actually sends.
    pub user_agent: &'a str,
    /// The per-response idle timeout — WinHTTP's four phase timeouts, and
    /// `timeoutIntervalForRequest` here.
    pub phase_timeout: Duration,
    /// The whole call's own deadline — `timeoutIntervalForResource`, and this
    /// thread's own wait.
    pub budget: Duration,
    /// The most body this will read. A response longer than this is an error
    /// and not a truncation: half a JSON document is not a smaller answer, it is
    /// a different one.
    pub cap: usize,
}

/// How many redirects are followed before the answer is the redirect itself.
///
/// Ten, because ten is `WINHTTP_OPTION_MAX_HTTP_AUTOMATIC_REDIRECTS`'s default
/// and this arm's job is to be the other arm. `NSURLSession`'s own internal
/// limit is twenty, which is why this is counted here rather than left to it.
const MAX_REDIRECTS: u32 = 10;

/// **What the delegate is filling in and the caller is waiting for.**
///
/// One allocation shared by two threads: the caller holds an [`Arc`] of it
/// across the wait and the delegate object holds the other as its ivar, so the
/// state outlives whichever of the two lets go first — including a delegate
/// callback that arrives after this call has already timed out and returned.
struct Exchange {
    /// [`HttpsGet::cap`], copied in so the delegate needs no other reference.
    cap: usize,
    progress: Mutex<Progress>,
    /// Raised once, by `didCompleteWithError:`.
    finished: Condvar,
}

/// The part of the exchange two threads touch.
#[derive(Default)]
struct Progress {
    /// What `didReceiveData:` has appended, and what the caller takes.
    body: Vec<u8>,
    /// The HTTP status, once there is a response. `None` for a task that ended
    /// without one.
    status: Option<NSInteger>,
    /// The first thing that went wrong, and only the first — a task this code
    /// cancelled reports a cancellation afterwards, and the reason it was
    /// cancelled is the one worth carrying.
    refusal: Option<String>,
    /// How many redirects have been followed.
    redirects: u32,
    /// Whether `didCompleteWithError:` has run.
    done: bool,
}

impl Exchange {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            progress: Mutex::new(Progress::default()),
            finished: Condvar::new(),
        }
    }

    /// The lock, with this crate's standing sentence about a poisoned one.
    fn locked(&self) -> MutexGuard<'_, Progress> {
        self.progress
            .lock()
            .expect("the update check's exchange is not held across a panic")
    }

    /// Record the first thing that went wrong.
    fn refuse(&self, why: String) {
        let mut progress = self.locked();
        if progress.refusal.is_none() {
            progress.refusal = Some(why);
        }
    }

    /// The task is over, whatever happened to it.
    fn finish(&self) {
        self.locked().done = true;
        self.finished.notify_all();
    }

    /// **Block this thread until the delegate is done, or until the deadline.**
    ///
    /// The deadline is the one failure this side owns: every other outcome is
    /// something the delegate wrote down.
    fn wait(&self, deadline: Duration) -> Result<Vec<u8>, String> {
        let started = Instant::now();
        let mut progress = self.locked();
        while !progress.done {
            let left = deadline.saturating_sub(started.elapsed());
            if left.is_zero() {
                return Err("the body did not arrive inside its budget".to_owned());
            }
            let (next, _) = self
                .finished
                .wait_timeout(progress, left)
                .expect("the update check's exchange is not held across a panic");
            progress = next;
        }
        outcome(&mut progress)
    }
}

/// **Which of the things that happened is the answer**, as one total function.
///
/// The order is the whole of it, and only one line of it is not obvious: **a
/// status that is not `200` outranks a refusal**, because the refusal in that
/// case is this code's own cancellation coming back as `NSURLErrorCancelled` —
/// the server's number is the fact worth carrying, and it is the fact the
/// Windows arm carries.
fn outcome(progress: &mut Progress) -> Result<Vec<u8>, String> {
    match (progress.status, progress.refusal.take()) {
        (Some(status), _) if status != 200 => Err(format!("the server answered {status}")),
        (_, Some(why)) => Err(why),
        (Some(_), None) => Ok(std::mem::take(&mut progress.body)),
        (None, None) => Err("the request ended without an answer".to_owned()),
    }
}

define_class!(
    // SAFETY:
    // - `NSObject` has no subclassing requirements.
    // - This class does not implement `Drop`; its one ivar does, and the
    //   macro's generated `dealloc` runs it.
    #[unsafe(super(NSObject))]
    #[name = "FolioUpdateCheckTransport"]
    #[ivars = Arc<Exchange>]
    struct Transport;

    unsafe impl NSObjectProtocol for Transport {}

    unsafe impl NSURLSessionDelegate for Transport {}

    unsafe impl NSURLSessionTaskDelegate for Transport {
        /// **The one kind of redirect this door will not follow, and the count
        /// that bounds the ones it will.**
        ///
        /// Passing the request back follows it; passing null stops, and the
        /// `30x` becomes the response this task completes with — which is how
        /// a refused redirect turns into `the server answered 301` on both
        /// platforms rather than into two different silences.
        #[unsafe(method(URLSession:task:willPerformHTTPRedirection:newRequest:completionHandler:))]
        fn will_perform_redirection(
            &self,
            _session: &NSURLSession,
            _task: &NSURLSessionTask,
            _response: &NSHTTPURLResponse,
            request: &NSURLRequest,
            handler: &DynBlock<dyn Fn(*mut NSURLRequest)>,
        ) {
            let secure = request
                .URL()
                .and_then(|url| url.scheme())
                .is_some_and(|scheme| scheme.to_string().eq_ignore_ascii_case("https"));
            let within = {
                let mut progress = self.ivars().locked();
                progress.redirects += 1;
                progress.redirects <= MAX_REDIRECTS
            };
            // The pointer is the argument's own, unretained, for the length of
            // this call — which is what `completionHandler(request)` is in
            // Objective-C, and what the loading system expects: it retains the
            // request itself if it goes on to use it.
            let next = if secure && within {
                ptr::from_ref(request).cast_mut()
            } else {
                ptr::null_mut()
            };
            handler.call((next,));
        }

        /// The task is over. Every path through this module ends here, which is
        /// why this is the only place that wakes the caller.
        #[unsafe(method(URLSession:task:didCompleteWithError:))]
        fn did_complete_with_error(
            &self,
            _session: &NSURLSession,
            _task: &NSURLSessionTask,
            error: Option<&NSError>,
        ) {
            let exchange = self.ivars();
            if let Some(error) = error {
                exchange.refuse(format!("NSURLSession: {}", error.localizedDescription()));
            }
            exchange.finish();
        }
    }

    unsafe impl NSURLSessionDataDelegate for Transport {
        /// The status, and the decision about whether there is any point
        /// reading the body that follows it.
        ///
        /// `Cancel` for anything that is not a `200`, because the Windows arm
        /// returns the moment `WinHttpQueryHeaders` answers and never reads the
        /// body of a refusal either. The number is recorded first, so the
        /// cancellation this causes cannot become the sentence the caller
        /// sees — see [`outcome`].
        #[unsafe(method(URLSession:dataTask:didReceiveResponse:completionHandler:))]
        fn did_receive_response(
            &self,
            _session: &NSURLSession,
            _task: &NSURLSessionDataTask,
            response: &NSURLResponse,
            handler: &DynBlock<dyn Fn(NSURLSessionResponseDisposition)>,
        ) {
            let status = response
                .downcast_ref::<NSHTTPURLResponse>()
                .map(NSHTTPURLResponse::statusCode);
            self.ivars().locked().status = status;
            let verdict = if status == Some(200) {
                NSURLSessionResponseDisposition::Allow
            } else {
                NSURLSessionResponseDisposition::Cancel
            };
            handler.call((verdict,));
        }

        /// One chunk, against the cap.
        ///
        /// The cap is checked **before** the bytes are kept, so the buffer
        /// never exceeds it — the Windows arm's `body.len() + want >
        /// request.cap` in this platform's spelling.
        #[unsafe(method(URLSession:dataTask:didReceiveData:))]
        fn did_receive_data(
            &self,
            _session: &NSURLSession,
            task: &NSURLSessionDataTask,
            data: &NSData,
        ) {
            let exchange = self.ivars();
            let chunk = data.to_vec();
            let mut progress = exchange.locked();
            if progress.refusal.is_some() {
                return;
            }
            if progress.body.len().saturating_add(chunk.len()) > exchange.cap {
                progress.refusal = Some(format!("the body is longer than {} bytes", exchange.cap));
                drop(progress);
                task.cancel();
                return;
            }
            progress.body.extend_from_slice(&chunk);
        }
    }
);

// SAFETY: the class has exactly one ivar, an `Arc<Exchange>`, and every
// mutation of what it points at goes through `Exchange`'s own `Mutex`. The
// object itself is only ever retained, released and sent the four messages
// above, all of which `NSURLSession` is entitled to send from the delegate
// queue it made — which is the thread this type exists to be used from.
unsafe impl Send for Transport {}
// SAFETY: as above; `&Transport` gives access to nothing that is not behind
// that `Mutex`.
unsafe impl Sync for Transport {}

impl Transport {
    fn new(exchange: Arc<Exchange>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(exchange);
        // SAFETY: `NSObject`'s designated initializer, called on a fresh
        // allocation whose ivars are set.
        unsafe { msg_send![super(this), init] }
    }
}

/// **The address, built here and then made to prove it is the one that was
/// asked for.**
///
/// The Windows arm cannot be given a scheme: `host` goes to `WinHttpConnect`,
/// `path` goes to `WinHttpOpenRequest`, and `WINHTTP_FLAG_SECURE` is the whole
/// of the TLS decision. This arm has to *compose* a URL, and composition is
/// where a caller could smuggle one in — a `host` of `http://elsewhere` would
/// otherwise become `https://http://elsewhere…`, which is a string the parser
/// may well read as a host of `http`.
///
/// So the URL is parsed back and asked two questions: **is its scheme `https`**,
/// and **is its host the host that was asked for**. That is one general rule
/// rather than a list of forbidden characters, and it refuses a scheme, a
/// credential, a port and a second authority with the same line.
fn address(host: &str, path: &str) -> Result<Retained<NSURL>, String> {
    if host.contains('\0') || path.contains('\0') {
        return Err("the address contains an embedded NUL".to_owned());
    }
    if !path.starts_with('/') {
        return Err(format!("{path} is not a path on a host"));
    }
    let composed = format!("https://{host}{path}");
    let url = NSURL::URLWithString(&NSString::from_str(&composed))
        .ok_or_else(|| format!("{composed} is not an address this system can read"))?;
    let scheme = url.scheme().map(|scheme| scheme.to_string());
    if scheme.as_deref() != Some("https") {
        return Err(format!("{composed} is not an https address"));
    }
    let parsed = url.host().map(|parsed| parsed.to_string());
    if !parsed
        .as_deref()
        .is_some_and(|parsed| parsed.eq_ignore_ascii_case(host))
    {
        return Err(format!("{composed} does not name the host {host}"));
    }
    Ok(url)
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
    let url = address(request.host, request.path)?;
    fetch(&url, request)
}

/// **The whole of the exchange, once the address is settled** — the session,
/// the delegate, the task, the wait and the answer.
///
/// Split from [`https_get`] at exactly the seam the two halves already were:
/// *where to send it* is [`address`]'s question and is asked of the caller's
/// strings, and *sending it* is this, which is asked of a URL. Nothing is
/// relaxed by the split — `https_get` is still the only door out of this module
/// and it still composes the only address it will build — and what the seam buys
/// is that this half can be put to a server on this machine's own loopback,
/// which is how the delegate's three decisions are proved without the public
/// internet answering for them. See this module's own tests.
fn fetch(url: &NSURL, request: &HttpsGet<'_>) -> Result<String, String> {
    let exchange = Arc::new(Exchange::new(request.cap));
    let transport = Transport::new(Arc::clone(&exchange));

    let configuration = NSURLSessionConfiguration::ephemeralSessionConfiguration();
    configuration.setTimeoutIntervalForRequest(request.phase_timeout.as_secs_f64());
    configuration.setTimeoutIntervalForResource(request.budget.as_secs_f64());
    // Belt for the ephemeral session's braces: an in-memory cookie jar that
    // dies with the session is already nothing anybody keeps, and this says the
    // request does not fill one in the first place.
    configuration.setHTTPShouldSetCookies(false);
    configuration.setRequestCachePolicy(NSURLRequestCachePolicy::ReloadIgnoringLocalCacheData);
    // One request, one connection. Nothing here reuses anything.
    configuration.setHTTPMaximumConnectionsPerHost(1);

    // SAFETY: the delegate is this file's own class; the queue is `None`, which
    // is documented as "the session creates a serial operation queue", and that
    // is the thread every callback above is written for.
    let session = unsafe {
        NSURLSession::sessionWithConfiguration_delegate_delegateQueue(
            &configuration,
            Some(ProtocolObject::from_ref(&*transport)),
            None,
        )
    };

    let http = NSMutableURLRequest::requestWithURL_cachePolicy_timeoutInterval(
        url,
        NSURLRequestCachePolicy::ReloadIgnoringLocalCacheData,
        request.phase_timeout.as_secs_f64(),
    );
    http.setHTTPMethod(ns_string!("GET"));
    // The one header this request carries, and the only thing it says about the
    // program making it.
    http.setValue_forHTTPHeaderField(
        Some(&NSString::from_str(request.user_agent)),
        ns_string!("User-Agent"),
    );

    let task = session.dataTaskWithRequest(&http);
    task.resume();

    let answer = exchange.wait(request.budget + request.phase_timeout);
    // **Unconditional, on every path.** A session with a delegate retains that
    // delegate until it is invalidated, and a task this thread gave up waiting
    // for is a task that must stop. `invalidateAndCancel` is both.
    session.invalidateAndCancel();

    String::from_utf8(answer?).map_err(|_| "the body is not text".to_owned())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        time::{Duration, Instant},
    };

    use objc2_foundation::{NSString, NSURL};

    use super::{HttpsGet, fetch, https_get};

    /// The product's own numbers, so that what these cases exercise is what
    /// `bt_app::update` exercises.
    const PHASE: Duration = Duration::from_secs(5);
    const BUDGET: Duration = Duration::from_secs(15);
    const CAP: usize = 1_024 * 1_024;

    fn ask(host: &str, path: &str) -> Result<String, String> {
        https_get(&HttpsGet {
            host,
            path,
            user_agent: "Folio",
            phase_timeout: PHASE,
            budget: BUDGET,
            cap: CAP,
        })
    }

    /// **One canned HTTP/1.1 response, from a server on this machine** — bound
    /// on a loopback port of the kernel's choosing, answering the first request
    /// that arrives and then closing.
    ///
    /// **The three cases below used to ask `api.github.com`, and that is why CI
    /// was red for two days**: a shared runner is one of a great many machines
    /// behind one address, and GitHub's unauthenticated rate limit is per
    /// address — so the three came back `403` there and `200` on a developer's
    /// Mac. A test that passes or fails according to how busy somebody else's
    /// service is has stopped being a statement about this code.
    ///
    /// The head is read before the answer is written, because a server that
    /// answers and closes while the client is still writing hands some stacks a
    /// reset in place of a response.
    fn answering(response: Vec<u8>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener
            .local_addr()
            .expect("the port the kernel chose")
            .port();
        std::thread::spawn(move || {
            let Ok((mut connection, _)) = listener.accept() else {
                return;
            };
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                if connection.read(&mut byte).unwrap_or(0) != 1 {
                    return;
                }
                head.push(byte[0]);
            }
            // The client of a body longer than its cap cancels mid-transfer, so
            // the far end going away here is one of the outcomes rather than a
            // failure of the server.
            let _ = connection.write_all(&response);
            let _ = connection.flush();
        });
        port
    }

    /// The least HTTP/1.1 the client needs: a status line, a length, and a
    /// connection that closes so the end of the body is not a guess.
    fn response(status: u16, reason: &str, body: &str) -> Vec<u8> {
        let mut out = format!(
            "HTTP/1.1 {status} {reason}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        out.extend_from_slice(body.as_bytes());
        out
    }

    /// **The product's own transport, put to that server.**
    ///
    /// [`super::fetch`] and not [`https_get`], because the door composes
    /// `https://{host}{path}` and refuses anything else — a port is not part of
    /// a host and a scheme cannot be smuggled through one, which is what
    /// `a_port_in_the_host_is_refused_before_the_network` and
    /// `a_plain_http_address_is_refused_without_a_round_trip` below pin, and
    /// what makes a loopback port unreachable *through* the door. So the two
    /// halves are asked separately: those two cases are about the address, and
    /// these three are about everything the session and its delegate do once
    /// there is one.
    ///
    /// **App Transport Security does not stand in the way, and the reason is
    /// worth writing down rather than discovering twice.** ATS reads
    /// `NSAppTransportSecurity` out of the *main bundle's* `Info.plist`, and a
    /// `cargo test` binary has no bundle at all — so no exception could be
    /// declared from here even if one were wanted. It is not wanted: ATS's rules
    /// are about connections to a **domain name**, and it does not apply to a
    /// host that is an IP literal (which is the gap `NSAllowsLocalNetworking`
    /// exists to cover for names like `localhost` and `*.local`). The address
    /// below is `127.0.0.1`, so a plaintext request to it is not a request ATS
    /// has anything to say about. If that ever stops being true, these three
    /// fail together with `NSURLSession: … App Transport Security policy
    /// requires the use of a secure connection`, which names itself.
    fn ask_this_machine(port: u16, path: &str, cap: usize) -> Result<String, String> {
        let composed = format!("http://127.0.0.1:{port}{path}");
        let url = NSURL::URLWithString(&NSString::from_str(&composed))
            .expect("a loopback address this system can read");
        fetch(
            &url,
            &HttpsGet {
                host: "127.0.0.1",
                path,
                user_agent: "Folio",
                phase_timeout: PHASE,
                budget: BUDGET,
                cap,
            },
        )
    }

    /// **A releases list, fetched and handed back whole** — the `Ok` path, and
    /// the only case here that runs the delegate all the way to a body.
    ///
    /// The response is the shape `bt_app::update::GitHubReleases` parses: a JSON
    /// array, served with a length and a close. What it proves is that a `200`
    /// is allowed to proceed, that every chunk the delegate is handed reaches
    /// the caller in order, and that the bytes become a `String` — which is the
    /// whole of what this transport owes the update check.
    ///
    /// MUTATION: cancel on a `200` in `didReceiveResponse:`, or drop a chunk in
    /// `didReceiveData:`, and this stops being a JSON array.
    #[test]
    fn the_releases_list_comes_back_as_json() {
        const RELEASES: &str = r#"[{"tag_name":"v0.3.0","name":"Folio 0.3.0","draft":false}]"#;
        let port = answering(response(200, "OK", RELEASES));
        let body = ask_this_machine(port, "/repos/lulu-loopp/folio-terminal/releases", CAP)
            .expect("the releases list");
        assert_eq!(body, RELEASES, "the body arrived whole and in order");
        assert!(
            body.trim_start().starts_with('['),
            "the release list is a JSON array: {}",
            &body[..body.len().min(120)]
        );
        assert!(body.len() < CAP, "and it fits inside the cap");
    }

    /// **A port is not part of a host**, and the refusal comes before a socket.
    ///
    /// `127.0.0.1:1` is the shape somebody reaches for when they want to point
    /// this door at a closed port, and [`super::address`] answers it without
    /// asking the network anything — which is the same answer it gives a
    /// smuggled scheme, and for the same reason.
    #[test]
    fn a_port_in_the_host_is_refused_before_the_network() {
        let started = Instant::now();
        let refusal = ask("127.0.0.1:1", "/").expect_err("a port is not part of a host");
        assert!(
            refusal.contains("does not name the host"),
            "a port in the host is not a host: {refusal}"
        );
        assert!(started.elapsed() < Duration::from_secs(1), "{refusal}");
    }

    /// **The `Err` path a machine with no answer takes**, against a host that is
    /// certainly there and a port that is certainly closed.
    ///
    /// Nothing listens on `127.0.0.1:443` on a stock macOS, so this is a
    /// connection refusal rather than a timeout — the failure an offline machine
    /// actually has, and the one the settings row's *could not check* state is
    /// for. The assertion on the clock is the important half: a door that
    /// answered only after the whole budget would carry the same `Err` and be a
    /// very different feature.
    #[test]
    fn a_closed_port_is_refused_inside_the_deadline() {
        let started = Instant::now();
        let refusal = ask("127.0.0.1", "/").expect_err("nothing is listening on 443 here");
        assert!(
            refusal.starts_with("NSURLSession: "),
            "the transport is what refused: {refusal}"
        );
        assert!(
            started.elapsed() < BUDGET + PHASE,
            "the refusal is inside the deadline: {:?}",
            started.elapsed()
        );
    }

    /// **`http://` never leaves this function**, and it is refused without a
    /// round trip.
    ///
    /// [`HttpsGet`] has no scheme to set, so the way a plain `http://` address
    /// could reach the wire is through the one string this arm composes: a
    /// `host` carrying a scheme of its own. The parse-back in
    /// [`super::address`] is what stops it, and it stops it before a socket
    /// exists — which is the point, because a request refused *after* it has
    /// been sent has already put the agent and the path on a plaintext channel.
    ///
    /// MUTATION: delete either check in `address` and one of these comes back
    /// `Ok` or waits on a real server.
    #[test]
    fn a_plain_http_address_is_refused_without_a_round_trip() {
        let started = Instant::now();
        for host in [
            "http://example.com",
            "example.com:80",
            "user@example.com",
            "example.com/../elsewhere.test",
            "",
        ] {
            let refusal = ask(host, "/anything").expect_err(host);
            assert!(
                refusal.contains("is not an https address")
                    || refusal.contains("does not name the host")
                    || refusal.contains("is not an address this system can read"),
                "{host}: {refusal}"
            );
        }
        // A path that is not one is the same refusal from the other side.
        assert!(
            ask("example.com", "http://elsewhere.test/")
                .expect_err("a path starts with a slash")
                .contains("is not a path on a host")
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "none of those touched the network: {:?}",
            started.elapsed()
        );
    }

    /// **The cap is a refusal and not a truncation**, against a body served
    /// with a cap small enough that the first chunk passes it.
    ///
    /// The `200` is the point: the server allows the transfer, the delegate
    /// starts it, and the decision is taken **while it is still running** —
    /// which is why the cap is checked in `didReceiveData:` and not after the
    /// body has arrived. The sentence is the Windows arm's word for word.
    ///
    /// MUTATION: turn the cap check in `didReceiveData:` into a truncation and
    /// this comes back `Ok` with a short body.
    #[test]
    fn a_body_longer_than_the_cap_is_an_error() {
        let long = format!("[{}]", "\"v0.3.0\",".repeat(40));
        assert!(long.len() > 64, "the body has to be longer than the cap");
        let port = answering(response(200, "OK", &long));
        let refusal = ask_this_machine(port, "/repos/lulu-loopp/folio-terminal/releases", 64)
            .expect_err("64 bytes is not a release list");
        assert_eq!(refusal, "the body is longer than 64 bytes");
    }

    /// **A status that is not `200` is carried as the number**, in the sentence
    /// the Windows arm uses.
    ///
    /// `404` is what a repository that is not there answers with, and the two
    /// things worth having proved are both about what happens *after* the
    /// number: the delegate cancels rather than reading a body it has no use
    /// for, and the cancellation that causes does not become the sentence the
    /// caller sees — the server's number outranks it (see [`super::outcome`]).
    ///
    /// MUTATION: let `outcome` prefer the refusal and this comes back
    /// `NSURLSession: cancelled`.
    #[test]
    fn a_status_that_is_not_200_comes_back_as_the_number() {
        let port = answering(response(404, "Not Found", r#"{"message":"Not Found"}"#));
        let refusal = ask_this_machine(
            port,
            "/repos/lulu-loopp/folio-terminal-not-a-repo/releases",
            CAP,
        )
        .expect_err("there is no such repository");
        assert_eq!(refusal, "the server answered 404");
    }
}
