//! The test world of [`crate::trust`] on Windows (E-13): a root, leaves and a
//! time-stamp authority made in the test with ephemeral keys, a PE file
//! copied from this test's own executable and signed in the test's folder by
//! `SignerSignEx3`, time-stamped over HTTP by a time-stamp authority this test
//! serves on the loopback interface, and verified under an exclusive-root
//! engine. Every key is ephemeral (`NCryptCreatePersistedKey` with no name),
//! every store is a memory store, and nothing is installed.

use std::ffi::c_void;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::time::{SystemTime, UNIX_EPOCH};

use windows::Win32::Foundation::{FILETIME, HANDLE};
use windows::Win32::Security::Cryptography::{
    BCRYPT_RSA_ALGORITHM, CALG_SHA_256, CERT_BASIC_CONSTRAINTS2_INFO, CERT_CONTEXT, CERT_EXTENSION,
    CERT_INFO, CERT_KEY_CONTEXT, CERT_KEY_CONTEXT_0, CERT_KEY_CONTEXT_PROP_ID,
    CERT_NAME_STR_REVERSE_FLAG, CERT_NCRYPT_KEY_SPEC, CERT_PUBLIC_KEY_INFO,
    CERT_SET_PROPERTY_INHIBIT_PERSIST_FLAG, CERT_SHA1_HASH_PROP_ID, CERT_STORE_ADD_ALWAYS,
    CERT_STORE_NO_CRYPT_RELEASE_FLAG, CERT_STRING_TYPE, CERT_V3, CERT_X500_NAME_STR,
    CMSG_CMS_ENCAPSULATED_CONTENT_FLAG, CMSG_CONTENT_PARAM, CMSG_SIGNED, CMSG_SIGNED_ENCODE_INFO,
    CMSG_SIGNER_ENCODE_INFO, CMSG_SIGNER_ENCODE_INFO_0, CRL_ENTRY, CRL_INFO, CRL_V2,
    CRYPT_ALGORITHM_IDENTIFIER, CRYPT_ATTRIBUTE, CRYPT_BIT_BLOB, CRYPT_INTEGER_BLOB, CTL_USAGE,
    CertAddEncodedCertificateToStore, CertCreateCertificateContext, CertFreeCertificateContext,
    CertGetCertificateContextProperty, CertSetCertificateContextProperty, CertStrToNameW,
    CryptEncodeObject, CryptExportPublicKeyInfo, CryptMsgClose, CryptMsgGetParam,
    CryptMsgOpenToEncode, CryptMsgUpdate, CryptSignAndEncodeCertificate,
    HCRYPTPROV_OR_NCRYPT_KEY_HANDLE, MS_KEY_STORAGE_PROVIDER, NCRYPT_FLAGS, NCRYPT_HANDLE,
    NCRYPT_KEY_HANDLE, NCRYPT_LENGTH_PROPERTY, NCRYPT_PROV_HANDLE, NCryptCreatePersistedKey,
    NCryptFinalizeKey, NCryptFreeObject, NCryptOpenStorageProvider, NCryptSetProperty,
    PKCS_7_ASN_ENCODING, SIGNER_CERT, SIGNER_CERT_0, SIGNER_CERT_POLICY_STORE, SIGNER_CERT_STORE,
    SIGNER_CERT_STORE_INFO, SIGNER_CONTEXT, SIGNER_FILE_INFO, SIGNER_NO_ATTR, SIGNER_SIGN_FLAGS,
    SIGNER_SIGNATURE_INFO, SIGNER_SUBJECT_FILE, SIGNER_SUBJECT_INFO, SIGNER_SUBJECT_INFO_0,
    SIGNER_TIMESTAMP_RFC3161, SignerFreeSignerContext, SignerSignEx3, X509_ASN_ENCODING,
    X509_BASIC_CONSTRAINTS2, X509_CERT_CRL_TO_BE_SIGNED, X509_CERT_TO_BE_SIGNED,
    X509_ENHANCED_KEY_USAGE,
};
use windows::Win32::System::LibraryLoader::{
    BeginUpdateResourceW, EndUpdateResourceW, UpdateResourceW,
};
use windows::core::{PCSTR, PCWSTR, PSTR};

use crate::trust::{
    CODE_SIGNING, Capability, Expectation, FileVersion, Machine, PUBLIC_TRUST_MARKER, Policy,
    Refusal, TIME_STAMPING, capability_of, identity_of, signature, verify_release_file_under,
};

