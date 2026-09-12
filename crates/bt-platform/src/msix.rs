//! **Folio's package identity** — what `packaging/msix/AppxManifest.xml` says,
//! and the deployment calls that make a Windows believe it (`docs/DESIGN.md`
//! §7.4a).
//!
//! # What a package buys, and what it costs
//!
//! Windows 11 promotes exactly one kind of right-click verb to the first page of
//! the menu: an `IExplorerCommand` declared by a **package**. Everything else —
//! including the two `HKCU\Software\Classes` trees §7.4 writes, and every classic
//! verb every other program on the machine registers — lands under *Show more
//! options*. So the first page is not a registry key we have not found; it is an
//! identity we did not have.
//!
//! The identity here is a **sparse** package: it carries no program at all.
//! `folio.exe`, `conpty.dll` and `OpenConsole.exe` stay in whatever folder the
//! user extracted the archive into, and the package is registered with an
//! *external location* naming that folder. That is the only shape a product with
//! no installer can take — the alternative is a real MSIX, which would mean
//! Windows owns where the program lives, and Folio's whole distribution is a zip
//! somebody drops where they like.
//!
//! # The three strings, and the one that will be got wrong
//!
//! [`PACKAGE_NAME`] and [`EXPLORER_COMMAND_CLSID`] are arbitrary and only have to
//! agree with the manifest. [`PACKAGE_PUBLISHER`] is not arbitrary: Windows
//! refuses to register a package whose `Publisher` is not the subject of the
//! certificate that signed it, and the refusal names **neither** string. That is
//! the failure this module's one pure function exists for —
//! [`publisher_matches_subject`] is the comparison, written so it can be made
//! without a certificate, and `scripts/release/smoke.ps1` makes the same
//! comparison on the artefact.
//!
//! **It is a distinguished-name comparison and not a string one.** Windows prints
//! a certificate's subject with its own spacing, and `CN=A, O=B` and `CN=A,O=B`
//! are the same name written twice. A raw `==` here would fail releases for a
//! space, which is a check that costs more than it catches.
//!
//! # Why the classic verb stays
//!
//! Registering this package does not remove the two registry trees, and switching
//! this off does not either. They are what Windows 10 has, they are what a
//! machine with no package registered has, and Windows itself puts the package's
//! item on the first page and the classic one under *Show more options* without
//! either knowing about the other. See `bt_app::explorer_menu` for the one row
//! that spans both.

use std::fmt::Write as _;

// ── the identity, which is the manifest's and is spelled once ───────────────

/// `Identity/@Name` in `packaging/msix/AppxManifest.xml`.
///
/// A publisher-scoped name rather than a bare `Folio`: package names are global
/// per publisher, and the prefix is what keeps this one from colliding with
/// anybody else's idea of what a folio is.
pub const PACKAGE_NAME: &str = "WeiyiShi.Folio";

/// `Identity/@Publisher`, which **must** be the signing certificate's subject.
///
/// Character for character, including the lower-case `mi`: this string is
/// compared against the certificate at registration time, and what it is
/// compared against is what the certificate authority issued rather than what
/// looks tidy here. See [`publisher_matches_subject`].
pub const PACKAGE_PUBLISHER: &str = "CN=Weiyi Shi, O=Weiyi Shi, L=Ann Arbor, S=mi, C=US";

/// The COM class Explorer creates to draw and to run the menu item.
///
/// Spelled without braces, which is the form both `com:Class/@Id` and
/// `desktop5:Verb/@Clsid` take in the manifest. [`explorer_command_clsid`] is
/// the same value as a GUID for the code that has to register it.
pub const EXPLORER_COMMAND_CLSID: &str = "E129F337-6C7E-4AC7-9C02-58FCB8940AF1";

/// The package file, as it is named in the release archive and looked for beside
/// `folio.exe`.
pub const PACKAGE_FILE_NAME: &str = "folio.msix";

