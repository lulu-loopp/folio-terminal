//! Tests of [`super`]: the decisions everywhere, and on Windows the real
//! `WinVerifyTrust`, time-stamp and chain calls against files signed inside
//! the test with certificates made inside the test, under an exclusive-root
//! engine (E-13). Nothing here opens a system certificate store for writing,
//! installs a certificate, or signs anything outside the test's own folder.

use super::{
    CHAIN_IS_REVOKED, CHAIN_OFFLINE_REVOCATION, CHAIN_REVOCATION_UNKNOWN, Capability, Expectation,
    FileVersion, Machine, PUBLIC_TRUST_MARKER, Refusal, capability, compare, identity_oid,
    publisher_verdict, revocation_verdict, timestamp_within, trust_verdict,
};

fn expectation(subject: &str, oid: &str) -> Expectation {
    Expectation {
        subject_dn: crate::msix::distinguished_name(subject),
        identity_oid: oid.to_owned(),
        version: FileVersion([0, 4, 6, 0]),
        machine: Machine::X64,
    }
}

use crate::trust_harness::{IDENTITY, OTHER_IDENTITY, SUBJECT};

/// PIN (U-15) — **the identity OID is the one Artifact Signing EKU that is not
/// the Public Trust marker**; none, or two, is no identity.
///
/// E-6 measured 0.4.5's leaf: code signing, `1.3.6.1.4.1.311.97.1.0` (every
/// Public Trust certificate carries it, whoever it names) and one
/// four-arc identity OID. Comparing the marker would compare nothing.
///
/// MUTATION: drop the `!= PUBLIC_TRUST_MARKER` filter in `identity_oid` and the
/// real leaf's shape becomes "two identities" — refused.
#[test]
fn the_identity_oid_is_the_one_that_is_not_the_public_trust_marker() {
    let usages = |list: &[&str]| list.iter().map(|&s| s.to_owned()).collect::<Vec<_>>();
    assert_eq!(
        identity_oid(&usages(&[
            PUBLIC_TRUST_MARKER,
            super::CODE_SIGNING,
            IDENTITY
        ])),
        Ok(IDENTITY.to_owned())
    );
    assert!(matches!(
        identity_oid(&usages(&[PUBLIC_TRUST_MARKER, super::CODE_SIGNING])),
        Err(Refusal::NoIdentity { .. })
    ));
    assert!(matches!(
        identity_oid(&usages(&[IDENTITY, OTHER_IDENTITY])),
        Err(Refusal::NoIdentity { .. })
    ));
}

/// RED (U-15) — **revocation "unknown" is a refusal, and revoked is another**
/// — the policy seam, over a chain's error status.
///
/// Windows' default Authenticode policy passes a signature whose revocation
/// could not be checked (offline is accepted), so the updater reads the
/// chain's own flags: F-5 makes "unknown" a refusal, retried with the next
/// press. On Windows the same refusal is produced by the real chain engine in
/// `a_test_signed_file_without_a_revocation_list_is_revocation_unknown`.
///
/// MUTATION: drop `CHAIN_REVOCATION_UNKNOWN | CHAIN_OFFLINE_REVOCATION` from
/// `revocation_verdict` and the unknown cases pass.
#[test]
fn revocation_unknown_is_a_refusal() {
    assert_eq!(revocation_verdict(0), Ok(()));
    assert_eq!(
        revocation_verdict(CHAIN_REVOCATION_UNKNOWN),
        Err(Refusal::RevocationUnknown)
    );
    assert_eq!(
        revocation_verdict(CHAIN_REVOCATION_UNKNOWN | CHAIN_OFFLINE_REVOCATION),
        Err(Refusal::RevocationUnknown)
    );
    assert_eq!(
        revocation_verdict(CHAIN_IS_REVOKED | CHAIN_REVOCATION_UNKNOWN),
        Err(Refusal::Revoked)
    );
    assert_eq!(
        trust_verdict(super::CRYPT_E_REVOCATION_OFFLINE),
        Err(Refusal::RevocationUnknown)
    );
    assert_eq!(trust_verdict(super::CERT_E_REVOKED), Err(Refusal::Revoked));
    assert_eq!(
        trust_verdict(super::TRUST_E_NOSIGNATURE),
        Err(Refusal::Unsigned)
    );
    #[cfg(windows)]
    windows_world::a_test_signed_file_without_a_revocation_list_is_revocation_unknown();
}