/// `sha256RSA`.
const SHA256_RSA: &str = "1.2.840.113549.1.1.11";
/// `id-sha256`.
const SHA256: &str = "2.16.840.1.101.3.4.2.1";
/// `id-ct-TSTInfo`.
const TST_INFO: &str = "1.2.840.113549.1.9.16.1.4";
/// ESS `signingCertificate`.
const SIGNING_CERTIFICATE: &str = "1.2.840.113549.1.9.16.2.12";

/// One day, in `FILETIME` ticks.
pub const DAY: u64 = 24 * 3600 * 10_000_000;

/// Now, in `FILETIME` ticks.
pub fn now() -> u64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    (since.as_secs() + 11_644_473_600) * 10_000_000 + u64::from(since.subsec_nanos() / 100)
}

fn filetime(ticks: u64) -> FILETIME {
    FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    }
}

fn cstr(text: &str) -> Vec<u8> {
    format!("{text}\0").into_bytes()
}

fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    text.encode_wide().chain([0]).collect()
}

// ── keys ────────────────────────────────────────────────────────────────────

/// An ephemeral RSA-2048 key: created with no name, so the key storage
/// provider persists nothing, and freed on drop.
pub struct Key(NCRYPT_KEY_HANDLE);

impl Key {
    pub fn new() -> Self {
        let mut provider = NCRYPT_PROV_HANDLE::default();
        let mut key = NCRYPT_KEY_HANDLE::default();
        // SAFETY: outputs are live; no name means an ephemeral key.
        unsafe {
            NCryptOpenStorageProvider(&mut provider, MS_KEY_STORAGE_PROVIDER, 0).unwrap();
            NCryptCreatePersistedKey(
                provider,
                &mut key,
                BCRYPT_RSA_ALGORITHM,
                PCWSTR::null(),
                windows::Win32::Security::Cryptography::CERT_KEY_SPEC(0),
                NCRYPT_FLAGS(0),
            )
            .unwrap();
            NCryptSetProperty(
                NCRYPT_HANDLE(key.0),
                NCRYPT_LENGTH_PROPERTY,
                &2048u32.to_le_bytes(),
                NCRYPT_FLAGS(0),
            )
            .unwrap();
            NCryptFinalizeKey(key, NCRYPT_FLAGS(0)).unwrap();
            NCryptFreeObject(NCRYPT_HANDLE(provider.0)).unwrap();
        }
        Self(key)
    }

    fn handle(&self) -> HCRYPTPROV_OR_NCRYPT_KEY_HANDLE {
        HCRYPTPROV_OR_NCRYPT_KEY_HANDLE(self.0.0)
    }

    /// The `SubjectPublicKeyInfo` of this key, as the bytes CryptoAPI lays the
    /// structure out in (the structure points into its own buffer).
    fn public_key_info(&self) -> Vec<u64> {
        let mut size = 0u32;
        // SAFETY: the size query, then the fill into an aligned buffer.
        unsafe {
            CryptExportPublicKeyInfo(self.handle(), Some(0), X509_ASN_ENCODING, None, &mut size)
                .unwrap();
            let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
            CryptExportPublicKeyInfo(
                self.handle(),
                Some(0),
                X509_ASN_ENCODING,
                Some(buffer.as_mut_ptr().cast()),
                &mut size,
            )
            .unwrap();
            buffer
        }
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: made above and freed once.
        unsafe {
            let _ = NCryptFreeObject(NCRYPT_HANDLE(self.0.0));
        }
    }
}

// ── names and extensions ────────────────────────────────────────────────────

/// A distinguished name encoded the way Windows prints it back
/// (`CERT_NAME_STR_REVERSE_FLAG` both ways: `CN` first when printed).
pub fn encode_name(dn: &str) -> Vec<u8> {
    let text = wide(std::ffi::OsStr::new(dn));
    let kind = CERT_STRING_TYPE(CERT_X500_NAME_STR.0 | CERT_NAME_STR_REVERSE_FLAG);
    let mut size = 0u32;
    // SAFETY: the size query, then the fill.
    unsafe {
        CertStrToNameW(
            X509_ASN_ENCODING,
            PCWSTR(text.as_ptr()),
            kind,
            None,
            None,
            &mut size,
            None,
        )
        .unwrap();
        let mut out = vec![0u8; size as usize];
        CertStrToNameW(
            X509_ASN_ENCODING,
            PCWSTR(text.as_ptr()),
            kind,
            None,
            Some(out.as_mut_ptr()),
            &mut size,
            None,
        )
        .unwrap();
        out.truncate(size as usize);
        out
    }
}