/// The program the manifest names — `Application/@Executable`, and the
/// `com:ExeServer` that answers Explorer's class.
///
/// **The one file an external location has to hold.** A sparse package carries
/// no program: the folder it is registered against is a folder Windows will look
/// in for exactly this name, so "is there still a Folio over there" is a
/// question with a spelling, and this is it. `bt_app::explorer_menu` asks it of
/// a registration that names another folder, which is the only way to tell an
/// install that moved from one that is still standing.
pub const PACKAGE_EXECUTABLE: &str = "folio.exe";

/// The first Windows build that has a first page to be promoted onto.
///
/// 22000 is Windows 11's first. Below it the whole of this module is a thing
/// that would succeed and change nothing anybody could see, which is why
/// [`supports_primary_context_menu`] is asked before the row is even drawn.
pub const PRIMARY_CONTEXT_MENU_BUILD: u32 = 22_000;

/// Whether this Windows has the menu page a package is registered to reach.
#[must_use]
pub fn supports_primary_context_menu(build: u32) -> bool {
    build >= PRIMARY_CONTEXT_MENU_BUILD
}

// ── the publisher, and the certificate it has to be ─────────────────────────

/// One relative distinguished name: the attribute type, upper-cased, and its
/// value exactly as written.
pub type Rdn = (String, String);

/// Split a distinguished name into its parts, normalised for comparison.
///
/// **Type upper-cased, value untouched but trimmed.** The types are a fixed set
/// of keywords and `cn` and `CN` are the same attribute; the values are not, and
/// a package whose publisher says `S=MI` where the certificate says `S=mi` is a
/// package Windows will refuse — so a comparison that folded the value's case
/// would pass the release that then fails on the user's machine, which is the
/// one outcome this function exists to prevent.
///
/// Commas inside `\,` and inside a quoted value do not separate, because they do
/// not in a distinguished name: `O=Acme\, Inc.` is one part and reading it as two
/// would make two names that are the same look different. The punctuation that
/// protected such a comma comes off with it — see the `match` below.
#[must_use]
pub fn distinguished_name(text: &str) -> Vec<Rdn> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    let mut quoted = false;
    for ch in text.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            // **The punctuation comes off and the character it protected
            // stays.** A backslash spells the next character literally and
            // quotation marks spell a whole value literally, so `O=Acme\, Inc.`
            // and `O="Acme, Inc."` reduce to one value — which is what
            // `scripts/release/smoke.ps1` does with the same two strings. Two
            // comparisons of one fact that disagreed about it would be worse
            // than one comparison.
            '\\' => escaped = true,
            '"' => quoted = !quoted,
            ',' if !quoted => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    parts.push(current);

    parts
        .into_iter()
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            // The **first** `=`: a value may hold one, an attribute type may not.
            let (kind, value) = part.split_once('=')?;
            Some((kind.trim().to_ascii_uppercase(), value.trim().to_owned()))
        })
        .collect()
}

/// Whether a manifest's `Publisher` names the same entity as a certificate's
/// subject.
///
/// Order matters and is not sorted away: a distinguished name is a path, `CN=A,
/// O=B` and `O=B, CN=A` are two different names, and a comparison that sorted
/// them would call a real mismatch a match.
///
/// An empty name on either side is never a match. That case is not decoration:
/// it is what a subject read off an unsigned file looks like, and it must not
/// come out equal to another unsigned file's.
#[must_use]
pub fn publisher_matches_subject(publisher: &str, subject: &str) -> bool {
    let publisher = distinguished_name(publisher);
    !publisher.is_empty() && publisher == distinguished_name(subject)
}

/// The publisher this build would register under, as one line for a diagnostic.
///
/// Used where a registration has just been refused and the two names are the
/// only thing worth printing.
#[must_use]
pub fn describe_name(name: &[Rdn]) -> String {
    let mut text = String::new();
    for (index, (kind, value)) in name.iter().enumerate() {
        if index > 0 {
            text.push_str(", ");
        }
        let _ = write!(text, "{kind}={value}");
    }
    text
}

// ── the deployment calls ────────────────────────────────────────────────────

#[cfg(windows)]
mod deployment {
    use std::path::{Path, PathBuf};

