//! **The release manifest is read out of the `folio.exe` this workspace builds, as a data file,
//! and is the manifest `build.rs` embedded** (0.4.6 ticket U-14; moved here by
//! T-TEST-ORDER-STRAYS).
//!
//! E-14's experiment: `LoadLibraryExW` with `LOAD_LIBRARY_AS_DATAFILE |
//! LOAD_LIBRARY_AS_IMAGE_RESOURCE` maps the built executable, which is never started, and
//! `FindResourceW` finds `FOLIO_RELEASE_MANIFEST` — the door `update_archive::EmbeddedManifest`
//! is (`bt_platform::pe_resource::read_rcdata` under
//! `bt_winres::release_manifest::RESOURCE_NAME`).
//!
//! **Why an integration target.** The executable is located by `CARGO_BIN_EXE_folio`, which
//! cargo sets only for an integration test, and for an integration test cargo builds the bin
//! first. So the file this reads exists and is the build of this checkout on every run that
//! selects it. As a unit test of the bin it read `target/<profile>/folio.exe`, which exists only
//! when something else in the run made cargo build the bin: red under a
//! `cargo test --bin folio <filter>` with no bin built, and a stale earlier build when one was
//! left there. The archive half of the old test — this build's own members through
//! `update_archive::expand` — stays beside the reader, fed the embedded manifest's text
//! (`update_archive::tests::this_builds_own_members_expand_whole_under_its_embedded_manifest`).
//! The product's own door, `update_archive::EmbeddedManifest` (crate-private to the bin, so not
//! reachable from here), is held on a real executable beside it: the bin's unit-test executable
//! is linked with the same resource (`rustc-link-arg-bins` reaches every unit of the bin target),
//! and `update_archive::tests::the_product_door_reads_the_manifest_out_of_this_test_executable`
//! reads it through the door under the product cap.
//!
//! Off Windows no build carries a manifest and the door has no loader.
//!
//! MUTATION: read the resource under another name and the door answers `NotFound`.

#![allow(clippy::disallowed_methods)]

use std::path::Path;

use bt_winres::release_manifest::RESOURCE_NAME;

#[test]
fn the_manifest_is_read_out_of_a_built_folio_exe_without_running_it() {
    let executable = Path::new(env!("CARGO_BIN_EXE_folio"));
    if cfg!(not(windows)) {
        let error = bt_platform::pe_resource::read_rcdata(executable, RESOURCE_NAME, 65_536)
            .expect_err("no loader off Windows");
        assert_eq!(error.kind(), std::io::ErrorKind::Unsupported, "{error}");
        return;
    }
    let Some(path) = option_env!("FOLIO_RELEASE_MANIFEST") else {
        panic!("a Windows build always embeds the release manifest");
    };
    let embedded = std::fs::read(path).expect("build.rs wrote the manifest it embedded");
    // The cap is the embedded text's own length: the door must hand back exactly that many
    // bytes. That the product's own cap admits the text is pinned beside the product
    // (`update_archive::tests::the_product_cap_admits_this_builds_manifest`).
    let read = bt_platform::pe_resource::read_rcdata(executable, RESOURCE_NAME, embedded.len())
        .unwrap_or_else(|error| panic!("{}: {error}", executable.display()));
    assert_eq!(
        String::from_utf8(read).expect("the resource is UTF-8"),
        String::from_utf8(embedded).expect("the manifest is UTF-8"),
        "the door reads what build.rs embedded"
    );
}