fn encode(kind: PCSTR, info: *const c_void) -> Vec<u8> {
    let mut size = 0u32;
    // SAFETY: `info` is the structure `kind` names, live across both calls.
    unsafe {
        CryptEncodeObject(X509_ASN_ENCODING, kind, info, None, &mut size).unwrap();
        let mut out = vec![0u8; size as usize];
        CryptEncodeObject(
            X509_ASN_ENCODING,
            kind,
            info,
            Some(out.as_mut_ptr()),
            &mut size,
        )
        .unwrap();
        out.truncate(size as usize);
        out
    }
}

fn key_usage_extension(usages: &[&str]) -> Vec<u8> {
    let mut strings: Vec<Vec<u8>> = usages.iter().map(|oid| cstr(oid)).collect();
    let mut pointers: Vec<PSTR> = strings.iter_mut().map(|s| PSTR(s.as_mut_ptr())).collect();
    let usage = CTL_USAGE {
        cUsageIdentifier: pointers.len() as u32,
        rgpszUsageIdentifier: pointers.as_mut_ptr(),
    };
    encode(X509_ENHANCED_KEY_USAGE, (&raw const usage).cast())
}

fn basic_constraints(ca: bool) -> Vec<u8> {
    let info = CERT_BASIC_CONSTRAINTS2_INFO {
        fCA: ca.into(),
        ..Default::default()
    };
    encode(X509_BASIC_CONSTRAINTS2, (&raw const info).cast())
}

// ── certificates ────────────────────────────────────────────────────────────

/// What a certificate made here is.
pub struct Spec<'a> {
    pub subject: &'a str,
    pub serial: u64,
    pub not_before: u64,
    pub not_after: u64,
    pub ca: bool,
    pub usages: &'a [&'a str],
    /// Mark the key-usage extension critical (a time-stamp authority's must be).
    pub critical_usages: bool,
}

/// A certificate made here, with the key it certifies.
pub struct Made {
    pub der: Vec<u8>,
    pub key: Key,
    pub subject: String,
}

fn serial_blob(serial: &[u8; 8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: 8,
        pbData: serial.as_ptr().cast_mut(),
    }
}

fn sign_structure(kind: PCSTR, info: *const c_void, signer: &Key) -> Vec<u8> {
    let mut algorithm_oid = cstr(SHA256_RSA);
    let algorithm = CRYPT_ALGORITHM_IDENTIFIER {
        pszObjId: PSTR(algorithm_oid.as_mut_ptr()),
        Parameters: CRYPT_INTEGER_BLOB::default(),
    };
    let mut size = 0u32;
    // SAFETY: `info` is the structure `kind` names; the key is live.
    unsafe {
        CryptSignAndEncodeCertificate(
            Some(signer.handle()),
            Some(CERT_NCRYPT_KEY_SPEC),
            X509_ASN_ENCODING,
            kind,
            info,
            &algorithm,
            None,
            None,
            &mut size,
        )
        .unwrap();
        let mut out = vec![0u8; size as usize];
        CryptSignAndEncodeCertificate(
            Some(signer.handle()),
            Some(CERT_NCRYPT_KEY_SPEC),
            X509_ASN_ENCODING,
            kind,
            info,
            &algorithm,
            None,
            Some(out.as_mut_ptr()),
            &mut size,
        )
        .unwrap();
        out.truncate(size as usize);
        out
    }
}