    use windows::{
        ApplicationModel::Package,
        Foundation::Uri,
        Management::Deployment::{AddPackageOptions, PackageManager},
        Wdk::System::SystemServices::RtlGetVersion,
        Win32::System::SystemInformation::OSVERSIONINFOW,
        core::HSTRING,
    };

    /// This Windows's build number, from the call that is never lied to.
    ///
    /// `GetVersionExW` answers `6.2` — Windows 8 — for any process without a
    /// compatibility manifest listing Windows 10's GUID, and `folio.exe` has no
    /// application manifest at all (`bt_winres` writes an icon and a version
    /// block, and nothing else). `RtlGetVersion` is the kernel's own answer and
    /// takes no notice of shims, which is exactly why the documentation points
    /// drivers at it.
    ///
    /// Zero when the call fails, which reads as "not Windows 11" everywhere it
    /// is asked — the safe direction, since what it gates is offering somebody a
    /// registration that would do nothing.
    #[must_use]
    pub fn windows_build() -> u32 {
        let mut info = OSVERSIONINFOW {
            dwOSVersionInfoSize: u32::try_from(size_of::<OSVERSIONINFOW>()).unwrap_or_default(),
            ..Default::default()
        };
        // SAFETY: `info` is a live, correctly sized `OSVERSIONINFOW` whose
        // `dwOSVersionInfoSize` names its own length, which is the whole of this
        // call's contract. It writes nothing else and keeps no pointer.
        let status = unsafe { RtlGetVersion(&raw mut info) };
        if status.is_ok() {
            info.dwBuildNumber
        } else {
            0
        }
    }

