//! The release statement's tests (0.4.10 plan row 102): one claim each. The ones that sign do so
//! with the throwaway [`TEST_KEYS`] pair, whose seeds are below and whose PEM files are committed
//! beside the fixtures; the one that proves the embedded keys sign with the real keys reads the
//! two statements signed outside the repository.

use super::*;

/// The committed fixtures.
#[cfg(feature = "release-key")]
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/release-key");

/// The seed of `TEST-ONLY-live.pem`.
#[cfg(feature = "release-key")]
const TEST_LIVE_SEED: [u8; 32] = [
    0x10, 0x28, 0xb5, 0x4e, 0x7c, 0xc4, 0x77, 0xc9, 0xc6, 0x99, 0x8a, 0xf8, 0x19, 0x6f, 0x23, 0x61,
    0x52, 0x0e, 0xb9, 0xae, 0x44, 0x10, 0x0e, 0x3e, 0xe3, 0x19, 0x80, 0x93, 0x1a, 0x4e, 0x57, 0xe5,
];

/// The seed of `TEST-ONLY-spare.pem`.
#[cfg(feature = "release-key")]
const TEST_SPARE_SEED: [u8; 32] = [
    0x5d, 0x60, 0xcf, 0xb8, 0xc3, 0xf3, 0x9c, 0x85, 0x78, 0x3e, 0xfb, 0xb7, 0x30, 0x6c, 0xad, 0xc7,
    0x89, 0x13, 0x44, 0xbb, 0xd8, 0x65, 0x8e, 0x2f, 0xc3, 0x66, 0x9f, 0xb8, 0xa6, 0xf6, 0xac, 0x20,
];

#[cfg(feature = "release-key")]
fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{FIXTURES}/{name}"))
        .unwrap_or_else(|error| panic!("the fixture {name}: {error}"))
}

/// `statement` signed with the test key whose seed is `seed`.
#[cfg(feature = "release-key")]
fn signed(statement: &str, seed: [u8; 32]) -> Vec<u8> {
    let pair = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new(seed));
    pair.sk.sign(statement.as_bytes(), None).to_vec()
}

/// A statement for `0.4.10` signed by the test key `key`, retiring `retires`.
fn statement_by(key: &ReleaseKey, retires: Vec<KeyId>) -> Statement {
    let mut statement =
        Statement::read_statement(fixture_statement(key).as_bytes()).expect("the fixture reads");
    statement.version = "0.4.10".to_owned();
    statement.tag = "v0.4.10-preview".to_owned();
    statement.min_from = "0.4.6".to_owned();
    statement.retires = retires;
    statement
}

/// The test keys as a set.
#[cfg(feature = "release-key")]
const TEST_SET: KeySet<'static> = KeySet::of(&TEST_KEYS);

/// RED (0.4.10 plan row 102) — **the keys this build embeds verify the two statements signed
/// with the real live and spare keys outside the repository**, and each fixture is exactly the
/// statement [`fixture_statement`] writes for its slot.
///
/// The embedded bytes are the only thing that ties a build to the private keys; one wrong byte
/// in [`RELEASE_KEYS`] is a build that refuses every release, found here rather than on a
/// reader's machine.
///
/// MUTATION: change one byte of the live key in [`RELEASE_KEYS`]: the fixture names another
/// key id and is refused as an unknown key (`plant-102-embedded-key.py`).
#[cfg(feature = "release-key")]
#[test]
fn the_embedded_keys_verify_the_statements_signed_with_the_real_keys() {
    for (key, slot) in RELEASE_KEYS.iter().zip(["live", "spare"]) {
        assert_eq!(key.slot, slot);
        let text = fixture(&format!("statement-{slot}.txt"));
        assert_eq!(
            text,
            fixture_statement(key).into_bytes(),
            "statement-{slot}.txt is the fixture statement for the {slot} key"
        );
        let signature = fixture(&format!("statement-{slot}.txt.sig"));
        let statement = verify_statement(&text, &signature, &KeySet::EMBEDDED, &[])
            .unwrap_or_else(|error| panic!("the {slot} key's statement: {error}"));
        assert_eq!(statement.key, key.key_id());
    }
}