/// **A certificate for `spec`**, issued by `issuer` (subject and key), or
/// self-signed when there is none.
pub fn issue(spec: &Spec<'_>, issuer: Option<&Made>) -> Made {
    let key = Key::new();
    let public = key.public_key_info();
    // SAFETY: `public` holds a `CERT_PUBLIC_KEY_INFO` and what it points at.
    let public_info = unsafe { *public.as_ptr().cast::<CERT_PUBLIC_KEY_INFO>() };
    let subject = encode_name(spec.subject);
    let issuer_name = encode_name(issuer.map_or(spec.subject, |made| made.subject.as_str()));
    let serial = spec.serial.to_le_bytes();
    let mut serial_positive = serial;
    serial_positive[7] &= 0x7F;
    let mut algorithm_oid = cstr(SHA256_RSA);

    let mut usage_oid = cstr("2.5.29.37");
    let mut constraints_oid = cstr("2.5.29.19");
    let mut usages = key_usage_extension(spec.usages);
    let mut constraints = basic_constraints(spec.ca);
    let mut extensions = vec![CERT_EXTENSION {
        pszObjId: PSTR(constraints_oid.as_mut_ptr()),
        fCritical: true.into(),
        Value: CRYPT_INTEGER_BLOB {
            cbData: constraints.len() as u32,
            pbData: constraints.as_mut_ptr(),
        },
    }];
    if !spec.usages.is_empty() {
        extensions.push(CERT_EXTENSION {
            pszObjId: PSTR(usage_oid.as_mut_ptr()),
            fCritical: spec.critical_usages.into(),
            Value: CRYPT_INTEGER_BLOB {
                cbData: usages.len() as u32,
                pbData: usages.as_mut_ptr(),
            },
        });
    }
    let info = CERT_INFO {
        dwVersion: CERT_V3,
        SerialNumber: serial_blob(&serial_positive),
        SignatureAlgorithm: CRYPT_ALGORITHM_IDENTIFIER {
            pszObjId: PSTR(algorithm_oid.as_mut_ptr()),
            Parameters: CRYPT_INTEGER_BLOB::default(),
        },
        Issuer: CRYPT_INTEGER_BLOB {
            cbData: issuer_name.len() as u32,
            pbData: issuer_name.as_ptr().cast_mut(),
        },
        NotBefore: filetime(spec.not_before),
        NotAfter: filetime(spec.not_after),
        Subject: CRYPT_INTEGER_BLOB {
            cbData: subject.len() as u32,
            pbData: subject.as_ptr().cast_mut(),
        },
        SubjectPublicKeyInfo: public_info,
        IssuerUniqueId: CRYPT_BIT_BLOB::default(),
        SubjectUniqueId: CRYPT_BIT_BLOB::default(),
        cExtension: extensions.len() as u32,
        rgExtension: extensions.as_mut_ptr(),
    };
    let signer = issuer.map_or(&key, |made| &made.key);
    let der = sign_structure(X509_CERT_TO_BE_SIGNED, (&raw const info).cast(), signer);
    drop(public);
    Made {
        der,
        key,
        subject: spec.subject.to_owned(),
    }
}

/// **A revocation list** `issuer` publishes, revoking `serials`, current from
/// yesterday to a week from now.
pub fn revocation_list(issuer: &Made, serials: &[u64]) -> Vec<u8> {
    let name = encode_name(&issuer.subject);
    let mut algorithm_oid = cstr(SHA256_RSA);
    let numbers: Vec<[u8; 8]> = serials
        .iter()
        .map(|serial| {
            let mut bytes = serial.to_le_bytes();
            bytes[7] &= 0x7F;
            bytes
        })
        .collect();
    let mut entries: Vec<CRL_ENTRY> = numbers
        .iter()
        .map(|bytes| CRL_ENTRY {
            SerialNumber: serial_blob(bytes),
            RevocationDate: filetime(now() - DAY),
            cExtension: 0,
            rgExtension: null_mut(),
        })
        .collect();
    let info = CRL_INFO {
        dwVersion: CRL_V2,
        SignatureAlgorithm: CRYPT_ALGORITHM_IDENTIFIER {
            pszObjId: PSTR(algorithm_oid.as_mut_ptr()),
            Parameters: CRYPT_INTEGER_BLOB::default(),
        },
        Issuer: CRYPT_INTEGER_BLOB {
            cbData: name.len() as u32,
            pbData: name.as_ptr().cast_mut(),
        },
        ThisUpdate: filetime(now() - DAY),
        NextUpdate: filetime(now() + 7 * DAY),
        cCRLEntry: entries.len() as u32,
        rgCRLEntry: entries.as_mut_ptr(),
        cExtension: 0,
        rgExtension: null_mut(),
    };
    sign_structure(
        X509_CERT_CRL_TO_BE_SIGNED,
        (&raw const info).cast(),
        &issuer.key,
    )
}

// ── DER, as much as a time-stamp authority needs ────────────────────────────

fn der(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let length = content.len();
    if length < 0x80 {
        out.push(length as u8);
    } else {
        let bytes: Vec<u8> = length
            .to_be_bytes()
            .into_iter()
            .skip_while(|&byte| byte == 0)
            .collect();
        out.push(0x80 | bytes.len() as u8);
        out.extend(bytes);
    }
    out.extend_from_slice(content);
    out
}

/// One element at the front of `bytes`: its tag, and the whole element.
fn element(bytes: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let tag = *bytes.first()?;
    let first = *bytes.get(1)?;
    let (length, header) = if first < 0x80 {
        (first as usize, 2)
    } else {
        let count = (first & 0x7F) as usize;
        let mut length = 0usize;
        for byte in bytes.get(2..2 + count)? {
            length = (length << 8) | *byte as usize;
        }
        (length, 2 + count)
    };
    let whole = bytes.get(..header + length)?;
    Some((tag, whole, &whole[header..]))
}

