//! The Windows halves of [`crate::trust`]'s tests (E-13): files signed in the
//! test's folder by the harness of `crate::trust_harness::world` (a root,
//! leaves and a time-stamp authority made with ephemeral keys; small PE files
//! signed by `SignerSignEx3`, time-stamped over HTTP on the loopback interface)
//! and verified under an exclusive-root engine. Every key is ephemeral, every
//! store is a memory store, and nothing is installed.

use std::path::{Path, PathBuf};

use windows::Win32::Security::Cryptography::{
    CERT_CONTEXT, CertCreateCertificateContext, CertFreeCertificateContext, X509_ASN_ENCODING,
};

use crate::trust::{
    CODE_SIGNING, Capability, Expectation, FileVersion, Machine, PUBLIC_TRUST_MARKER, Policy,
    Refusal, capability_of, identity_of, signature, verify_release_file_under,
};
use crate::trust_harness::world::{
    DAY, Folder, IDENTITY, OTHER_IDENTITY, SUBJECT, VERSION, World, executable, now, sign, wide,
};

// ── the tests' Windows halves ───────────────────────────────────────────────

/// The version an offer names in these tests.
pub const NEXT: FileVersion = FileVersion([0, 4, 7, 0]);

/// Another publisher's subject.
pub const OTHER_SUBJECT: &str =
    "CN=Another Publisher, O=Another Publisher, L=Example City, S=mi, C=US";

pub fn expectation_of(path: &Path, policy: &Policy) -> Expectation {
    identity_of(path, policy).unwrap_or_else(|refusal| panic!("{refusal}"))
}

fn verify(path: &Path, expected: &Expectation, policy: &Policy) -> Result<(), Refusal> {
    verify_release_file_under(path, expected, policy).map(|_| ())
}

/// `CERT_E_UNTRUSTEDROOT`.
const CERT_E_UNTRUSTEDROOT: i32 = 0x800B_0109_u32.cast_signed();
/// `TRUST_E_BAD_DIGEST`.
const TRUST_E_BAD_DIGEST: i32 = 0x8009_6010_u32.cast_signed();

pub fn a_test_signed_file_without_a_revocation_list_is_revocation_unknown() {
    let world = World::new();
    let folder = Folder::new("revocation");
    let leaf = world.leaf(SUBJECT, IDENTITY);
    let path = world.signed(&folder, "folio.exe", &leaf, VERSION);
    assert!(signature(&path, &world.policy()).is_ok(), "known good");
    let silent = Policy::ExclusiveRoot {
        root: world.root.der.clone(),
        revocation_lists: Vec::new(),
    };
    assert_eq!(signature(&path, &silent), Err(Refusal::RevocationUnknown));
    assert_eq!(
        signature(&path, &world.policy_revoking(&[world.last_serial()])),
        Err(Refusal::Revoked)
    );
}

pub fn validly_signed_wrong_product_or_version_is_refused() {
    let world = World::new();
    let folder = Folder::new("wrong");
    let policy = world.policy();
    let running = world.signed(
        &folder,
        "running.exe",
        &world.leaf(SUBJECT, IDENTITY),
        VERSION,
    );
    let expected = expectation_of(&running, &policy).for_offer(NEXT);

    let good = world.signed(&folder, "good.exe", &world.leaf(SUBJECT, IDENTITY), NEXT);
    assert_eq!(verify(&good, &expected, &policy), Ok(()));

    let other = world.signed(
        &folder,
        "other.exe",
        &world.leaf(OTHER_SUBJECT, OTHER_IDENTITY),
        NEXT,
    );
    assert!(matches!(
        verify(&other, &expected, &policy),
        Err(Refusal::IdentityDiffers { .. })
    ));
    let borrowed = world.signed(
        &folder,
        "borrowed.exe",
        &world.leaf(OTHER_SUBJECT, IDENTITY),
        NEXT,
    );
    assert!(matches!(
        verify(&borrowed, &expected, &policy),
        Err(Refusal::SubjectDiffers { .. })
    ));
    let old = world.signed(&folder, "old.exe", &world.leaf(SUBJECT, IDENTITY), VERSION);
    assert_eq!(
        verify(&old, &expected, &policy),
        Err(Refusal::VersionDiffers {
            expected: NEXT,
            found: VERSION
        })
    );
    let mut elsewhere = expected.clone();
    elsewhere.machine = if expected.machine == Machine::ARM64 {
        Machine::X64
    } else {
        Machine::ARM64
    };
    assert!(matches!(
        verify(&good, &elsewhere, &policy),
        Err(Refusal::MachineDiffers { .. })
    ));

    let bare = executable(&folder, "bare.exe", NEXT);
    assert_eq!(verify(&bare, &expected, &policy), Err(Refusal::Unsigned));
    let unstamped = executable(&folder, "unstamped.exe", NEXT);
    sign(
        &unstamped,
        &world.leaf(SUBJECT, IDENTITY),
        &[&world.root],
        None,
    );
    assert_eq!(
        verify(&unstamped, &expected, &policy),
        Err(Refusal::NoTimestamp)
    );

    // A changed byte after signing: `WinVerifyTrust`'s own digest check,
    // which an exclusive root does not relax.
    let mut bytes = std::fs::read(&good).unwrap();
    bytes[0x400] ^= 0xFF;
    std::fs::write(&good, bytes).unwrap();
    assert_eq!(
        verify(&good, &expected, &policy),
        Err(Refusal::Untrusted {
            code: TRUST_E_BAD_DIGEST
        })
    );
}