/// PIN (0.4.10 plan row 102) — **no build embeds a test key**: the test pair's private halves are
/// in this repository, so a build that trusted them would trust anyone.
///
/// MUTATION: put [`TEST_KEYS`]`[0]` in the spare slot of [`RELEASE_KEYS`]
/// (`plant-102-test-key-embedded.py`).
#[test]
fn this_build_embeds_no_test_key() {
    assert!(!embeds_test_keys());
    assert_ne!(
        RELEASE_KEYS[0].key_id(),
        RELEASE_KEYS[1].key_id(),
        "the live and spare keys are two keys"
    );
}

/// PIN (0.4.10 plan row 102) — **the test keys are the committed PEM files, and a statement
/// OpenSSL signed with one verifies here**: the release is signed with OpenSSL and verified with
/// this crate, so the two implementations meet on the committed bytes, not on this crate's own
/// signing.
#[cfg(feature = "release-key")]
#[test]
fn an_openssl_signature_by_a_test_key_verifies() {
    for ((key, seed), slot) in TEST_KEYS
        .iter()
        .zip([TEST_LIVE_SEED, TEST_SPARE_SEED])
        .zip(["live", "spare"])
    {
        let pem = String::from_utf8(fixture(&format!("TEST-ONLY-{slot}.pem"))).unwrap();
        let der = pem_body(&pem);
        assert_eq!(der.len(), 48, "an Ed25519 PKCS#8 private key");
        assert_eq!(der[..16], PKCS8_ED25519_PREFIX, "TEST-ONLY-{slot}.pem");
        assert_eq!(
            der[16..],
            seed,
            "the seed constant is TEST-ONLY-{slot}.pem's"
        );
        let pair = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new(seed));
        assert_eq!(
            *pair.pk, key.public,
            "TEST_KEYS' {slot} slot is the PEM's key"
        );

        let text = fixture(&format!("statement-test-{slot}.txt"));
        assert_eq!(text, fixture_statement(key).into_bytes());
        let signature = fixture(&format!("statement-test-{slot}.txt.sig"));
        verify_statement(&text, &signature, &TEST_SET, &[]).unwrap();
    }
}

/// The 16 bytes before the seed in an Ed25519 private key's PKCS#8 DER (RFC 8410).
#[cfg(feature = "release-key")]
const PKCS8_ED25519_PREFIX: [u8; 16] = [
    0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
];

/// The bytes of a one-block PEM file's base64 body.
#[cfg(feature = "release-key")]
fn pem_body(pem: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let body: String = pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    let mut bits = 0u32;
    let mut held = 0u32;
    let mut out = Vec::new();
    for byte in body.bytes().filter(|byte| *byte != b'=') {
        let value = ALPHABET
            .iter()
            .position(|letter| *letter == byte)
            .expect("base64") as u32;
        bits = (bits << 6) | value;
        held += 6;
        if held >= 8 {
            held -= 8;
            out.push((bits >> held) as u8);
            bits &= (1 << held) - 1;
        }
    }
    out
}

/// RED (0.4.10 plan row 102) — **one changed byte refuses the statement**: a changed digit in a
/// member's SHA-256 (text that still reads) and a changed bit in the signature both fail the
/// signature check.
///
/// MUTATION: return the read statement without calling `verify` (`plant-102-no-verify.py`).
#[cfg(feature = "release-key")]
#[test]
fn a_tampered_byte_is_refused() {
    let text = statement_by(&TEST_KEYS[0], Vec::new()).statement_text();
    let signature = signed(&text, TEST_LIVE_SEED);
    verify_statement(text.as_bytes(), &signature, &TEST_SET, &[]).expect("the untouched pair");

    let digest_at = text.rfind('\n').unwrap() - 1;
    let digest_at = text[..digest_at].rfind('\n').unwrap() + 1;
    let mut tampered = text.clone().into_bytes();
    tampered[digest_at] = if tampered[digest_at] == b'0' {
        b'1'
    } else {
        b'0'
    };
    Statement::read_statement(&tampered).expect("the tampered statement still reads");
    assert_eq!(
        verify_statement(&tampered, &signature, &TEST_SET, &[]).unwrap_err(),
        StatementError::BadSignature(TEST_KEYS[0].key_id())
    );

    let mut flipped = signature.clone();
    flipped[10] ^= 0x01;
    assert_eq!(
        verify_statement(text.as_bytes(), &flipped, &TEST_SET, &[]).unwrap_err(),
        StatementError::BadSignature(TEST_KEYS[0].key_id())
    );
    assert_eq!(
        verify_statement(text.as_bytes(), &signature[..63], &TEST_SET, &[]).unwrap_err(),
        StatementError::SignatureLength { found: 63 }
    );
}

