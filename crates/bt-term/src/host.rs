//! **What the program around this crate answers for it** — the facts about the machine a terminal
//! session needs and does not ask the platform for itself (`docs/ARCHITECTURE.md` §3.2).
//!
//! `bt-term` builds without the platform layer: a browser build references it, and there is no
//! operating system behind it there to name a host or set a thread's priority. So the host
//! program installs these answers once, at its start, before its first session:
//!
//! * [`install_host_names`] — the names this machine answers to, which a `file://<host>/` report
//!   is compared against ([`local_host_names`]);
//! * [`install_pool_thread_start`] — what every thread of the image resample pool runs first
//!   (Folio's desktop build puts the thread in the band below normal);
//! * [`install_svg_rasterizer`] — the codec an inline image whose bytes no raster container claims
//!   is handed to (Folio's desktop build installs `bt_math::rasterize_svg_document`; this crate
//!   names no rasterizer).
//!
//! **Each is one answer per process.** Installing the same host names or the same codec again is
//! a no-op; installing different ones is a panic, and so is a second thread-start hook. A read of
//! the host names before anything was installed panics in every build profile: a session that
//! quietly read "no names" would take every `file://<host>/` report from this machine for a remote
//! share, which is the defect the installed fact exists to close (B-AUDIT-046 TRM-3). A read of
//! the codec before an installation panics the same way: a decoder that quietly had none would
//! call every SVG an unsupported format, and the host that forgot the install would never be told.
//! The thread-start hook alone is optional: a host that installs none (a browser) gets a pool with
//! no hook. The names and the codec are not: every host, a browser included, installs both.
//!
//! The fourth answer a host gives this crate — the finished name a hand-off door would open — is
//! not installed: it is handed to [`crate::verify_path`] by the worker that calls it.

use std::sync::OnceLock;

use bt_doc::svg::{SvgRaster, SvgRasterError};

/// The panic a read of the host names makes when no host has installed them.
pub const HOST_NAMES_READ_BEFORE_INSTALL: &str = "host names read before the host installed them";

/// The names a test process installs: one name no machine is given (`.invalid` is reserved for
/// exactly that, RFC 2606), so a test that reads it can only have been handed it.
pub const TEST_HOST_NAMES: &[&str] = &["folio-test-host.invalid"];

static HOST_NAMES: OnceLock<Vec<String>> = OnceLock::new();

/// `None` once read with nothing installed: the pool was built without a hook, and a hook
/// installed after that would never run.
static POOL_THREAD_START: OnceLock<Option<fn()>> = OnceLock::new();

/// **Install this machine's names** — every spelling a shell on it may put in the authority of a
/// `file://<host>/path` report. Called by the host before its first session.
///
/// # Panics
///
/// When different names were installed before: the process has one answer.
pub fn install_host_names(names: Vec<String>) {
    let Err(offered) = HOST_NAMES.set(names) else {
        return;
    };
    assert!(
        HOST_NAMES.get() == Some(&offered),
        "host names installed twice with different values: {:?}, then {offered:?}",
        HOST_NAMES.get()
    );
}

/// [`TEST_HOST_NAMES`], installed: what a test process calls in place of the host's own answer.
/// Equal values install idempotently, so every test of a process may call it.
pub fn install_test_host_names() {
    install_host_names(
        TEST_HOST_NAMES
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
    );
}

/// This machine's names, as the host installed them — the authorities a `file://` URI may carry
/// besides none and `localhost`.
///
/// **Installed by the host, never read from an environment variable** (B-AUDIT-046 TRM-3). The
/// reader used to read `COMPUTERNAME`, which exists only on Windows: on a Mac it answered nothing,
/// so every OSC 7 of the form `file://<host>/path` — fish's own report, Apple's
/// `zshrc_Apple_Terminal`, `vte.sh` — was taken for a remote share and the pane forgot its
/// directory.
///
/// This crate's own unit tests install [`TEST_HOST_NAMES`] here, in the one place every read
/// passes, so no test of the crate can read before an installation.
///
/// # Panics
///
/// With [`HOST_NAMES_READ_BEFORE_INSTALL`] when nothing was installed, in every build profile.
pub fn local_host_names() -> &'static [String] {
    #[cfg(test)]
    install_test_host_names();
    HOST_NAMES.get().expect(HOST_NAMES_READ_BEFORE_INSTALL)
}