fn children(content: &[u8]) -> Vec<(u8, &[u8])> {
    let mut out = Vec::new();
    let mut rest = content;
    while let Some((tag, whole, _)) = element(rest) {
        out.push((tag, whole));
        rest = &rest[whole.len()..];
    }
    out
}

fn generalized_time(ticks: u64) -> Vec<u8> {
    let seconds = ticks / 10_000_000 - 11_644_473_600;
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let text = format!(
        "{year:04}{month:02}{day:02}{:02}{:02}{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    );
    der(0x18, text.as_bytes())
}

// ── the time-stamp authority ────────────────────────────────────────────────

/// **A time-stamp authority** answering RFC 3161 requests over HTTP on the
/// loopback interface, stamping every request with `at`.
pub struct Authority<'a> {
    pub certificate: &'a Made,
    pub at: u64,
}

impl Authority<'_> {
    fn sha1(&self) -> Vec<u8> {
        // SAFETY: a context made from the certificate's bytes, freed below.
        unsafe {
            let context = CertCreateCertificateContext(X509_ASN_ENCODING, &self.certificate.der);
            let mut hash = vec![0u8; 20];
            let mut size = 20u32;
            CertGetCertificateContextProperty(
                context,
                CERT_SHA1_HASH_PROP_ID,
                Some(hash.as_mut_ptr().cast()),
                &mut size,
            )
            .unwrap();
            let _ = CertFreeCertificateContext(Some(context));
            hash
        }
    }

    /// The `TimeStampResp` to one `TimeStampReq`.
    fn respond(&self, request: &[u8]) -> Vec<u8> {
        let (_, _, content) = element(request).expect("a request");
        let fields = children(content);
        let imprint = fields
            .iter()
            .find(|(tag, _)| *tag == 0x30)
            .expect("an imprint")
            .1;
        // The version is the request's first INTEGER; a nonce, when there is
        // one, is the second.
        let nonce = fields
            .iter()
            .filter(|(tag, _)| *tag == 0x02)
            .nth(1)
            .map(|(_, whole)| *whole);
        let mut tst = Vec::new();
        tst.extend(der(0x02, &[1]));
        tst.extend(der(0x06, &[0x2A, 0x03, 0x04, 0x01])); // policy 1.2.3.4.1
        tst.extend_from_slice(imprint);
        tst.extend(der(0x02, &[0x01, 0x23, 0x45]));
        tst.extend(generalized_time(self.at));
        if let Some(nonce) = nonce {
            tst.extend_from_slice(nonce);
        }
        let tst = der(0x30, &tst);
        let token = self.token(&tst);
        let mut response = der(0x30, &der(0x02, &[0]));
        response.extend(token);
        der(0x30, &response)
    }

    /// The `ContentInfo` of a signed `TSTInfo`.
    fn token(&self, tst: &[u8]) -> Vec<u8> {
        let hash = self.sha1();
        let ess = der(0x30, &der(0x30, &der(0x30, &der(0x04, &hash))));
        let mut ess_value = CRYPT_INTEGER_BLOB {
            cbData: ess.len() as u32,
            pbData: ess.as_ptr().cast_mut(),
        };
        let mut ess_oid = cstr(SIGNING_CERTIFICATE);
        let mut attributes = [CRYPT_ATTRIBUTE {
            pszObjId: PSTR(ess_oid.as_mut_ptr()),
            cValue: 1,
            rgValue: &raw mut ess_value,
        }];
        let mut hash_oid = cstr(SHA256);
        // SAFETY: every structure below is live until the message is closed.
        unsafe {
            let context = CertCreateCertificateContext(X509_ASN_ENCODING, &self.certificate.der);
            let mut signer = CMSG_SIGNER_ENCODE_INFO {
                cbSize: size_of::<CMSG_SIGNER_ENCODE_INFO>() as u32,
                pCertInfo: (*context).pCertInfo,
                Anonymous: CMSG_SIGNER_ENCODE_INFO_0 {
                    hNCryptKey: self.certificate.key.0,
                },
                dwKeySpec: CERT_NCRYPT_KEY_SPEC.0,
                HashAlgorithm: CRYPT_ALGORITHM_IDENTIFIER {
                    pszObjId: PSTR(hash_oid.as_mut_ptr()),
                    Parameters: CRYPT_INTEGER_BLOB::default(),
                },
                pvHashAuxInfo: null_mut(),
                cAuthAttr: 1,
                rgAuthAttr: attributes.as_mut_ptr(),
                cUnauthAttr: 0,
                rgUnauthAttr: null_mut(),
            };
            let mut certificate = CRYPT_INTEGER_BLOB {
                cbData: self.certificate.der.len() as u32,
                pbData: self.certificate.der.as_ptr().cast_mut(),
            };
            let info = CMSG_SIGNED_ENCODE_INFO {
                cbSize: size_of::<CMSG_SIGNED_ENCODE_INFO>() as u32,
                cSigners: 1,
                rgSigners: &raw mut signer,
                cCertEncoded: 1,
                rgCertEncoded: &raw mut certificate,
                cCrlEncoded: 0,
                rgCrlEncoded: null_mut(),
            };
            let content_type = cstr(TST_INFO);
            let message = CryptMsgOpenToEncode(
                X509_ASN_ENCODING.0 | PKCS_7_ASN_ENCODING.0,
                CMSG_CMS_ENCAPSULATED_CONTENT_FLAG,
                CMSG_SIGNED,
                (&raw const info).cast(),
                PCSTR(content_type.as_ptr()),
                None,
            );
            assert!(
                !message.is_null(),
                "{}",
                windows::core::Error::from_thread()
            );
            CryptMsgUpdate(message, Some(tst), true).unwrap();
            let mut size = 0u32;
            CryptMsgGetParam(message, CMSG_CONTENT_PARAM, 0, None, &mut size).unwrap();
            let mut out = vec![0u8; size as usize];
            CryptMsgGetParam(
                message,
                CMSG_CONTENT_PARAM,
                0,
                Some(out.as_mut_ptr().cast()),
                &mut size,
            )
            .unwrap();
            out.truncate(size as usize);
            CryptMsgClose(Some(message)).unwrap();
            let _ = CertFreeCertificateContext(Some(context));
            out
        }
    }

    /// Answer `requests` HTTP requests on `listener`.
    fn serve(&self, listener: &TcpListener, requests: usize) {
        listener.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        for _ in 0..requests {
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if std::time::Instant::now() > deadline {
                            return;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            let mut received = Vec::new();
            let mut buffer = [0u8; 4096];
            let body = loop {
                let read = stream.read(&mut buffer).unwrap();
                assert!(read > 0, "the client hung up");
                received.extend_from_slice(&buffer[..read]);
                let Some(end) = received.windows(4).position(|w| w == b"\r\n\r\n") else {
                    continue;
                };
                let head = String::from_utf8_lossy(&received[..end]).to_ascii_lowercase();
                let length: usize = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .map_or(0, |value| value.trim().parse().unwrap());
                if received.len() >= end + 4 + length {
                    break received[end + 4..end + 4 + length].to_vec();
                }
            };
            let reply = self.respond(&body);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/timestamp-reply\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                reply.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(&reply).unwrap();
        }
    }
}