/// PIN (0.4.10 plan row 102) — **a statement verifies with the key it names and no other**: one
/// signed by a key outside the set is an unknown key, and one that names the spare while signed
/// by the live key fails the signature, since the key id is inside the signed bytes.
#[cfg(feature = "release-key")]
#[test]
fn a_statement_verifies_with_the_key_it_names_only() {
    let text = statement_by(&TEST_KEYS[0], Vec::new()).statement_text();
    let signature = signed(&text, TEST_LIVE_SEED);
    assert_eq!(
        verify_statement(text.as_bytes(), &signature, &KeySet::EMBEDDED, &[]).unwrap_err(),
        StatementError::UnknownKey(TEST_KEYS[0].key_id()),
        "the embedded set does not hold a test key"
    );

    let named_spare = statement_by(&TEST_KEYS[1], Vec::new()).statement_text();
    let by_live = signed(&named_spare, TEST_LIVE_SEED);
    assert_eq!(
        verify_statement(named_spare.as_bytes(), &by_live, &TEST_SET, &[]).unwrap_err(),
        StatementError::BadSignature(TEST_KEYS[1].key_id())
    );
}

/// RED (0.4.10 plan row 102) — **a retired key is refused**: the spare's statement that retires
/// the live key verifies and says so, and once that is remembered a statement the live key signs
/// is refused before its signature is checked.
///
/// MUTATION: drop the `retired.contains` check in [`verify_statement`]
/// (`plant-102-retired-accepted.py`).
#[cfg(feature = "release-key")]
#[test]
fn a_retired_key_is_refused() {
    let live = TEST_KEYS[0].key_id();
    let retiring = statement_by(&TEST_KEYS[1], vec![live]).statement_text();
    let retired = verify_statement(
        retiring.as_bytes(),
        &signed(&retiring, TEST_SPARE_SEED),
        &TEST_SET,
        &[],
    )
    .expect("the spare may retire the live key");
    assert_eq!(retired.retires, vec![live]);

    let by_live = statement_by(&TEST_KEYS[0], Vec::new()).statement_text();
    let signature = signed(&by_live, TEST_LIVE_SEED);
    verify_statement(by_live.as_bytes(), &signature, &TEST_SET, &[])
        .expect("before the retire is remembered");
    assert_eq!(
        verify_statement(by_live.as_bytes(), &signature, &TEST_SET, &retired.retires).unwrap_err(),
        StatementError::RetiredKey(live)
    );
}

/// RED (0.4.10 plan row 102) — **a statement retires neither its own signer nor a key the build
/// does not embed**: the first would lock every copy out of the next release; the second is a
/// line that names nothing this build can act on.
///
/// MUTATION: drop the own-signer check in [`Statement::read_statement`]
/// (`plant-102-retire-own-signer.py`).
#[cfg(feature = "release-key")]
#[test]
fn a_statement_retires_neither_its_signer_nor_an_unknown_key() {
    let own = statement_by(&TEST_KEYS[0], vec![TEST_KEYS[0].key_id()]).statement_text();
    let error = Statement::read_statement(own.as_bytes()).unwrap_err();
    assert!(
        matches!(&error, StatementError::Malformed(detail) if detail.contains("the key that signs it")),
        "{error}"
    );

    let stranger = KeyId::of_public(&[7; 32]);
    let unknown = statement_by(&TEST_KEYS[0], vec![stranger]).statement_text();
    assert_eq!(
        verify_statement(
            unknown.as_bytes(),
            &signed(&unknown, TEST_LIVE_SEED),
            &TEST_SET,
            &[]
        )
        .unwrap_err(),
        StatementError::RetiresUnknownKey(stranger)
    );
}