    /// A package of ours that this user currently has registered.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct PackageRegistration {
        /// What [`remove`] has to be given.
        pub full_name: String,
        /// The folder the registration points its content at, when it has one.
        ///
        /// `None` for a package with no external location — which ours always
        /// has, so in practice it means an operating system that would not say.
        pub external_path: Option<PathBuf>,
    }

    /// What this user has registered under [`super::PACKAGE_NAME`], if anything.
    ///
    /// **For this user and not for the machine.** The all-users query needs
    /// administrator, and everything this product writes belongs to the account
    /// that ran it (§7.4's own rule, one hive further out): an empty security id
    /// is how the deployment API spells "whoever is asking".
    pub fn registered() -> Result<Option<PackageRegistration>, String> {
        let manager = PackageManager::new().map_err(|error| error.message())?;
        let found = manager
            .FindPackagesByUserSecurityIdNamePublisher(
                &HSTRING::new(),
                &HSTRING::from(super::PACKAGE_NAME),
                &HSTRING::from(super::PACKAGE_PUBLISHER),
            )
            .map_err(|error| error.message())?;
        // **The first, and there is never a second.** A name and publisher
        // identify one package per user; a machine that somehow held two would
        // have two answers to a question with one, and taking the first is the
        // same thing the deployment stack itself would do.
        found
            .into_iter()
            .next()
            .map(|package| describe(&package))
            .transpose()
    }

    fn describe(package: &Package) -> Result<PackageRegistration, String> {
        let full_name = package
            .Id()
            .and_then(|id| id.FullName())
            .map_err(|error| error.message())?
            .to_string_lossy();
        // A package with no external location answers this with an error rather
        // than with an empty string, and that is not a failure of ours to report:
        // the caller's question is "where does this registration point", and "it
        // does not point anywhere" is an answer.
        let external_path = package
            .EffectiveExternalPath()
            .ok()
            .map(|path| PathBuf::from(path.to_string_lossy()))
            .filter(|path| !path.as_os_str().is_empty());
        Ok(PackageRegistration {
            full_name,
            external_path,
        })
    }

    /// Register `msix`, with `external` as the folder its content is at.
    ///
    /// Blocking, and therefore never on the window thread —
    /// `bt_app::explorer_menu` runs it on one of its own. A deployment is one to
    /// three seconds of somebody else's service on a good day.
    ///
    /// Registering over an existing registration is how a moved folder is
    /// repaired: the call replaces what is there rather than refusing, so there
    /// is no remove-then-add sequence that could leave a machine with neither.
    ///
    /// **It ends by telling the shell** — see
    /// [`crate::changing_explorer_menu`], which is the wrapper both directions
    /// of this deployment and both directions of the classic registration go
    /// through. A registration nobody announced is a menu item nobody sees.
    pub fn register(msix: &Path, external: &Path) -> Result<(), String> {
        crate::changing_explorer_menu(
            || register_package(msix, external),
            crate::announce_explorer_menu_change,
        )
    }

    fn register_package(msix: &Path, external: &Path) -> Result<(), String> {
        let manager = PackageManager::new().map_err(|error| error.message())?;
        let options = AddPackageOptions::new().map_err(|error| error.message())?;
        options
            .SetExternalLocationUri(&file_uri(external)?)
            .map_err(|error| error.message())?;
        let operation = manager
            .AddPackageByUriAsync(&file_uri(msix)?, &options)
            .map_err(|error| error.message())?;
        let result = operation.join().map_err(|error| error.message())?;
        // **The deployment result carries its own failure**, and it is not the
        // one the call returned: `join` answers the HRESULT of *running the
        // operation*, while a package that was refused comes back as a completed
        // operation holding an error. Reading only the first is how a refusal
        // becomes a switch that says it worked.
        let error = result
            .ExtendedErrorCode()
            .map_err(|error| error.message())?;
        if error.is_err() {
            let text = result
                .ErrorText()
                .map(|text| text.to_string_lossy())
                .unwrap_or_default();
            let text = text.trim();
            return Err(if text.is_empty() {
                format!("0x{:08X}", error.0)
            } else {
                text.to_owned()
            });
        }
        Ok(())
    }

    /// Take the registration back off this user's machine.
    ///
    /// [`register`]'s announcement, for the same reason read the other way: an
    /// item Explorer still draws after the package behind it is gone is an item
    /// whose click finds no class to create.
    pub fn remove(full_name: &str) -> Result<(), String> {
        crate::changing_explorer_menu(
            || remove_package(full_name),
            crate::announce_explorer_menu_change,
        )
    }

    fn remove_package(full_name: &str) -> Result<(), String> {
        let manager = PackageManager::new().map_err(|error| error.message())?;
        let operation = manager
            .RemovePackageAsync(&HSTRING::from(full_name))
            .map_err(|error| error.message())?;
        let result = operation.join().map_err(|error| error.message())?;
        let error = result
            .ExtendedErrorCode()
            .map_err(|error| error.message())?;
        if error.is_err() {
            let text = result
                .ErrorText()
                .map(|text| text.to_string_lossy())
                .unwrap_or_default();
            let text = text.trim();
            return Err(if text.is_empty() {
                format!("0x{:08X}", error.0)
            } else {
                text.to_owned()
            });
        }
        Ok(())
    }

    /// A path as the `file:` address the deployment API takes.
    ///
    /// **Percent-encoded before it is parsed, and by the one encoder this
    /// workspace has** (R2-11). The note that stood here said the address was
    /// "built by the operating system's own parser rather than by string
    /// arithmetic", and the line under it was string arithmetic: separators were
    /// turned round and every other character was handed to `Uri::CreateUri` as
    /// it stood. A parser is not an encoder, and the three characters it reads
    /// as syntax are three characters Windows allows in a folder name —
    /// `C:\Work#2\Folio` names `C:/Work` with a fragment on it, `C:\a?b` truncates
    /// at the `?`, and a `%` in front of two hexadecimal digits decodes to a
    /// different byte entirely. Each of those is a registration pointing at a
    /// folder the reader does not have.
    ///
    /// [`bt_transcript::paths::local_path_to_file_uri`] is the encoder, shared
    /// with the transcript's own links rather than written a second time here:
    /// two spellings of "a path as a URI" is two places for one of them to be
    /// wrong, and this is the one whose being wrong changes the machine.
    pub(super) fn file_uri(path: &Path) -> Result<Uri, String> {
        Uri::CreateUri(&HSTRING::from(
            bt_transcript::paths::local_path_to_file_uri(path),
        ))
        .map_err(|error| error.message())
    }
}

