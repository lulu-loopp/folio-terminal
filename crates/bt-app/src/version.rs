//! **Which build this is** — said once, and said the same way everywhere it is
//! said.
//!
//! # The four places
//!
//! A preview release has to be identifiable from four different directions, and
//! the whole risk is that they drift:
//!
//! 1. `folio --version`, typed by the person who has it.
//! 2. The `VERSIONINFO` block Explorer shows on the executable's Properties
//!    page — which is the only one of the four a user can read without running
//!    the program at all.
//! 3. `%TEMP%\folio-panic.log`, `diagnostics.log`, and each hang report: the
//!    three files a bug report arrives as.
//! 4. The `Cargo.toml` the build came from.
//!
//! The fourth is the source and the other three are derived. `Cargo.toml`'s
//! `[workspace.package] version` becomes `CARGO_PKG_VERSION`; [`VERSION`] is
//! that, `build.rs` compiles that same string into the PE resource, and
//! [`banner`] is the one sentence the other three print. There is no second
//! literal anywhere, and
//! [`the_version_is_the_manifests_and_nothing_elses`](tests) is the gate that
//! says so.
//!
//! # Why the commit is here too
//!
//! A version answers *which release*; between a tag and the next one there are
//! a hundred builds that all answer `0.1.0`. What a panic log from a preview is
//! actually asked is *which build*, and only the hash answers that. It comes
//! from `build.rs` and is `unknown` when there was no git to ask — which is a
//! real state (a source tarball, an exported tree) and not a failure.

/// The workspace's version, which is the product's.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The commit this binary was built from, short, or `unknown`.
///
/// See `build.rs`: it is read at compile time and the crate is rebuilt when
/// `HEAD` moves, so it cannot go stale while the binary stays.
pub const COMMIT: &str = env!("FOLIO_COMMIT");

/// **The one sentence.** `Folio 0.1.0 (0a1b2c3d4e)`.
///
/// What `--version` answers, and the first line of all three diagnostic files.
/// One shape rather than three, because the reader of a bug report is comparing
/// them: a panic log that spelled the build differently from the `--version` in
/// the same report would have to be read twice to see they agree.
#[must_use]
pub fn banner() -> String {
    format!("{} {VERSION} ({COMMIT})", crate::APP_NAME)
}

#[cfg(test)]
mod tests {
    use super::{COMMIT, VERSION, banner};

    /// The `VERSIONINFO` resource `build.rs` produced for this very build.
    ///
    /// Read from the build script's own output rather than from a rebuilt copy,
    /// so what this test inspects is the bytes that were handed to the linker.
    const RESOURCE: &[u8] = include_bytes!(env!("FOLIO_VERSION_RESOURCE"));

    /// The version line out of the workspace manifest, read as text.
    ///
    /// Deliberately a dumb reader and not a TOML parse: the thing being checked
    /// is that a person editing that file edits the one line everything else
    /// follows, and a parser that understood inheritance would happily agree
    /// with itself.
    fn manifest_version() -> String {
        let manifest =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml"))
                .expect("the workspace manifest is two directories up from this crate");
        let table = manifest
            .split("[workspace.package]")
            .nth(1)
            .expect("the workspace manifest has a [workspace.package] table");
        let line = table
            .lines()
            .find(|line| line.trim_start().starts_with("version"))
            .expect("[workspace.package] declares a version");
        line.split('"')
            .nth(1)
            .expect("the version is a quoted string")
            .to_owned()
    }

    /// PIN — **one version, in four places, from one line.**
    ///
    /// The manifest, `--version`, the PE resource, and the header of every
    /// diagnostic file. Red gate: write `0.1.0` into any one of the four by hand
    /// and this passes until the next release moves the other three, which is
    /// precisely when nobody is looking. It has happened to this repository
    /// already in a smaller way — see the `Cargo.toml` note the workspace
    /// version now carries.
    #[test]
    fn the_version_is_the_manifests_and_nothing_elses() {
        assert_eq!(
            VERSION,
            manifest_version(),
            "the binary's version is the workspace manifest's"
        );

        let line = banner();
        assert!(line.starts_with("Folio "), "{line}");
        assert!(line.contains(VERSION), "{line}");
        assert!(line.contains(COMMIT), "{line}");

        // The resource holds its strings as UTF-16, which is why this looks for
        // the version spelled that way rather than as bytes.
        let wide = RESOURCE
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        let text = String::from_utf16_lossy(&wide);
        assert!(
            text.contains(VERSION),
            "the PE VERSIONINFO block carries the same version"
        );
        assert!(
            text.contains("folio.exe"),
            "and says which file it belongs to"
        );

        // And the four numbers beside those strings, which are what a script
        // reading `(Get-Item folio.exe).VersionInfo` actually gets.
        let numbers = bt_winres_file_version(RESOURCE);
        let manifest = VERSION
            .split(['-', '+'])
            .next()
            .expect("a version has a leading core")
            .split('.')
            .map(|field| field.parse::<u16>().expect("three numbers"))
            .collect::<Vec<_>>();
        assert_eq!(
            numbers,
            [manifest[0], manifest[1], manifest[2], 0],
            "VS_FIXEDFILEINFO packs the same three numbers"
        );
    }

    /// `dwFileVersionMS` and `dwFileVersionLS` out of the resource, unpacked.
    ///
    /// Found by the signature rather than by walking the container, because what
    /// is being checked here is the *content* — the container has its own tests
    /// in `bt-winres`, and a reader written twice would only ever agree with
    /// itself.
    fn bt_winres_file_version(resource: &[u8]) -> [u16; 4] {
        let signature = 0xFEEF_04BDu32.to_le_bytes();
        let at = resource
            .windows(4)
            .position(|window| window == signature)
            .expect("the resource carries a VS_FIXEDFILEINFO");
        let word = |offset: usize| {
            u32::from_le_bytes([
                resource[at + offset],
                resource[at + offset + 1],
                resource[at + offset + 2],
                resource[at + offset + 3],
            ])
        };
        let high = word(8);
        let low = word(12);
        [
            (high >> 16) as u16,
            (high & 0xFFFF) as u16,
            (low >> 16) as u16,
            (low & 0xFFFF) as u16,
        ]
    }

    /// PIN — **every file this build writes says which build wrote it.**
    ///
    /// The three are not interchangeable and all three are shipped: a panic log
    /// is what a crash leaves, `diagnostics.log` is what a run leaves, and a
    /// hang report is what a wedged window leaves. A bug report that arrives
    /// with any one of them has to be attributable without asking the reporter
    /// which build they had — which they will not know.
    ///
    /// MUTATION: drop [`banner`] from any of the three writers and this fails on
    /// that one.
    #[test]
    fn every_diagnostic_file_carries_the_build_that_wrote_it() {
        let line = banner();

        let panic_log = crate::panic_report(1_700_000_000_000, "main", "it went wrong", "");
        assert!(panic_log.contains(&line), "the panic log: {panic_log}");

        let run = crate::diagnostics::run_header("2026-08-27T01:02:03.004Z", 4242);
        assert!(run.contains(&line), "the diagnostics log: {run}");
        assert!(run.contains("4242"), "and which process wrote it: {run}");

        // The hang report's copy is asserted where its other lines are, in
        // `hang_watch`: building the facts it renders from takes a stack sample
        // fixture that belongs beside the rest of them.
    }
}