/// RED (0.4.10 plan row 102) — **a statement for another tag or another version vouches for
/// nothing here**: a valid statement of 0.4.9 published beside the files of 0.4.10 is refused.
///
/// MUTATION: drop the tag comparison in [`Statement::vouches_for`]
/// (`plant-102-other-tag.py`).
#[test]
fn a_statement_for_another_tag_or_version_is_refused() {
    let statement = statement_by(&TEST_KEYS[0], Vec::new());
    let offer = |tag, version| OfferedRelease {
        tag,
        version,
        running: "0.4.9",
    };
    statement
        .vouches_for(&offer("v0.4.10-preview", "0.4.10"))
        .expect("its own release");
    assert_eq!(
        statement
            .vouches_for(&offer("v0.4.11-preview", "0.4.10"))
            .unwrap_err(),
        StatementError::OtherTag {
            expected: "v0.4.11-preview".to_owned(),
            found: "v0.4.10-preview".to_owned(),
        }
    );
    assert_eq!(
        statement
            .vouches_for(&offer("v0.4.10-preview", "0.4.11"))
            .unwrap_err(),
        StatementError::OtherVersion {
            expected: "0.4.11".to_owned(),
            found: "0.4.10".to_owned(),
        }
    );
}

/// RED (0.4.10 plan row 102) — **`min_from` above this build is too old, compared as three
/// numbers**: 0.4.9 is below 0.4.10, which a comparison of the text would invert.
///
/// MUTATION: compare `min_from` and the running version as strings
/// (`plant-102-min-from-as-text.py`).
#[test]
fn min_from_above_this_build_is_too_old() {
    let mut statement = statement_by(&TEST_KEYS[0], Vec::new());
    statement.min_from = "0.4.10".to_owned();
    let offer = |running| OfferedRelease {
        tag: "v0.4.10-preview",
        version: "0.4.10",
        running,
    };
    assert_eq!(
        statement.vouches_for(&offer("0.4.9")).unwrap_err(),
        StatementError::TooOld {
            needs: "0.4.10".to_owned(),
            this: "0.4.9".to_owned(),
        }
    );
    statement
        .vouches_for(&offer("0.4.10"))
        .expect("min_from is the oldest allowed, itself included");
    statement.min_from = "0.4.6".to_owned();
    statement
        .vouches_for(&offer("0.4.9"))
        .expect("0.4.9 >= 0.4.6");
    assert_eq!(
        statement.vouches_for(&offer("0.4")).unwrap_err(),
        StatementError::RunningVersionUnreadable("0.4".to_owned())
    );
}

/// RED (0.4.10 plan row 102) — **an archive whose members are not the statement's is refused**:
/// a member missing, a member the statement does not list, and a member with other bytes.
///
/// MUTATION: drop the check for a member the statement does not list
/// (`plant-102-extra-member.py`).
#[test]
fn a_member_list_that_differs_is_refused() {
    let statement = statement_by(&TEST_KEYS[0], Vec::new());
    let listed = statement.members.clone();
    statement.members_match(&listed).expect("the same members");

    assert!(matches!(
        statement.members_match(&[]).unwrap_err(),
        StatementError::MembersDiffer(detail) if detail.contains("listed and not in the archive")
    ));

    let mut extra = listed.clone();
    extra.push(Member {
        name: "planted.dll".to_owned(),
        sha256: hex(&sha256(b"planted")),
        size: 7,
    });
    assert!(matches!(
        statement.members_match(&extra).unwrap_err(),
        StatementError::MembersDiffer(detail) if detail.contains("planted.dll is in the archive and not listed")
    ));

    let mut other = listed;
    other[0].sha256 = hex(&sha256(b"another folio.exe"));
    assert!(matches!(
        statement.members_match(&other).unwrap_err(),
        StatementError::MembersDiffer(detail) if detail.starts_with("folio.exe is")
    ));
}

