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
//! Not a client. There is no redirect following, no keep-alive, no connection
//! reuse, no POST, no request body, no header the caller can name, and no
//! `http://`. It is one function, it is shaped like the one call the product
//! makes, and the next caller that needs something else should widen it on
//! purpose rather than find it already widened.
//!
//! # The bound on how long it can take
//!
//! WinHTTP's four timeouts are **per phase** — resolve, connect, send, receive
//! — so setting all four to five seconds bounds four separate waits and not the
//! whole. A body that arrives one byte at a time is inside the receive timeout
//! forever. So the loop that reads it carries a deadline of its own
//! ([`HttpsGet::budget`]), and that deadline is the number a caller can reason
//! about: nothing here outlives it by more than one phase timeout.

use std::{
    ffi::c_void,
    time::{Duration, Instant},
};

use windows::{
    Win32::Networking::WinHttp::{
        INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
        WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
        WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpQueryHeaders,
        WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    },
    core::PCWSTR,
};

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