#[cfg(windows)]
pub use deployment::{PackageRegistration, register, registered, remove, windows_build};

/// **Sparse-package deployment, on a platform that has no packages** (M1-1, and
/// there is no later ticket: §1 of the macOS plan puts "any MSIX equivalent"
/// out of 0.4 by name, because macOS has no first-page context menu to register
/// into).
///
/// The pure half of this module — the six identity constants, the build test
/// and the whole certificate-subject parser — is **ungated and compiles here
/// today**, which is a correction the inventory makes to §4.4's closing
/// sentence: "`msix.rs` is absent" would send a ticket to delete something that
/// already builds. Only the four deployment entry points and the CLSID are
/// Windows, and these are the three of them `bt-app` names without a gate of
/// its own.
#[cfg(not(windows))]
mod no_deployment {
    use std::path::{Path, PathBuf};

    /// A package of ours that this user currently has registered. There is
    /// never one; the type exists so that `bt-app`'s settings row reads the
    /// same shape on both platforms.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct PackageRegistration {
        /// What [`remove`] has to be given.
        pub full_name: String,
        /// The folder the registration points its content at, when it has one.
        pub external_path: Option<PathBuf>,
    }

    /// What this user has registered. Nothing, and that is not a failure: the
    /// row above reads `Ok(None)` as *not installed*, which is true and final
    /// here rather than pending.
    pub fn registered() -> Result<Option<PackageRegistration>, String> {
        Ok(None)
    }

    /// Register the package. Refused, with the reason the row shows.
    pub fn register(msix: &Path, external: &Path) -> Result<(), String> {
        let _ = (msix, external);
        Err(NO_PACKAGES.to_owned())
    }

    /// Remove it. Refused, and nothing is there to remove.
    pub fn remove(full_name: &str) -> Result<(), String> {
        let _ = full_name;
        Err(NO_PACKAGES.to_owned())
    }

    const NO_PACKAGES: &str =
        "this platform has no package to register: the first-page context menu is Windows only";
}

#[cfg(not(windows))]
pub use no_deployment::{PackageRegistration, register, registered, remove};