/// **Install what every thread of the image resample pool runs first.** Called by the host before
/// its first session; a host that installs nothing gets a pool whose threads run no hook.
///
/// # Panics
///
/// When a hook was installed before, or when the pool was already built without one.
pub fn install_pool_thread_start(hook: fn()) {
    assert!(
        POOL_THREAD_START.set(Some(hook)).is_ok(),
        "the resample pool's thread-start hook was installed twice, or after the pool was built \
         without one"
    );
}

/// The installed thread-start hook, if any. Reading it settles the answer for the process.
pub(crate) fn pool_thread_start() -> Option<fn()> {
    *POOL_THREAD_START.get_or_init(|| None)
}

/// **The SVG codec**: rasterize a standalone SVG document at its intrinsic size, in straight
/// (unpremultiplied) sRGB RGBA. `Parse` is bytes that are not an SVG document (the decoder answers
/// [`crate::InlineImageDecodeError::UnsupportedFormat`]); `Dimensions` is a document whose
/// intrinsic size the codec refuses ([`crate::InlineImageDecodeError::InvalidDimensions`]).
pub type SvgRasterizer = fn(&[u8]) -> Result<SvgRaster, SvgRasterError>;

/// The panic a read of the SVG codec makes when no host has installed one.
pub const SVG_RASTERIZER_READ_BEFORE_INSTALL: &str =
    "the SVG rasterizer read before the host installed it";

/// The one document [`test_svg_rasterizer`] answers with a raster: three by two user units, one
/// half-transparent fill.
pub const TEST_SVG_DOCUMENT: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="3" height="2"><rect width="3" height="2" fill="#12a4e6" fill-opacity="0.5"/></svg>"##;

/// Every pixel of the raster [`test_svg_rasterizer`] answers [`TEST_SVG_DOCUMENT`] with, in
/// straight alpha.
pub const TEST_SVG_PIXEL: [u8; 4] = [0x12, 0xa4, 0xe6, 0x80];

static SVG_RASTERIZER: OnceLock<SvgRasterizer> = OnceLock::new();

/// **Install the SVG codec** every inline-image decode of this process hands an SVG payload to.
/// Called by the host before its first session.
///
/// # Panics
///
/// When a different codec was installed before: the process has one answer.
pub fn install_svg_rasterizer(codec: SvgRasterizer) {
    let Err(offered) = SVG_RASTERIZER.set(codec) else {
        return;
    };
    let installed = *SVG_RASTERIZER
        .get()
        .expect("a codec that could not be set is the one installed before");
    assert!(
        std::ptr::fn_addr_eq(installed, offered),
        "the SVG rasterizer installed twice with different codecs"
    );
}

/// The installed SVG codec.
///
/// This crate's own unit tests install [`test_svg_rasterizer`] here, in the one place every read
/// passes, so no test of the crate can read before an installation.
///
/// # Panics
///
/// With [`SVG_RASTERIZER_READ_BEFORE_INSTALL`] when nothing was installed, in every build profile.
pub(crate) fn svg_rasterizer() -> SvgRasterizer {
    #[cfg(test)]
    install_test_svg_rasterizer();
    *SVG_RASTERIZER
        .get()
        .expect(SVG_RASTERIZER_READ_BEFORE_INSTALL)
}

/// The codec a test process installs in place of the host's: [`TEST_SVG_DOCUMENT`] is a three by
/// two raster of [`TEST_SVG_PIXEL`], and every other payload is not an SVG document. It parses
/// nothing, so a test that reads that raster back can only have been handed it by this codec.
///
/// # Errors
///
/// [`SvgRasterError::Parse`] for every payload but [`TEST_SVG_DOCUMENT`].
pub fn test_svg_rasterizer(bytes: &[u8]) -> Result<SvgRaster, SvgRasterError> {
    if bytes != TEST_SVG_DOCUMENT {
        return Err(SvgRasterError::Parse(
            "not the test codec's one document".to_owned(),
        ));
    }
    Ok(SvgRaster {
        rgba: TEST_SVG_PIXEL.repeat(3 * 2),
        width_px: 3,
        height_px: 2,
    })
}

/// [`test_svg_rasterizer`], installed: what a test process calls in place of the host's codec.
/// The same codec installs as nothing the second time, so every test of a process may call it.
pub fn install_test_svg_rasterizer() {
    install_svg_rasterizer(test_svg_rasterizer);
}
