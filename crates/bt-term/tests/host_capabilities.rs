//! **What the host installs into this crate, in a process that installed nothing before**
//! (`docs/ARCHITECTURE.md` §3.2, CC-4).
//!
//! The installed facts are process-wide, so their contract can only be read in a process of its
//! own: an integration-test binary links the library built without `cfg(test)`, where nothing is
//! installed for it. Each test here touches one fact, so they may run side by side.
//!
//! The panic of a read before installation is a contract of every build profile, release
//! included. This binary is run under the release profile as well:
//! `cargo test --release -p bt-term --test host_capabilities`.

#![allow(clippy::disallowed_methods)]

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex},
};

/// The message a panic carried.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
        })
        .unwrap_or_default()
}

fn names(spellings: &[&str]) -> Vec<String> {
    spellings.iter().map(|name| (*name).to_owned()).collect()
}

/// RED (CC-4) — **a read of the host names before the host installed them panics, with its named
/// message, in every build profile; the same names again install as nothing, and different names
/// are a panic**: one answer per process.
///
/// MUTATION ①: answer an empty list from `local_host_names` when nothing is installed (or turn the
/// `expect` into a `debug_assert!` with an empty answer) — the first assertion goes red, under
/// `--release` for the `debug_assert!` form. MUTATION ②: let a second installation replace the
/// first — the last assertion goes red.
#[test]
fn host_names_are_read_only_after_the_host_installed_them_and_installed_once() {
    let early = catch_unwind(bt_term::local_host_names)
        .expect_err("a read before the host installed the names panics");
    assert_eq!(
        panic_message(early.as_ref()),
        bt_term::HOST_NAMES_READ_BEFORE_INSTALL,
        "and says what happened"
    );
    assert_eq!(
        bt_term::HOST_NAMES_READ_BEFORE_INSTALL,
        "host names read before the host installed them"
    );

    let machine = names(&["studio-\u{e9}t\u{e9}", "studio-\u{e9}t\u{e9}.example.lan"]);
    bt_term::install_host_names(machine.clone());
    bt_term::install_host_names(machine.clone());
    assert_eq!(
        bt_term::local_host_names(),
        machine.as_slice(),
        "the names as installed, once however often they are offered"
    );

    let other = catch_unwind(|| bt_term::install_host_names(names(&["another-machine"])))
        .expect_err("different names are a second answer, and a process has one");
    assert!(
        panic_message(other.as_ref()).contains("installed twice with different values"),
        "{}",
        panic_message(other.as_ref())
    );
    assert_eq!(bt_term::local_host_names(), machine.as_slice());
}

static STARTED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn record_the_thread() {
    let name = std::thread::current().name().unwrap_or_default().to_owned();
    STARTED.lock().unwrap().push(name);
}

/// RED (CC-4) — **a resample big enough for the pool runs on threads that ran the host's hook
/// first, and the hook is installed once.**
///
/// MUTATION: build the pool without reading the installed hook (`build_resample_pool(None)` in
/// `resample_pool`) — no thread records itself and the first assertion goes red.
#[test]
fn the_resample_pool_runs_the_hook_the_host_installed() {
    bt_term::install_pool_thread_start(record_the_thread);
    // 1600×1200 into 400×300: the work, not the picture, decides the pool (`worth_the_machine`).
    let (width, height) = (1600u32, 1200u32);
    let task = bt_term::InlineImageScaleTask {
        occurrence_id: 1,
        content_key: "image:cc4-pool".to_owned(),
        rgba: Arc::from(vec![0x7f_u8; (width * height * 4) as usize]),
        width_px: width,
        height_px: height,
        display_width_px: 400,
        display_height_px: 300,
    };
    let scaled = bt_term::scale_inline_image(&task);
    assert_eq!((scaled.width_px, scaled.height_px), (400, 300));

    let started = STARTED.lock().unwrap().clone();
    assert!(
        !started.is_empty(),
        "the pool's threads ran the installed hook before their work"
    );
    assert!(
        started
            .iter()
            .all(|name| name.starts_with("bt-image-resample-")),
        "and only the pool's threads did: {started:?}"
    );

    let again = catch_unwind(AssertUnwindSafe(|| {
        bt_term::install_pool_thread_start(record_the_thread);
    }))
    .expect_err("a second hook is a second answer");
    assert!(
        panic_message(again.as_ref()).contains("installed twice"),
        "{}",
        panic_message(again.as_ref())
    );
}