/// The class this build registers, as a GUID.
#[cfg(windows)]
#[must_use]
pub fn explorer_command_clsid() -> windows::core::GUID {
    windows::core::GUID::from_u128(0xE129_F337_6C7E_4AC7_9C02_58FC_B894_0AF1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RED (R2-11) — **a package location with punctuation in it names the folder
    /// it is.**
    ///
    /// The location is a real folder on somebody's disk, and Windows lets a folder
    /// be called `Work #2` or `100% done`. `#`, `?` and `%` are the three
    /// characters a URI parser reads as syntax rather than as text, so a path
    /// handed to `Uri::CreateUri` unencoded does not fail — it succeeds, naming a
    /// **different location**, and the registration points there.
    ///
    /// MUTATION: put the old `format!("file:///{}", text.replace('\\', "/"))`
    /// back and the fragment is `2/100% done/folio?x`, the query is `x`, and the
    /// path stops at `Work `.
    #[cfg(windows)]
    #[test]
    fn a_package_location_with_punctuation_in_it_names_the_folder_it_is() {
        use std::path::Path;
        let folder = Path::new(r"C:\Work #2\100% done\folio?x");
        let uri = deployment::file_uri(folder).expect("a location is a URI");
        assert_eq!(
            uri.RawUri()
                .expect("the URI has a spelling")
                .to_string_lossy(),
            "file:///C:/Work%20%232/100%25%20done/folio%3Fx",
            "every character Windows allows in a folder name survives as itself"
        );
        assert!(
            uri.Fragment()
                .expect("a fragment")
                .to_string_lossy()
                .is_empty(),
            "a `#` in a folder name is not the start of a fragment"
        );
        assert!(
            uri.Query().expect("a query").to_string_lossy().is_empty(),
            "a `?` in a folder name is not the start of a query"
        );
        // And the ordinary case is left looking like itself.
        assert_eq!(
            deployment::file_uri(Path::new(r"C:\Users\me\Folio"))
                .expect("a location is a URI")
                .RawUri()
                .expect("the URI has a spelling")
                .to_string_lossy(),
            "file:///C:/Users/me/Folio"
        );
    }

    /// PIN — **a certificate subject and a manifest publisher differ by spacing
    /// and are the same name.**
    ///
    /// This is the comparison that decides whether a release can be registered at
    /// all, and it is made against a string Windows formats rather than one this
    /// project wrote. A `==` here fails a good release for a space after a comma.
    ///
    /// MUTATION: compare the two strings directly and the second assertion goes
    /// red, which is a release refused for punctuation.
    #[test]
    fn a_publisher_is_a_distinguished_name_and_not_a_string() {
        assert!(publisher_matches_subject(
            PACKAGE_PUBLISHER,
            PACKAGE_PUBLISHER
        ));
        assert!(publisher_matches_subject(
            PACKAGE_PUBLISHER,
            "CN=Weiyi Shi,O=Weiyi Shi,L=Ann Arbor,S=mi,C=US"
        ));
        assert!(publisher_matches_subject(
            PACKAGE_PUBLISHER,
            "cn=Weiyi Shi, o=Weiyi Shi, l=Ann Arbor, s=mi, c=US"
        ));
    }

    /// PIN — **the ways two names are genuinely different, and each is caught.**
    ///
    /// A different holder, a different value's case (which Windows treats as a
    /// different name at registration), a missing part, a reordering, and an
    /// empty subject — the last being what an unsigned file answers, and the one
    /// that must never come out equal to anything.
    ///
    /// MUTATION: fold the value's case and the third assertion goes red, which is
    /// a release that passes here and is refused on the user's machine.
    #[test]
    fn a_publisher_that_is_not_the_certificate_is_refused() {
        assert!(!publisher_matches_subject(
            PACKAGE_PUBLISHER,
            "CN=Someone Else, O=Weiyi Shi, L=Ann Arbor, S=mi, C=US"
        ));
        assert!(!publisher_matches_subject(
            PACKAGE_PUBLISHER,
            "CN=Weiyi Shi, O=Weiyi Shi, L=Ann Arbor, C=US"
        ));
        assert!(!publisher_matches_subject(
            PACKAGE_PUBLISHER,
            "CN=Weiyi Shi, O=Weiyi Shi, L=Ann Arbor, S=MI, C=US"
        ));
        assert!(!publisher_matches_subject(
            PACKAGE_PUBLISHER,
            "O=Weiyi Shi, CN=Weiyi Shi, L=Ann Arbor, S=mi, C=US"
        ));
        assert!(!publisher_matches_subject(PACKAGE_PUBLISHER, ""));
        assert!(!publisher_matches_subject("", ""));
    }

    /// PIN — **a comma inside a value does not divide the name, and its two
    /// legal spellings are one value.**
    ///
    /// `O=Acme\, Inc.` is one part. Splitting it into two makes a name that
    /// compares unequal to itself written the other legal way, and the failure
    /// arrives as a release nobody can register for a reason nobody can see.
    ///
    /// The backslash and the quotation marks are punctuation rather than
    /// content, so both come off — which is also what `smoke.ps1` does to the
    /// same two strings, and the two must agree or one of them passes a release
    /// the other would have stopped.
    #[test]
    fn an_escaped_or_quoted_comma_is_part_of_the_value() {
        assert_eq!(
            distinguished_name(r"CN=A, O=Acme\, Inc., C=US"),
            vec![
                ("CN".to_owned(), "A".to_owned()),
                ("O".to_owned(), "Acme, Inc.".to_owned()),
                ("C".to_owned(), "US".to_owned()),
            ]
        );
        assert!(publisher_matches_subject(
            r"CN=A, O=Acme\, Inc., C=US",
            r#"CN=A,O="Acme, Inc.",C=US"#
        ));
    }

    /// PIN — **the manifest on disk is the manifest this code believes in.**
    ///
    /// Five strings live in two places — the package's name, its publisher, the
    /// CLSID Explorer creates, the CLSID the two item types point at, and the
    /// program an external location has to hold — and nothing at build time would
    /// notice them drifting apart. What a drift costs is a menu item that is
    /// registered and never appears, a class Explorer asks for that this binary
    /// does not answer to, or a launch looking for the wrong file over there and
    /// so re-registering the package on top of an install that is still standing.
    ///
    /// MUTATION: change any one of the five in either file and this goes red.
    #[test]
    fn the_manifest_and_this_module_say_the_same_five_things() {
        let manifest = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("packaging")
                .join("msix")
                .join("AppxManifest.xml"),
        )
        .expect("the manifest is in the repository beside this crate");
        assert!(
            manifest.contains(&format!("Name=\"{PACKAGE_NAME}\"")),
            "the manifest names a different package"
        );
        assert!(
            manifest.contains(&format!("Publisher=\"{PACKAGE_PUBLISHER}\"")),
            "the manifest names a different publisher"
        );
        assert!(
            manifest.contains(&format!("Id=\"{EXPLORER_COMMAND_CLSID}\"")),
            "the manifest's COM class is not the one this build registers"
        );
        assert_eq!(
            manifest.matches(EXPLORER_COMMAND_CLSID).count(),
            3,
            "the class is declared once and pointed at by both item types"
        );
        assert_eq!(
            manifest
                .matches(&format!("Executable=\"{PACKAGE_EXECUTABLE}\""))
                .count(),
            2,
            "the application and its class server name the program this module \
             looks for at an external location"
        );
        // The version is a placeholder on purpose; `package.ps1` injects the
        // workspace's. A real one here would be a second version line.
        assert!(
            manifest.contains("Version=\"0.0.0.0\""),
            "the manifest carries a version of its own, which is a second place \
             this product's version is written"
        );
    }

    /// RED (2026-09-07) — **both directions of the deployment go through the
    /// wrapper that tells the shell.**
    ///
    /// A deployment cannot be made in a test — it is a service call that would
    /// change the right-click menu of whoever is running the suite — so what can
    /// be held is the shape: [`register`] and [`remove`] are two lines each, and
    /// the line that matters is
    /// [`crate::changing_explorer_menu`]. The wrapper itself is tested where it
    /// is defined; this is the pin that says the two calls that need it use it.
    ///
    /// It is the same kind of pin as
    /// [`the_manifest_and_this_module_say_the_same_five_things`] below and for
    /// the same reason: two things have to agree, nothing at build time would
    /// notice them coming apart, and what it would cost is a package registered
    /// on a machine that goes on drawing the menu it read an hour ago.
    ///
    /// **The suite's own half of the file is cut off before the search**, or
    /// every needle below would find itself: the words this test is looking for
    /// are words it is written in.
    ///
    /// MUTATION: call `register_package` or `remove_package` directly from the
    /// public function and this goes red.
    #[test]
    fn every_deployment_this_module_makes_announces_itself_to_the_shell() {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("msix.rs"),
        )
        .expect("this module is a file in this crate");
        let code = source
            .split_once("#[cfg(test)]")
            .expect("this module carries the suite this test is in")
            .0;
        assert_eq!(
            code.matches("crate::changing_explorer_menu(").count(),
            2,
            "the two deployment calls are the two that go through the wrapper"
        );
        for call in [
            "register_package(msix, external)",
            "remove_package(full_name)",
        ] {
            assert_eq!(
                code.matches(call).count(),
                1,
                "{call} is reached from the wrapper and from nowhere else"
            );
        }
    }

    /// PIN — **Windows 11's first build is the edge, and 10 is below it.**
    #[test]
    fn only_windows_eleven_has_a_first_page() {
        assert!(!supports_primary_context_menu(0));
        assert!(!supports_primary_context_menu(19_045));
        assert!(!supports_primary_context_menu(21_999));
        assert!(supports_primary_context_menu(22_000));
        assert!(supports_primary_context_menu(26_100));
    }
}
