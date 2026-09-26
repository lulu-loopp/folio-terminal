//! **One `GET`, on a platform with no stack to make it with** (M4-10).
//!
//! WinHTTP on Windows (`http.rs`), `NSURLSession` on macOS (`macos_http.rs`),
//! and the reason this is a platform door at all rather than a Rust HTTP client
//! is written in the Windows arm's own header: what the operating system's
//! stack buys is its TLS, its certificate store and its proxy configuration —
//! including the PAC file a managed laptop is handed — rather than forty
//! packages and a trust store of our own.
//!
//! **This is now the third arm and not the unwritten one.** Since M4-10 the two
//! platforms Folio ships on both answer this call; what is left here is the
//! Linux build, which has neither stack and is not a product. `bt-app`'s one
//! caller — the update check — names this module with no `cfg` at all, and the
//! `Err` below is what turns into the settings row's own *could not check*
//! state there.

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

pub use crate::https_download::{
    DownloadError, DownloadMonitor, DownloadProgress, DownloadStage, Downloaded, HttpsDownload,
};

/// Stream a file. Refused, for the reason `https_get` is: this build has no
/// HTTP stack. Nothing is written.
///
/// # Errors
///
/// Always, at [`DownloadStage::Connect`].
pub fn https_download(request: &HttpsDownload<'_>) -> Result<Downloaded, DownloadError> {
    let _ = request;
    Err(DownloadError::at(
        DownloadStage::Connect,
        "this build has no HTTP stack",
    ))
}