// ── the signed file ─────────────────────────────────────────────────────────

/// A test folder, removed when the test ends.
pub struct Folder(pub PathBuf);

impl Folder {
    pub fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("bt-platform-trust-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A `VS_VERSIONINFO` block holding only the fixed file info.
fn version_block(version: FileVersion) -> Vec<u8> {
    let [a, b, c, d] = version.0;
    let key: Vec<u16> = "VS_VERSION_INFO\0".encode_utf16().collect();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // wLength, below
    out.extend_from_slice(&52u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    for unit in key {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    while out.len() % 4 != 0 {
        out.push(0);
    }
    for word in [
        0xFEEF_04BDu32,
        0x0001_0000,
        (u32::from(a) << 16) | u32::from(b),
        (u32::from(c) << 16) | u32::from(d),
        (u32::from(a) << 16) | u32::from(b),
        (u32::from(c) << 16) | u32::from(d),
        0x3F,
        0,
        4,
        1,
        0,
        0,
        0,
    ] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    let length = out.len() as u16;
    out[..2].copy_from_slice(&length.to_le_bytes());
    out
}

/// **A PE file in `folder`**: a copy of this test's executable, carrying
/// `version` as its `VERSIONINFO`.
pub fn executable(folder: &Folder, name: &str, version: FileVersion) -> PathBuf {
    let path = folder.0.join(name);
    std::fs::copy(std::env::current_exe().unwrap(), &path).unwrap();
    let block = version_block(version);
    let file = wide(path.as_os_str());
    // SAFETY: the path is NUL-terminated; the block lives across the update.
    unsafe {
        let update = BeginUpdateResourceW(PCWSTR(file.as_ptr()), false).unwrap();
        UpdateResourceW(
            update,
            PCWSTR(16 as _),
            PCWSTR(1 as _),
            0x0409,
            Some(block.as_ptr().cast()),
            block.len() as u32,
        )
        .unwrap();
        EndUpdateResourceW(update, false).unwrap();
    }
    path
}

/// **Sign `path` with `leaf`**, putting `chain` in the signature, time-stamped
/// by `authority` when there is one (RFC 3161, SHA-256 imprint: `signtool
/// sign /fd SHA256 /tr <url> /td SHA256`, the form `sign.ps1` produces).
pub fn sign(path: &Path, leaf: &Made, chain: &[&Made], authority: Option<&Authority<'_>>) {
    assert!(
        path.starts_with(std::env::temp_dir()),
        "signs only inside the test's own folder"
    );
    let store = crate::trust::arm::Store::memory().unwrap();
    for made in chain {
        // SAFETY: the memory store just opened; the bytes are copied.
        unsafe {
            CertAddEncodedCertificateToStore(
                Some(store.0),
                X509_ASN_ENCODING,
                &made.der,
                CERT_STORE_ADD_ALWAYS,
                None,
            )
        }
        .unwrap();
    }
    let file = wide(path.as_os_str());
    // SAFETY: every structure is live across the signing call; the context and
    // the certificate are freed after it.
    unsafe {
        let context: *const CERT_CONTEXT =
            CertCreateCertificateContext(X509_ASN_ENCODING, &leaf.der);
        let key_context = CERT_KEY_CONTEXT {
            cbSize: size_of::<CERT_KEY_CONTEXT>() as u32,
            Anonymous: CERT_KEY_CONTEXT_0 {
                hNCryptKey: leaf.key.0,
            },
            dwKeySpec: CERT_NCRYPT_KEY_SPEC.0,
        };
        // Not handed over: `CERT_STORE_NO_CRYPT_RELEASE_FLAG` keeps the
        // context from freeing the key when it is freed (`Key` does that),
        // and `CERT_SET_PROPERTY_INHIBIT_PERSIST_FLAG` keeps the property on
        // this in-memory context.
        CertSetCertificateContextProperty(
            context,
            CERT_KEY_CONTEXT_PROP_ID,
            CERT_SET_PROPERTY_INHIBIT_PERSIST_FLAG | CERT_STORE_NO_CRYPT_RELEASE_FLAG.0,
            Some((&raw const key_context).cast()),
        )
        .unwrap();
        let mut store_info = SIGNER_CERT_STORE_INFO {
            cbSize: size_of::<SIGNER_CERT_STORE_INFO>() as u32,
            pSigningCert: context,
            dwCertPolicy: SIGNER_CERT_POLICY_STORE,
            hCertStore: store.0,
        };
        let certificate = SIGNER_CERT {
            cbSize: size_of::<SIGNER_CERT>() as u32,
            dwCertChoice: SIGNER_CERT_STORE,
            Anonymous: SIGNER_CERT_0 {
                pCertStoreInfo: &raw mut store_info,
            },
            hwnd: windows::Win32::Foundation::HWND::default(),
        };
        let mut file_info = SIGNER_FILE_INFO {
            cbSize: size_of::<SIGNER_FILE_INFO>() as u32,
            pwszFileName: PCWSTR(file.as_ptr()),
            hFile: HANDLE::default(),
        };
        let mut index = 0u32;
        let subject = SIGNER_SUBJECT_INFO {
            cbSize: size_of::<SIGNER_SUBJECT_INFO>() as u32,
            pdwIndex: &raw mut index,
            dwSubjectChoice: SIGNER_SUBJECT_FILE,
            Anonymous: SIGNER_SUBJECT_INFO_0 {
                pSignerFileInfo: &raw mut file_info,
            },
        };
        let signature_info = SIGNER_SIGNATURE_INFO {
            cbSize: size_of::<SIGNER_SIGNATURE_INFO>() as u32,
            algidHash: CALG_SHA_256,
            dwAttrChoice: SIGNER_NO_ATTR,
            ..Default::default()
        };
        let hash_oid = cstr(SHA256);
        let mut signed: *mut SIGNER_CONTEXT = null_mut();
        let mut signing = |url: Option<&[u16]>| {
            SignerSignEx3(
                SIGNER_SIGN_FLAGS(0),
                &subject,
                &certificate,
                &signature_info,
                None,
                url.map(|_| SIGNER_TIMESTAMP_RFC3161),
                if url.is_some() {
                    PCSTR(hash_oid.as_ptr())
                } else {
                    PCSTR::null()
                },
                url.map_or(PCWSTR::null(), |url| PCWSTR(url.as_ptr())),
                None,
                None,
                &raw mut signed,
                None,
                None,
                None,
            )
        };
        let result = match authority {
            None => signing(None),
            Some(authority) => {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                let url = wide(std::ffi::OsStr::new(&format!(
                    "http://{}/",
                    listener.local_addr().unwrap()
                )));
                std::thread::scope(|scope| {
                    let served = scope.spawn(|| authority.serve(&listener, 1));
                    let result = signing(Some(&url));
                    served.join().unwrap();
                    result
                })
            }
        };
        result.unwrap_or_else(|error| panic!("SignerSignEx3: {error}"));
        if !signed.is_null() {
            SignerFreeSignerContext(signed).unwrap();
        }
        let _ = CertFreeCertificateContext(Some(context));
    }
}

// ── the world ───────────────────────────────────────────────────────────────

pub const SUBJECT: &str = super::SUBJECT;
pub const IDENTITY: &str = super::IDENTITY;
pub const OTHER_IDENTITY: &str = super::OTHER_IDENTITY;
/// The version every test file carries unless it says otherwise.
pub const VERSION: FileVersion = FileVersion([0, 4, 6, 0]);

/// A root, and a time-stamp authority under it.
pub struct World {
    pub root: Made,
    pub authority: Made,
    serial: std::cell::Cell<u64>,
}

impl World {
    pub fn new() -> Self {
        let root = issue(
            &Spec {
                subject: "CN=Folio Test Root, O=Folio Test",
                serial: 1,
                not_before: now() - 3650 * DAY,
                not_after: now() + 3650 * DAY,
                ca: true,
                usages: &[],
                critical_usages: false,
            },
            None,
        );
        let authority = issue(
            &Spec {
                subject: "CN=Folio Test Time Stamping, O=Folio Test",
                serial: 2,
                not_before: now() - 3650 * DAY,
                not_after: now() + 3650 * DAY,
                ca: false,
                usages: &[TIME_STAMPING],
                critical_usages: true,
            },
            Some(&root),
        );
        Self {
            root,
            authority,
            serial: std::cell::Cell::new(100),
        }
    }

    /// A code-signing leaf for `subject`, carrying the Public Trust marker and
    /// `identity`, valid from `not_before` to `not_after`.
    pub fn leaf_between(
        &self,
        subject: &str,
        identity: &str,
        not_before: u64,
        not_after: u64,
    ) -> Made {
        let serial = self.serial.get() + 1;
        self.serial.set(serial);
        issue(
            &Spec {
                subject,
                serial,
                not_before,
                not_after,
                ca: false,
                usages: &[PUBLIC_TRUST_MARKER, CODE_SIGNING, identity],
                critical_usages: false,
            },
            Some(&self.root),
        )
    }

    /// A leaf valid for three days around now, like Artifact Signing's.
    pub fn leaf(&self, subject: &str, identity: &str) -> Made {
        self.leaf_between(subject, identity, now() - DAY, now() + 2 * DAY)
    }

    /// The exclusive-root policy of this world, publishing `revoked` as its
    /// revocation list.
    pub fn policy_revoking(&self, revoked: &[u64]) -> Policy {
        Policy::ExclusiveRoot {
            root: self.root.der.clone(),
            revocation_lists: vec![revocation_list(&self.root, revoked)],
        }
    }

    /// The exclusive-root policy, with a revocation list revoking nothing.
    pub fn policy(&self) -> Policy {
        self.policy_revoking(&[])
    }

    /// The last serial this world issued.
    pub fn last_serial(&self) -> u64 {
        self.serial.get()
    }

    /// A time-stamp authority stamping `at`.
    pub fn stamping(&self, at: u64) -> Authority<'_> {
        Authority {
            certificate: &self.authority,
            at,
        }
    }

    /// **A test executable signed by `leaf`**, time-stamped now.
    pub fn signed(
        &self,
        folder: &Folder,
        name: &str,
        leaf: &Made,
        version: FileVersion,
    ) -> PathBuf {
        let path = executable(folder, name, version);
        sign(&path, leaf, &[&self.root], Some(&self.stamping(now())));
        path
    }
}

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
    for made in [&world.root, &world.authority] {
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
        DOWNLOAD_BUDGET, DOWNLOAD_IDLE_TIMEOUT, DownloadMonitor, HttpsDownload,
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
        budget: DOWNLOAD_BUDGET,
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