/// The picture an OSC 1337 inline image carrying `document` decodes to.
fn decode_printed(
    document: &[u8],
) -> Result<bt_term::DecodedInlineImage, bt_term::InlineImageDecodeError> {
    use base64::Engine as _;
    bt_term::decode_inline_image(bt_term::InlineImageTask {
        occurrence_id: 7,
        source: bt_term::InlineImageSource::Osc1337(
            base64::engine::general_purpose::STANDARD
                .encode(document)
                .into_bytes(),
        ),
    })
}

/// A codec that answers every document with one known 2x1 raster — not the test codec, so the
/// picture read back below can only be this one's.
fn two_pixel_codec(
    _document: &[u8],
) -> Result<bt_doc::svg::SvgRaster, bt_doc::svg::SvgRasterError> {
    Ok(bt_doc::svg::SvgRaster {
        rgba: vec![0xe6, 0x12, 0xa4, 0x40, 0x01, 0x02, 0x03, 0xff],
        width_px: 2,
        height_px: 1,
    })
}

/// RED (CC-7) — **an SVG decode before the host installed a codec panics, with its named message,
/// in every build profile; the installed codec is what decodes an inline SVG; the same codec
/// again installs as nothing, and a different one is a panic**: one answer per process.
///
/// MUTATION ①: answer `UnsupportedFormat` from `decode_svg_bytes` when nothing is installed (or
/// turn the reader's `expect` into a `debug_assert!` with that answer) — the first assertion goes
/// red, under `--release` for the `debug_assert!` form. MUTATION ②: let a second, different
/// installation replace the first — the last assertion goes red.
#[test]
fn the_svg_codec_is_read_only_after_the_host_installed_it_and_installed_once() {
    let early = catch_unwind(|| decode_printed(bt_term::TEST_SVG_DOCUMENT))
        .expect_err("an SVG decode before the host installed a codec panics");
    assert_eq!(
        panic_message(early.as_ref()),
        bt_term::SVG_RASTERIZER_READ_BEFORE_INSTALL,
        "and says what happened"
    );
    assert_eq!(
        bt_term::SVG_RASTERIZER_READ_BEFORE_INSTALL,
        "the SVG rasterizer read before the host installed it"
    );

    bt_term::install_svg_rasterizer(two_pixel_codec);
    bt_term::install_svg_rasterizer(two_pixel_codec);
    let decoded = decode_printed("<svg>\u{3b1}\u{e9}</svg>".as_bytes())
        .expect("the installed codec answered a raster");
    assert_eq!((decoded.width_px, decoded.height_px), (2, 1));
    assert_eq!(
        &decoded.rgba[..],
        &[0xe6, 0x12, 0xa4, 0x40, 0x01, 0x02, 0x03, 0xff],
        "the installed codec's raster, straight alpha as it answered it"
    );

    let other = catch_unwind(|| bt_term::install_svg_rasterizer(bt_term::test_svg_rasterizer))
        .expect_err("a different codec is a second answer, and a process has one");
    assert!(
        panic_message(other.as_ref()).contains("installed twice with different codecs"),
        "{}",
        panic_message(other.as_ref())
    );
    assert_eq!(
        decode_printed(bt_term::TEST_SVG_DOCUMENT)
            .expect("the first codec still answers")
            .width_px,
        2
    );
}