/// RED (0.4.10 plan row 102) — **the text reads back as written, and a statement cut short or in
/// another format is refused by name**: every cut at a line end is refused, never read as a
/// smaller release; a CR, a second format and a line after the last member are refused.
///
/// MUTATION: accept a text without its final newline (`plant-102-cut-accepted.py`).
#[test]
fn the_text_reads_back_and_a_cut_statement_is_refused() {
    let statement = statement_by(&TEST_KEYS[0], vec![TEST_KEYS[1].key_id()]);
    let text = statement.statement_text();
    assert_eq!(
        Statement::read_statement(text.as_bytes()).unwrap(),
        statement
    );

    for (at, _) in text.match_indices('\n') {
        for cut in [&text[..at], &text[..=at]] {
            if cut.len() == text.len() {
                continue;
            }
            let error = Statement::read_statement(cut.as_bytes()).unwrap_err();
            assert!(
                matches!(error, StatementError::Truncated(_)),
                "a statement cut to {} bytes: {error}",
                cut.len()
            );
        }
    }
    assert!(matches!(
        Statement::read_statement(text.replace("statement 1", "statement 2").as_bytes())
            .unwrap_err(),
        StatementError::UnknownFormat(_)
    ));
    assert!(matches!(
        Statement::read_statement(text.replace('\n', "\r\n").as_bytes()).unwrap_err(),
        StatementError::Malformed(_)
    ));
    assert!(matches!(
        Statement::read_statement(format!("{text}more\n").as_bytes()).unwrap_err(),
        StatementError::Malformed(_)
    ));
    assert!(matches!(
        Statement::read_statement(
            text.replace("tag v0.4.10-preview", "tag v0.4.11-preview")
                .as_bytes()
        )
        .unwrap_err(),
        StatementError::Malformed(detail) if detail.contains("does not name the version")
    ));
    assert_eq!(
        KeyId::parse_key_id(&TEST_KEYS[0].key_id().to_string()),
        Some(TEST_KEYS[0].key_id())
    );
    assert_eq!(KeyId::parse_key_id("42BE5D87140DC304"), None);
}