pub fn dn_values_preserve_case_and_order() {
    let world = World::new();
    let folder = Folder::new("names");
    let policy = world.policy();
    let running = world.signed(
        &folder,
        "running.exe",
        &world.leaf(SUBJECT, IDENTITY),
        VERSION,
    );
    let expected = expectation_of(&running, &policy);
    let pairs = |list: &[(&str, &str)]| {
        list.iter()
            .map(|&(kind, value)| (kind.to_owned(), value.to_owned()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        expected.subject_dn,
        pairs(&[
            ("CN", "Folio Test Publisher"),
            ("O", "Folio Test Publisher"),
            ("L", "Example City"),
            ("S", "mi"),
            ("C", "US"),
        ]),
        "read from the signed file, as issued"
    );
    let shouted = world.signed(
        &folder,
        "shouted.exe",
        &world.leaf(
            "CN=Folio Test Publisher, O=Folio Test Publisher, L=Example City, S=MI, C=US",
            IDENTITY,
        ),
        VERSION,
    );
    assert!(matches!(
        verify(&shouted, &expected, &policy),
        Err(Refusal::SubjectDiffers { .. })
    ));
    let reordered = world.signed(
        &folder,
        "reordered.exe",
        &world.leaf(
            "O=Folio Test Publisher, CN=Folio Test Publisher, L=Example City, S=mi, C=US",
            IDENTITY,
        ),
        VERSION,
    );
    assert!(matches!(
        verify(&reordered, &expected, &policy),
        Err(Refusal::SubjectDiffers { .. })
    ));
}

pub fn timestamp_policy_accepts_the_packagers_format() {
    let world = World::new();
    let folder = Folder::new("stamp");
    let policy = world.policy();
    let leaf = world.leaf(SUBJECT, IDENTITY);
    let path = executable(&folder, "folio.exe", VERSION);
    // An hour ago, to the second: `GeneralizedTime` without fractions.
    let at = (now() - 3600 * 10_000_000) / 10_000_000 * 10_000_000;
    sign(&path, &leaf, &[&world.root], Some(&world.stamping(at)));
    let signed = signature(&path, &policy).unwrap_or_else(|refusal| panic!("{refusal}"));
    assert_eq!(
        signed.stamped_at, at,
        "the authority's time, read from the token"
    );

    let unstamped = executable(&folder, "unstamped.exe", VERSION);
    sign(&unstamped, &leaf, &[&world.root], None);
    assert_eq!(signature(&unstamped, &policy), Err(Refusal::NoTimestamp));
}

pub fn an_unsigned_running_build_is_not_capable() {
    // This test's own executable is the running build, and cargo signs nothing.
    assert_eq!(crate::trust::running_capability(true), Capability::Unsigned);
    assert_eq!(
        crate::trust::running_capability(false),
        Capability::NotFlagged
    );

    let world = World::new();
    let folder = Folder::new("capable");
    let policy = world.policy();
    let signed = world.signed(
        &folder,
        "folio.exe",
        &world.leaf(SUBJECT, IDENTITY),
        VERSION,
    );
    assert_eq!(
        capability_of(&signed, true, &policy),
        Capability::Capable(Box::new(expectation_of(&signed, &policy)))
    );
    assert_eq!(
        capability_of(&signed, false, &policy),
        Capability::NotFlagged
    );
    let unidentified = world.signed(
        &folder,
        "unidentified.exe",
        &world.leaf(SUBJECT, PUBLIC_TRUST_MARKER),
        VERSION,
    );
    assert!(matches!(
        capability_of(&unidentified, true, &policy),
        Capability::Unidentified(Refusal::NoIdentity { .. })
    ));
}

pub fn a_new_certificate_with_the_same_subject_is_accepted() {
    let world = World::new();
    let folder = Folder::new("renewal");
    let policy = world.policy();
    let today = world.leaf(SUBJECT, IDENTITY);
    let tomorrow = world.leaf(SUBJECT, IDENTITY);
    assert_ne!(today.der, tomorrow.der, "two certificates, two keys");
    let running = world.signed(&folder, "running.exe", &today, VERSION);
    let expected = expectation_of(&running, &policy).for_offer(NEXT);
    let update = world.signed(&folder, "update.exe", &tomorrow, NEXT);
    assert_eq!(verify(&update, &expected, &policy), Ok(()));
}

pub fn same_dn_with_a_different_identity_is_refused() {
    let world = World::new();
    let folder = Folder::new("identity");
    let policy = world.policy();
    let running = world.signed(
        &folder,
        "running.exe",
        &world.leaf(SUBJECT, IDENTITY),
        VERSION,
    );
    let expected = expectation_of(&running, &policy).for_offer(NEXT);
    let impostor = world.signed(
        &folder,
        "impostor.exe",
        &world.leaf(SUBJECT, OTHER_IDENTITY),
        NEXT,
    );
    assert_eq!(
        verify(&impostor, &expected, &policy),
        Err(Refusal::IdentityDiffers {
            expected: IDENTITY.to_owned(),
            found: OTHER_IDENTITY.to_owned(),
        })
    );
    let anonymous = world.signed(
        &folder,
        "anonymous.exe",
        &world.leaf(SUBJECT, PUBLIC_TRUST_MARKER),
        NEXT,
    );
    assert!(matches!(
        verify(&anonymous, &expected, &policy),
        Err(Refusal::NoIdentity { .. })
    ));
}

pub fn an_expired_leaf_with_a_valid_timestamp_is_accepted() {
    let world = World::new();
    let folder = Folder::new("expired");
    let policy = world.policy();
    let running = world.signed(
        &folder,
        "running.exe",
        &world.leaf(SUBJECT, IDENTITY),
        VERSION,
    );
    let expected = expectation_of(&running, &policy).for_offer(NEXT);

    // `SignerSignEx3` refuses to sign with a certificate that has already
    // expired (`CERT_E_EXPIRED`), as `signtool` does; so the leaf here lives
    // a few seconds, signs and is stamped while it is valid, and is verified
    // after it has expired — the everyday case of a release older than its
    // three-day certificate, in seconds.
    let inside = executable(&folder, "inside.exe", NEXT);
    let expires = (now() + EXPIRY_TICKS) / 10_000_000 * 10_000_000;
    let short = world.leaf_between(SUBJECT, IDENTITY, now() - DAY, expires);
    sign(
        &inside,
        &short,
        &[&world.root],
        Some(&world.stamping(now())),
    );
    while now() <= expires + 10_000_000 {
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    assert_eq!(
        verify(&inside, &expected, &policy),
        Ok(()),
        "expired, stamped inside"
    );

    // A stamp before the leaf's validity began is outside it.
    let before = executable(&folder, "before.exe", NEXT);
    let late = world.leaf_between(
        SUBJECT,
        IDENTITY,
        now() - 3600 * 10_000_000,
        now() + 2 * DAY,
    );
    sign(
        &before,
        &late,
        &[&world.root],
        Some(&world.stamping(now() - DAY)),
    );
    assert_eq!(
        verify(&before, &expected, &policy),
        Err(Refusal::TimestampOutsideValidity)
    );
}

/// How long the expiring leaf lives: long enough to be signed and stamped
/// under a loaded test run, short enough to wait out.
const EXPIRY_TICKS: u64 = 15 * 10_000_000;

pub fn the_machine_store_is_never_written() {
    use windows::Win32::Security::Cryptography::{
        CERT_FIND_EXISTING, CERT_OPEN_STORE_FLAGS, CERT_STORE_OPEN_EXISTING_FLAG,
        CERT_STORE_PROV_SYSTEM_W, CERT_STORE_READONLY_FLAG, CERT_SYSTEM_STORE_CURRENT_USER,
        CERT_SYSTEM_STORE_LOCAL_MACHINE, CertCloseStore, CertFindCertificateInStore, CertOpenStore,
    };

    let world = World::new();
    let engine = crate::trust::arm::engine_of(&world.policy()).unwrap();
    assert!(
        engine.is_exclusive(),
        "the test policy builds its own engine"
    );
    assert_eq!(
        engine.exclusive_roots(),
        1,
        "whose exclusive root store holds the one root"
    );
    let system = crate::trust::arm::engine_of(&Policy::System).unwrap();
    assert!(!system.is_exclusive());
    assert_eq!(system.exclusive_roots(), 0);

    // The test root is trusted by the exclusive engine and by nothing on this
    // machine: under the system policy the same file does not chain.
    let folder = Folder::new("store");
    let path = world.signed(
        &folder,
        "folio.exe",
        &world.leaf(SUBJECT, IDENTITY),
        VERSION,
    );
    assert!(signature(&path, &world.policy()).is_ok());
    assert_eq!(
        signature(&path, &Policy::System),
        Err(Refusal::Untrusted {
            code: CERT_E_UNTRUSTEDROOT
        })
    );

    // And no system store holds a certificate this world made: each store is
    // opened read-only, existing only, and searched by content.
    for made in [&world.root, &*world.authority] {
        let context = Cert::of(&made.der);
        for location in [
            CERT_SYSTEM_STORE_CURRENT_USER,
            CERT_SYSTEM_STORE_LOCAL_MACHINE,
        ] {
            for name in ["Root", "CA", "My", "TrustedPublisher", "Trust"] {
                let name = wide(std::ffi::OsStr::new(name));
                let flags = CERT_OPEN_STORE_FLAGS(
                    location | CERT_STORE_READONLY_FLAG.0 | CERT_STORE_OPEN_EXISTING_FLAG.0,
                );
                // SAFETY: a read-only open of an existing store by name.
                let Ok(store) = (unsafe {
                    CertOpenStore(
                        CERT_STORE_PROV_SYSTEM_W,
                        windows::Win32::Security::Cryptography::CERT_QUERY_ENCODING_TYPE(0),
                        None,
                        flags,
                        Some(name.as_ptr().cast()),
                    )
                }) else {
                    continue;
                };
                // SAFETY: a live store and a live context to compare with.
                let found = unsafe {
                    CertFindCertificateInStore(
                        store,
                        X509_ASN_ENCODING,
                        0,
                        CERT_FIND_EXISTING,
                        Some(context.0.cast()),
                        None,
                    )
                };
                let present = !found.is_null();
                // SAFETY: frees what the search returned, and closes the store.
                unsafe {
                    if present {
                        let _ = CertFreeCertificateContext(Some(found));
                    }
                    let _ = CertCloseStore(Some(store), 0);
                }
                assert!(!present, "a test certificate is in a system store");
            }
        }
    }
}

/// A certificate context over DER, freed on drop.
struct Cert(*const CERT_CONTEXT);

impl Cert {
    fn of(der: &[u8]) -> Self {
        // SAFETY: the bytes are copied into the context.
        Self(unsafe { CertCreateCertificateContext(X509_ASN_ENCODING, der) })
    }
}

impl Drop for Cert {
    fn drop(&mut self) {
        // SAFETY: made above, freed once.
        unsafe {
            let _ = CertFreeCertificateContext(Some(self.0));
        }
    }
}

// ── E-6: a real release ─────────────────────────────────────────────────────

const REPOSITORY: &str = "lulu-loopp/folio-terminal";
const AGENT: &str = "folio-trust-test";

fn sha256_hex(bytes: &[u8]) -> String {
    use windows::Win32::Security::Cryptography::{BCRYPT_SHA256_ALG_HANDLE, BCryptHash};
    let mut digest = [0u8; 32];
    // SAFETY: a one-shot hash into a 32-byte buffer.
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, bytes, &mut digest) };
    assert!(status.is_ok(), "{status:?}");
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn download(folder: &Folder, path: &str, name: &str) -> PathBuf {
    use crate::https_download::{
        DOWNLOAD_FLOOR_BYTES, DOWNLOAD_IDLE_TIMEOUT, DownloadMonitor, HttpsDownload,
    };
    let monitor = std::sync::Arc::new(DownloadMonitor::new(|| {}));
    crate::http::https_download(&HttpsDownload {
        host: "github.com",
        path,
        user_agent: AGENT,
        directory: &folder.0,
        file_name: name,
        ceiling: 200 * 1024 * 1024,
        idle_timeout: DOWNLOAD_IDLE_TIMEOUT,
        floor: DOWNLOAD_FLOOR_BYTES,
        monitor: &monitor,
    })
    .unwrap_or_else(|error| panic!("{path}: {error:?}"))
    .path
}

fn latest_tag() -> String {
    let body = crate::http::https_get(&crate::http::HttpsGet {
        host: "api.github.com",
        path: &format!("/repos/{REPOSITORY}/releases/latest"),
        user_agent: AGENT,
        phase_timeout: std::time::Duration::from_secs(30),
        budget: std::time::Duration::from_secs(60),
        cap: 1024 * 1024,
    })
    .unwrap();
    let at = body.find("\"tag_name\"").expect("a tag_name");
    body[at..].split('"').nth(3).expect("its value").to_owned()
}

pub fn the_released_windows_assets_carry_an_identity_oid() {
    let tag = std::env::var("BT_TRUST_RELEASE_TAG")
        .ok()
        .filter(|tag| !tag.is_empty())
        .unwrap_or_else(latest_tag);
    let version = tag
        .trim_start_matches('v')
        .split('-')
        .next()
        .unwrap()
        .to_owned();
    let archive_name = format!("folio-{version}-windows-x64.zip");
    let folder = Folder::new("release");
    let sums = download(
        &folder,
        &format!("/{REPOSITORY}/releases/download/{tag}/SHA256SUMS.txt"),
        "SHA256SUMS.txt",
    );
    let archive = download(
        &folder,
        &format!("/{REPOSITORY}/releases/download/{tag}/{archive_name}"),
        &archive_name,
    );
    let sums = std::fs::read_to_string(sums).unwrap();
    let digest = sha256_hex(&std::fs::read(&archive).unwrap());
    assert!(
        sums.lines().any(|line| {
            let mut fields = line.split_whitespace();
            fields.next() == Some(digest.as_str())
                && fields.next().map(|name| name.trim_start_matches('*'))
                    == Some(archive_name.as_str())
        }),
        "{archive_name} does not match SHA256SUMS.txt"
    );
    let tar = std::path::Path::new(&std::env::var("SystemRoot").unwrap())
        .join("System32")
        .join("tar.exe");
    let status = crate::quiet_command(&tar)
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(&folder.0)
        .status()
        .unwrap();
    assert!(status.success(), "tar -xf {status}");
    let root = folder.0.join(format!("folio-{version}"));

    let exe = root.join("folio.exe");
    let identity = expectation_of(&exe, &Policy::System);
    let arcs = identity.identity_oid[crate::trust::IDENTITY_PREFIX.len()..]
        .split('.')
        .count();
    eprintln!(
        "E-6 {tag}: folio.exe identity OID {}<{arcs} arcs>; subject {} RDNs; version {}; machine {}",
        crate::trust::IDENTITY_PREFIX,
        identity.subject_dn.len(),
        identity.version,
        identity.machine
    );
    let signed = signature(&exe, &Policy::System).unwrap();
    assert!(
        signed
            .key_usages
            .iter()
            .any(|oid| oid == PUBLIC_TRUST_MARKER)
    );
    assert!(signed.key_usages.iter().any(|oid| oid == CODE_SIGNING));
    eprintln!(
        "E-6 {tag}: folio.exe key usages: {} ({} under the prefix)",
        signed.key_usages.len(),
        signed
            .key_usages
            .iter()
            .filter(|oid| oid.starts_with(crate::trust::IDENTITY_PREFIX))
            .count()
    );
    assert!(
        crate::msix::publisher_matches_subject(crate::msix::PACKAGE_PUBLISHER, &signed.subject),
        "the rendered subject is the package's publisher"
    );
    verify_release_file_under(&exe, &identity, &Policy::System).unwrap();

    let msix = root.join("folio.msix");
    let package =
        crate::trust::verify_release_package(&msix, &identity.clone().for_offer(identity.version))
            .unwrap_or_else(|refusal| panic!("folio.msix: {refusal}"));
    eprintln!(
        "E-6 {tag}: folio.msix passes: version {}",
        package.identity.version
    );

    for sidecar in ["conpty.dll", "OpenConsole.exe"] {
        let answer = crate::trust::verify_sidecar(&root.join(sidecar));
        eprintln!(
            "E-6 {tag}: {sidecar}: {:?}",
            answer.as_ref().map(|signed| signed.stamped_at)
        );
        answer.unwrap_or_else(|refusal| panic!("{sidecar}: {refusal}"));
    }
}