/// PIN (U-15) — **the stamped time must fall inside the signer's validity,
/// both ends included.**
#[test]
fn the_stamped_time_must_fall_inside_the_leaf() {
    assert_eq!(timestamp_within(10, 10, 20), Ok(()));
    assert_eq!(timestamp_within(20, 10, 20), Ok(()));
    assert_eq!(
        timestamp_within(21, 10, 20),
        Err(Refusal::TimestampOutsideValidity)
    );
    assert_eq!(
        timestamp_within(9, 10, 20),
        Err(Refusal::TimestampOutsideValidity)
    );
}

/// RED (U-15) — **the msix manifest's `Publisher` must equal its own signer's
/// subject** — as parsed names, case kept, order kept.
///
/// Windows' own signer refuses to sign a package whose `Publisher` is not the
/// certificate's subject, so a mismatched signed package is not something the
/// test can make with the real signer; the decision is held here on the
/// strings, and the real read runs in
/// `the_released_windows_assets_carry_an_identity_oid` on a downloaded
/// `folio.msix`.
///
/// MUTATION: make `publisher_verdict` answer `Ok(())` unconditionally.
#[test]
fn the_msix_publisher_must_equal_its_signer() {
    assert_eq!(publisher_verdict(SUBJECT, SUBJECT), Ok(()));
    assert_eq!(
        publisher_verdict(
            "CN=Folio Test Publisher,O=Folio Test Publisher,L=Example City,S=mi,C=US",
            SUBJECT
        ),
        Ok(()),
        "spacing is not identity"
    );
    for other in [
        "CN=Folio Test Publisher, O=Folio Test Publisher, L=Example City, S=MI, C=US",
        "CN=Another Publisher, O=Folio Test Publisher, L=Example City, S=mi, C=US",
        "",
    ] {
        assert!(
            matches!(
                publisher_verdict(other, SUBJECT),
                Err(Refusal::PublisherIsNotSigner { .. })
            ),
            "{other}"
        );
    }
}

/// RED (U-15) — **the capability is the flag and the signature, both.**
///
/// MUTATION: answer `Capable` before asking the flag.
#[test]
fn the_capability_needs_the_flag_and_the_signature() {
    let identity = || Ok(expectation(SUBJECT, IDENTITY));
    assert_eq!(capability(false, identity), Capability::NotFlagged);
    assert_eq!(
        capability(true, identity),
        Capability::Capable(Box::new(expectation(SUBJECT, IDENTITY)))
    );
    assert_eq!(
        capability(true, || Err(Refusal::Unsigned)),
        Capability::Unsigned
    );
    assert_eq!(
        capability(true, || Err(Refusal::RevocationUnknown)),
        Capability::Unidentified(Refusal::RevocationUnknown)
    );
}

/// PIN (U-15) — **the comparison's order is identity, subject, version,
/// machine**, and each difference is named.
#[test]
fn the_comparison_names_the_first_difference() {
    let expected = expectation(SUBJECT, IDENTITY);
    assert_eq!(compare(&expected, &expected), Ok(()));
    let mut other = expectation(
        "CN=Another Publisher, O=Folio Test Publisher, L=Example City, S=mi, C=US",
        OTHER_IDENTITY,
    );
    other.version = FileVersion([0, 4, 7, 0]);
    assert!(matches!(
        compare(&expected, &other),
        Err(Refusal::IdentityDiffers { .. })
    ));
    other.identity_oid = IDENTITY.to_owned();
    assert!(matches!(
        compare(&expected, &other),
        Err(Refusal::SubjectDiffers { .. })
    ));
    other.subject_dn = expected.subject_dn.clone();
    assert!(matches!(
        compare(&expected, &other),
        Err(Refusal::VersionDiffers { .. })
    ));
    other.version = expected.version;
    other.machine = Machine::ARM64;
    assert!(matches!(
        compare(&expected, &other),
        Err(Refusal::MachineDiffers { .. })
    ));
}