/// A stored zip archive of `entries`, as a zip writer lays it out.
#[cfg(all(feature = "release-key", feature = "inflate"))]
fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for (name, data) in entries {
        let offset = out.len() as u32;
        let crc = crate::zip::crc32(data);
        let fixed = |out: &mut Vec<u8>| {
            out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        };
        out.extend_from_slice(&crate::zip::LOCAL_FILE_HEADER.to_le_bytes());
        fixed(&mut out);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        directory.extend_from_slice(&crate::zip::CENTRAL_DIRECTORY_HEADER.to_le_bytes());
        directory.extend_from_slice(&20u16.to_le_bytes());
        fixed(&mut directory);
        directory.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
        directory.extend_from_slice(&0u32.to_le_bytes());
        directory.extend_from_slice(&offset.to_le_bytes());
        directory.extend_from_slice(name.as_bytes());
    }
    let at = out.len() as u32;
    let count = (entries.len() as u16).to_le_bytes();
    out.extend_from_slice(&directory);
    out.extend_from_slice(&crate::zip::END_OF_CENTRAL_DIRECTORY.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0, count[0], count[1], count[0], count[1]]);
    out.extend_from_slice(&(directory.len() as u32).to_le_bytes());
    out.extend_from_slice(&at.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// RED (0.4.10 plan row 102) — **the release check reads a published release whole**: the
/// statement verified, every asset its listed size and SHA-256, and the Windows archive's
/// members the statement's; a changed asset byte or a member slipped into the archive is
/// refused.
///
/// MUTATION: skip the members of the archive in [`check_published`]
/// (`plant-102-check-skips-members.py`).
#[cfg(all(feature = "release-key", feature = "inflate"))]
#[test]
fn the_release_check_reads_a_published_release_whole() {
    let exe: &[u8] = b"the 0.4.10 folio.exe";
    let notice: &[u8] = b"MIT";
    let image: &[u8] = b"the 0.4.10 macOS image";
    let archive = stored_zip(&[
        ("folio-0.4.10/", b""),
        ("folio-0.4.10/folio.exe", exe),
        ("folio-0.4.10/LICENSE-MIT", notice),
    ]);
    let member = |name: &str, bytes: &[u8]| Member {
        name: name.to_owned(),
        sha256: hex(&sha256(bytes)),
        size: bytes.len() as u64,
    };
    let asset = |platform: &str, name: &str, bytes: &[u8]| StatementAsset {
        sha256: hex(&sha256(bytes)),
        size: bytes.len() as u64,
        platform: platform.to_owned(),
        name: name.to_owned(),
    };
    let mut statement = statement_by(&TEST_KEYS[0], Vec::new());
    statement.assets = vec![
        asset(
            WINDOWS_X64,
            "folio-0.4.10-windows-x64-unsigned.zip",
            &archive,
        ),
        asset(MACOS_ARM64, "Folio-0.4.10-macos-arm64.dmg", image),
    ];
    statement.members = vec![member("folio.exe", exe), member("LICENSE-MIT", notice)];
    let text = statement.statement_text();
    let signature = signed(&text, TEST_LIVE_SEED);

    let published = |archive: Vec<u8>| {
        let text = text.clone();
        let signature = signature.clone();
        move |name: &str| -> Result<Vec<u8>, String> {
            match name {
                STATEMENT_FILE => Ok(text.clone().into_bytes()),
                SIGNATURE_FILE => Ok(signature.clone()),
                "folio-0.4.10-windows-x64-unsigned.zip" => Ok(archive.clone()),
                "Folio-0.4.10-macos-arm64.dmg" => Ok(image.to_vec()),
                other => Err(format!("{other} is not published")),
            }
        }
    };
    let checked = check_published(
        &mut published(archive.clone()),
        "v0.4.10-preview",
        &TEST_SET,
    )
    .unwrap();
    assert_eq!(checked, statement);

    let mut changed = archive.clone();
    let last = changed.len() - 30;
    changed[last] ^= 0x01;
    assert!(matches!(
        check_published(&mut published(changed), "v0.4.10-preview", &TEST_SET).unwrap_err(),
        StatementError::AssetDiffers(_)
    ));

    let slipped = stored_zip(&[
        ("folio-0.4.10/folio.exe", exe),
        ("folio-0.4.10/LICENSE-MIT", notice),
        ("folio-0.4.10/planted.dll", b"planted"),
    ]);
    let mut resigned = statement.clone();
    resigned.assets[0] = asset(
        WINDOWS_X64,
        "folio-0.4.10-windows-x64-unsigned.zip",
        &slipped,
    );
    let resigned_text = resigned.statement_text();
    let resigned_signature = signed(&resigned_text, TEST_LIVE_SEED);
    let mut read = |name: &str| -> Result<Vec<u8>, String> {
        match name {
            STATEMENT_FILE => Ok(resigned_text.clone().into_bytes()),
            SIGNATURE_FILE => Ok(resigned_signature.clone()),
            "folio-0.4.10-windows-x64-unsigned.zip" => Ok(slipped.clone()),
            _ => Ok(image.to_vec()),
        }
    };
    assert!(matches!(
        check_published(&mut read, "v0.4.10-preview", &TEST_SET).unwrap_err(),
        StatementError::MembersDiffer(detail) if detail.contains("planted.dll")
    ));
    assert_eq!(
        check_published(&mut published(archive), "v0.4.11-preview", &TEST_SET).unwrap_err(),
        StatementError::OtherTag {
            expected: "v0.4.11-preview".to_owned(),
            found: "v0.4.10-preview".to_owned(),
        }
    );
}
