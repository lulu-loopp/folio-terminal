//! **One `GET`, before there is a stack to make it with** (M4-10).
//!
//! WinHTTP on Windows, `NSURLSession` on macOS, and the reason this is a
//! platform door at all rather than a Rust HTTP client is written in the
//! Windows arm's own header: what the operating system's stack buys is its TLS,
//! its certificate store and its proxy configuration — including the PAC file a
//! managed laptop is handed — rather than forty packages and a trust store of
//! our own.
//!
//! `bt-app`'s one caller is the update check, and it already names this module
//! only inside `#[cfg(windows)]` (`update.rs:376`, one of the eleven files
//! §4.3 lists). The module exists here anyway so that the crate's shape does
//! not depend on which arm a caller happens to be in, and M4-10 fills it.

use std::time::Duration;

/// One request, described.
pub struct HttpsGet<'a> {
    /// The host to ask.
    pub host: &'a str,
    /// The path on it.
    pub path: &'a str,
    /// What this build calls itself.
    pub user_agent: &'a str,
    /// How long any one phase may take.
    pub phase_timeout: Duration,
    /// How long the whole request may take.
    pub budget: Duration,
    /// The most bytes that will be read back.
    pub cap: usize,
}

/// Make the request. Refused; M4-10.
///
/// `bt_app::update::latest_tag` turns the `Err` into the settings row's own
/// *could not check* state, which is a sentence a reader can act on rather than
/// a version number that silently stopped moving.
pub fn https_get(request: &HttpsGet<'_>) -> Result<String, String> {
    let _ = request;
    Err("this build has no HTTP stack".to_owned())
}