/// PIN (U-15) — **a PE header's machine type is read from `e_lfanew`**, and
/// a file that is not an image has none.
#[test]
fn the_machine_type_is_read_from_the_pe_header() {
    let mut head = vec![0u8; 0x200];
    head[..2].copy_from_slice(b"MZ");
    head[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    head[0x80..0x84].copy_from_slice(b"PE\0\0");
    head[0x84..0x86].copy_from_slice(&0xAA64u16.to_le_bytes());
    assert_eq!(Machine::of_image_header(&head), Some(Machine::ARM64));
    assert_eq!(Machine::of_image_header(b"folio"), None);
    assert_eq!(
        FileVersion::parse_four("0.4.6.0"),
        Some(FileVersion([0, 4, 6, 0]))
    );
    assert_eq!(FileVersion::parse_four("0.4.6"), None);
}

#[cfg(windows)]
#[path = "trust_tests_windows.rs"]
mod windows_world;

/// Off Windows every door refuses by name.
#[cfg(not(windows))]
fn refused_off_windows() {
    let path = std::path::Path::new("folio.exe");
    let expected = expectation(SUBJECT, IDENTITY);
    assert_eq!(
        super::verify_release_file(path, &expected),
        Err(Refusal::Unsupported)
    );
    assert_eq!(
        super::verify_release_package(path, &expected),
        Err(Refusal::Unsupported)
    );
    assert_eq!(super::verify_sidecar(path), Err(Refusal::Unsupported));
    assert_eq!(
        super::running_capability(true),
        Capability::Unidentified(Refusal::Unsupported)
    );
}

/// RED (U-15) — **a validly signed file of another product, another version
/// or another machine is refused, each by name** — and so are an unsigned
/// copy, a signature with no time stamp, and a changed byte.
///
/// The files are this test's own executable, given a `VERSIONINFO` and signed
/// in the test's folder by `SignerSignEx3` with certificates made in the
/// test; the real `WinVerifyTrust`, `CryptVerifyTimeStampSignature` and chain
/// engine decide, under an exclusive root (E-13). "Another product" is a
/// signer with another subject and identity (refused at the identity OID),
/// and one that carries this identity under another subject (refused at the
/// subject).
///
/// MUTATION: skip the version comparison in `compare` and `old.exe` passes.
#[test]
fn validly_signed_wrong_product_or_version_is_refused() {
    #[cfg(windows)]
    windows_world::validly_signed_wrong_product_or_version_is_refused();
    #[cfg(not(windows))]
    refused_off_windows();
}

/// RED (U-15) — **a subject is compared as parsed RDNs with the value's case
/// and the RDNs' order kept**: `S=mi` is not `S=MI`, and `CN, O` is not
/// `O, CN`.
///
/// The subject is read from the signed file's leaf as Windows prints it (`CN`
/// first, the order `PACKAGE_PUBLISHER` and `smoke.ps1` use) and parsed by
/// `msix::distinguished_name`; never a substring.
///
/// MUTATION: compare `describe_name(..).to_lowercase()` in `compare` and the
/// `S=MI` leaf passes.
#[test]
fn dn_values_preserve_case_and_order() {
    let parsed = crate::msix::distinguished_name(SUBJECT);
    assert_eq!(parsed[3], ("S".to_owned(), "mi".to_owned()));
    assert_eq!(parsed[0].0, "CN");
    #[cfg(windows)]
    windows_world::dn_values_preserve_case_and_order();
}

/// RED (U-15) — **the time stamp `sign.ps1` produces is the one this reads:
/// an RFC 3161 countersignature over the signature, SHA-256 imprint, and its
/// time is the authority's.**
///
/// `sign.ps1` runs `signtool sign /fd SHA256 /tr <url> /td SHA256`; signtool
/// signs through `mssign32`'s `SignerSignEx3` with `SIGNER_TIMESTAMP_RFC3161`
/// and the SHA-256 algorithm, which is the call this test makes against a
/// time-stamp authority it serves on the loopback interface. The same format
/// on a real release is measured by the `#[ignore]`d
/// `the_released_windows_assets_carry_an_identity_oid` (E-6). An unstamped
/// signature is refused.
///
/// MUTATION: return `Ok` from step 2 when `rfc3161` is `None` and the unstamped
/// file passes.
#[test]
fn timestamp_policy_accepts_the_packagers_format() {
    #[cfg(windows)]
    windows_world::timestamp_policy_accepts_the_packagers_format();
    #[cfg(not(windows))]
    refused_off_windows();
}

/// RED (U-15) — **an unsigned running build is not capable**, a flag alone is
/// not enough, and a signed one without an identity OID is not either.
///
/// This test's executable is the running build and cargo signs nothing, so
/// `running_capability(true)` reads the real file and answers `Unsigned`.
///
/// MUTATION: answer `Capable` for `Refusal::Unsigned` in `capability`.
#[test]
fn an_unsigned_running_build_is_not_capable() {
    #[cfg(windows)]
    windows_world::an_unsigned_running_build_is_not_capable();
    #[cfg(not(windows))]
    refused_off_windows();
}

/// RED (U-15) — **a new certificate with the same subject and identity OID,
/// and another key, is accepted** — Artifact Signing's everyday renewal (C8).
///
/// MUTATION: compare the leaf's DER (a thumbprint pin) and the renewal is
/// refused.
#[test]
fn a_new_certificate_with_the_same_subject_is_accepted() {
    #[cfg(windows)]
    windows_world::a_new_certificate_with_the_same_subject_is_accepted();
    #[cfg(not(windows))]
    refused_off_windows();
}

/// RED (U-15) — **the same subject with another identity-validation OID is
/// refused** (F-5: equal DNs are not continuity of the signing principal), and
/// a leaf with no identity OID is refused as having none.
///
/// MUTATION: skip the identity comparison in `compare`.
#[test]
fn same_dn_with_a_different_identity_is_refused() {
    #[cfg(windows)]
    windows_world::same_dn_with_a_different_identity_is_refused();
    #[cfg(not(windows))]
    refused_off_windows();
}

/// RED (U-15) — **a leaf that has expired since it signed is accepted when
/// the time stamp falls inside its validity**, and refused when it does not.
///
/// Artifact Signing's leaves live 72 hours; every release older than that is
/// in the first case. The chain is judged at the stamped time. `SignerSignEx3`
/// will not sign with a certificate that has already expired, so the test's
/// leaf lives fifteen seconds: it signs and is stamped inside them, and the
/// file is verified once they are over.
///
/// MUTATION: build the exclusive chain at the current time (`now`) instead of
/// the stamped time and the expired leaf is refused.
#[test]
fn an_expired_leaf_with_a_valid_timestamp_is_accepted() {
    #[cfg(windows)]
    windows_world::an_expired_leaf_with_a_valid_timestamp_is_accepted();
    #[cfg(not(windows))]
    refused_off_windows();
}

/// RED (U-15) — **the machine store is never written**: the test policy's
/// engine is built with an exclusive root store holding the one test root,
/// the test root chains under it and under nothing on this machine, and no
/// system store (current user or local machine: Root, CA, My,
/// TrustedPublisher, Trust), opened read-only, holds any certificate the test
/// made.
///
/// By construction as well: `trust` opens only memory stores, which
/// `bt-app`'s `update_tests::the_trust_door_opens_no_system_certificate_store`
/// pins over the source.
///
/// MUTATION: build the test engine without `hExclusiveRoot` and the test root
/// stops chaining (and `exclusive_roots` is no longer 1).
#[test]
fn the_machine_store_is_never_written() {
    #[cfg(windows)]
    windows_world::the_machine_store_is_never_written();
    #[cfg(not(windows))]
    refused_off_windows();
}

/// E-6 (U-15) — **a released `folio.exe` and `folio.msix` carry an
/// identity-validation OID, and pass every step against themselves under the
/// system policy**; the two Microsoft sidecars pass theirs.
///
/// Downloads the release named by `BT_TRUST_RELEASE_TAG` (the latest release
/// when it is unset) into a temporary folder, checks the archive against the
/// release's `SHA256SUMS.txt`, expands it, and reads the real signatures —
/// online, since revocation is checked. Nothing is committed: a signed Folio
/// names its publisher. Prints the OID's shape only.
#[cfg(windows)]
#[test]
#[ignore = "downloads a release from GitHub; run by hand (E-6), see scripts/ci/ignored-tests.txt"]
fn the_released_windows_assets_carry_an_identity_oid() {
    windows_world::the_released_windows_assets_carry_an_identity_oid();
}
