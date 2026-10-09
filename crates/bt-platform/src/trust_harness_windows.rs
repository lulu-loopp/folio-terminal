//! The test world of [`crate::trust`] on Windows (E-13, U-15; shared with
//! `bt-app`'s Windows Prepare tests by U-20 through [`super`]): a root, leaves
//! and a time-stamp authority made in the test with ephemeral keys, a small PE
//! file written in the test's folder and signed there by `SignerSignEx3`,
//! time-stamped over HTTP by a time-stamp authority served on the loopback
//! interface, and verified under an exclusive-root engine. Every key is
//! ephemeral (`NCryptCreatePersistedKey` with no name), every store is a memory
//! store made by `trust`'s own `Store`, and nothing is installed.

use crate::trust_harness::Behaviour;
use std::ffi::c_void;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use windows::Win32::Foundation::{FILETIME, HANDLE};
use windows::Win32::Security::Cryptography::{
    BCRYPT_RSA_ALGORITHM, CALG_SHA_256, CERT_BASIC_CONSTRAINTS2_INFO, CERT_CONTEXT, CERT_EXTENSION,
    CERT_INFO, CERT_KEY_CONTEXT, CERT_KEY_CONTEXT_0, CERT_KEY_CONTEXT_PROP_ID,
    CERT_NAME_STR_REVERSE_FLAG, CERT_NCRYPT_KEY_SPEC, CERT_PUBLIC_KEY_INFO,
    CERT_SET_PROPERTY_INHIBIT_PERSIST_FLAG, CERT_SHA1_HASH_PROP_ID,
    CERT_STORE_NO_CRYPT_RELEASE_FLAG, CERT_STRING_TYPE, CERT_V3, CERT_X500_NAME_STR,
    CMSG_CMS_ENCAPSULATED_CONTENT_FLAG, CMSG_CONTENT_PARAM, CMSG_SIGNED, CMSG_SIGNED_ENCODE_INFO,
    CMSG_SIGNER_ENCODE_INFO, CMSG_SIGNER_ENCODE_INFO_0, CRL_ENTRY, CRL_INFO, CRL_V2,
    CRYPT_ALGORITHM_IDENTIFIER, CRYPT_ATTRIBUTE, CRYPT_BIT_BLOB, CRYPT_INTEGER_BLOB, CTL_USAGE,
    CertCreateCertificateContext, CertFreeCertificateContext, CertGetCertificateContextProperty,
    CertSetCertificateContextProperty, CertStrToNameW, CryptEncodeObject, CryptExportPublicKeyInfo,
    CryptMsgClose, CryptMsgGetParam, CryptMsgOpenToEncode, CryptMsgUpdate,
    CryptSignAndEncodeCertificate, HCRYPTPROV_OR_NCRYPT_KEY_HANDLE, MS_KEY_STORAGE_PROVIDER,
    NCRYPT_FLAGS, NCRYPT_HANDLE, NCRYPT_KEY_HANDLE, NCRYPT_LENGTH_PROPERTY, NCRYPT_PROV_HANDLE,
    NCryptCreatePersistedKey, NCryptFinalizeKey, NCryptFreeObject, NCryptOpenStorageProvider,
    NCryptSetProperty, PKCS_7_ASN_ENCODING, SIGNER_CERT, SIGNER_CERT_0, SIGNER_CERT_POLICY_STORE,
    SIGNER_CERT_STORE, SIGNER_CERT_STORE_INFO, SIGNER_CONTEXT, SIGNER_FILE_INFO, SIGNER_NO_ATTR,
    SIGNER_SIGN_FLAGS, SIGNER_SIGNATURE_INFO, SIGNER_SUBJECT_FILE, SIGNER_SUBJECT_INFO,
    SIGNER_SUBJECT_INFO_0, SIGNER_TIMESTAMP_RFC3161, SignerFreeSignerContext, SignerSignEx3,
    X509_ASN_ENCODING, X509_BASIC_CONSTRAINTS2, X509_CERT_CRL_TO_BE_SIGNED, X509_CERT_TO_BE_SIGNED,
    X509_ENHANCED_KEY_USAGE,
};
use windows::Win32::System::LibraryLoader::{
    BeginUpdateResourceW, EndUpdateResourceW, UpdateResourceW,
};
use windows::core::{PCSTR, PCWSTR, PSTR};

use crate::trust::{CODE_SIGNING, FileVersion, PUBLIC_TRUST_MARKER, Policy, TIME_STAMPING};

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

pub fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    text.encode_wide().chain([0]).collect()
}

// ── keys ────────────────────────────────────────────────────────────────────

/// An ephemeral RSA-2048 key: created with no name, so the key storage
/// provider persists nothing, and freed on drop.
pub struct Key(NCRYPT_KEY_HANDLE);

impl Key {
    #[expect(
        clippy::new_without_default,
        reason = "a key is made, never defaulted: each one is a new key pair"
    )]
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
#[derive(Clone)]
pub struct Authority {
    pub certificate: Arc<Made>,
    pub at: u64,
}

impl Authority {
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

    /// Answer `requests` HTTP requests on `listener`, on a worker of the
    /// thread door nobody waits for: a signing call that never connects leaves
    /// it blocked in `accept` until the test process ends, and a signing call
    /// that does connect has its answer before it returns.
    fn serve(&self, listener: &TcpListener, requests: usize) {
        for _ in 0..requests {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
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
        let root = bt_testpath::temp_path(&format!("bt-platform-trust-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).unwrap();
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

/// **The smallest x64 program Windows loads**: one page of headers and one
/// section, a PE32+ image a kilobyte long, which `UpdateResourceW` gives
/// resources, `SignerSignEx3` signs and `WinVerifyTrust`,
/// `GetFileVersionInfoW` and `LoadLibraryExW` read like any other. It opens no
/// window. [`Behaviour::Returns`]: `xor eax, eax; ret`, importing nothing — the
/// process exits at once. [`Behaviour::SaysItsCommandLineLength`]: the same,
/// with its command line's length in bytes as the exit code, read from its
/// PEB. [`Behaviour::StaysUp`]: `Sleep(INFINITE)` in a loop,
/// importing that one function from `KERNEL32.dll` — the process waits in the
/// kernel, using no processor, until it is ended. (A loop that spins instead
/// keeps a scanner's emulator busy until its own time limit whenever the file
/// is written, which costs seconds per program.)
fn small_program(behaviour: Behaviour) -> Vec<u8> {
    const HEADERS: u32 = 0x200;
    const TEXT_RVA: u32 = 0x1000;
    // `StaysUp`'s section: the code at 0x00, the import descriptors at 0x40
    // (one and the terminator), the lookup table at 0x68, the address table
    // at 0x78, `Sleep`'s hint and name at 0x88, the library's name at 0x90.
    const IMPORTS: u32 = 0x40;
    const LOOKUP: u32 = 0x68;
    const ADDRESSES: u32 = 0x78;
    const HINT_NAME: u32 = 0x88;
    const LIBRARY: u32 = 0x90;
    let section: Vec<u8> = match behaviour {
        Behaviour::Returns => vec![0x31, 0xC0, 0xC3],
        // mov rax, gs:[0x60] (the PEB); mov rax, [rax + 0x20] (its process
        // parameters); movzx eax, word [rax + 0x70] (their command line's
        // `Length`); ret.
        Behaviour::SaysItsCommandLineLength => vec![
            0x65, 0x48, 0x8B, 0x04, 0x25, 0x60, 0x00, 0x00, 0x00, 0x48, 0x8B, 0x40, 0x20, 0x0F,
            0xB7, 0x40, 0x70, 0xC3,
        ],
        Behaviour::StaysUp => {
            let mut section = vec![0u8; 0xA0];
            // sub rsp, 0x28; mov ecx, INFINITE; call [rip + (ADDRESSES - 0x0F)];
            // jmp back to the `mov`.
            let call = (ADDRESSES - 0x0F).to_le_bytes();
            section[..0x11].copy_from_slice(&[
                0x48, 0x83, 0xEC, 0x28, 0xB9, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x15, call[0], call[1],
                call[2], call[3], 0xEB, 0xF3,
            ]);
            let at = |offset: u32| offset as usize;
            let rva = |offset: u32| (TEXT_RVA + offset).to_le_bytes();
            // The descriptor: lookup table, no time stamp, no forwarder, the
            // library's name, the address table.
            section[at(IMPORTS)..at(IMPORTS) + 4].copy_from_slice(&rva(LOOKUP));
            section[at(IMPORTS) + 12..at(IMPORTS) + 16].copy_from_slice(&rva(LIBRARY));
            section[at(IMPORTS) + 16..at(IMPORTS) + 20].copy_from_slice(&rva(ADDRESSES));
            for table in [LOOKUP, ADDRESSES] {
                section[at(table)..at(table) + 4].copy_from_slice(&rva(HINT_NAME));
            }
            section[at(HINT_NAME) + 2..at(HINT_NAME) + 7].copy_from_slice(b"Sleep");
            section[at(LIBRARY)..at(LIBRARY) + 12].copy_from_slice(b"KERNEL32.dll");
            section
        }
    };
    let section_size = u32::try_from(section.len()).expect("a page at most");
    let mut image = vec![0u8; 0x400];
    let mut at = 0usize;
    let put = |image: &mut Vec<u8>, at: &mut usize, bytes: &[u8]| {
        image[*at..*at + bytes.len()].copy_from_slice(bytes);
        *at += bytes.len();
    };
    // The DOS header: its signature, and where the PE header is.
    put(&mut image, &mut at, b"MZ");
    image[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
    at = 0x40;
    put(&mut image, &mut at, b"PE\0\0");
    // The file header: x64, one section, a PE32+ optional header, an
    // executable image that may use large addresses.
    for field in [0x8664u16, 1] {
        put(&mut image, &mut at, &field.to_le_bytes());
    }
    put(&mut image, &mut at, &[0; 12]);
    for field in [240u16, 0x0022] {
        put(&mut image, &mut at, &field.to_le_bytes());
    }
    // The optional header, PE32+.
    put(&mut image, &mut at, &0x020Bu16.to_le_bytes());
    put(&mut image, &mut at, &[14, 0]);
    for field in [HEADERS, 0, 0, TEXT_RVA, TEXT_RVA] {
        put(&mut image, &mut at, &field.to_le_bytes());
    }
    put(&mut image, &mut at, &0x1_4000_0000u64.to_le_bytes());
    for field in [0x1000u32, HEADERS] {
        put(&mut image, &mut at, &field.to_le_bytes());
    }
    for field in [6u16, 0, 0, 0, 6, 0] {
        put(&mut image, &mut at, &field.to_le_bytes());
    }
    for field in [0u32, 0x2000, HEADERS, 0] {
        put(&mut image, &mut at, &field.to_le_bytes());
    }
    // Console subsystem; no-execute data, terminal-server aware.
    for field in [3u16, 0x8100] {
        put(&mut image, &mut at, &field.to_le_bytes());
    }
    for field in [0x10_0000u64, 0x1000, 0x10_0000, 0x1000] {
        put(&mut image, &mut at, &field.to_le_bytes());
    }
    for field in [0u32, 16] {
        put(&mut image, &mut at, &field.to_le_bytes());
    }
    // The sixteen data directories: the import table (1) and the import
    // address table (12) for `StaysUp`, nothing else.
    let directories = at;
    put(&mut image, &mut at, &[0; 16 * 8]);
    if behaviour == Behaviour::StaysUp {
        for (index, rva, size) in [(1, IMPORTS, 40u32), (12, ADDRESSES, 16)] {
            let entry = directories + index * 8;
            image[entry..entry + 4].copy_from_slice(&(TEXT_RVA + rva).to_le_bytes());
            image[entry + 4..entry + 8].copy_from_slice(&size.to_le_bytes());
        }
    }
    // The one section: code, readable and executable — and writable for
    // `StaysUp`, whose address table the loader fills.
    put(&mut image, &mut at, b".text\0\0\0");
    for field in [section_size, TEXT_RVA, HEADERS, HEADERS, 0, 0] {
        put(&mut image, &mut at, &field.to_le_bytes());
    }
    put(&mut image, &mut at, &[0; 4]);
    let characteristics: u32 = match behaviour {
        Behaviour::Returns | Behaviour::SaysItsCommandLineLength => 0x6000_0020,
        Behaviour::StaysUp => 0xE000_0060,
    };
    put(&mut image, &mut at, &characteristics.to_le_bytes());
    assert!(at <= HEADERS as usize, "the headers fit their page");
    image[HEADERS as usize..HEADERS as usize + section.len()].copy_from_slice(&section);
    image
}

/// **A PE file at `path`**, which must not exist: [`small_program`] that
/// returns at once, carrying `version` as its `VERSIONINFO` and each of
/// `resources` as an `RCDATA` resource of that name. Unsigned.
pub fn program_at(path: &Path, version: FileVersion, resources: &[(&str, &[u8])]) {
    program_doing(path, version, resources, Behaviour::Returns);
}

/// [`program_at`], whose code is `behaviour`'s.
pub fn program_doing(
    path: &Path,
    version: FileVersion,
    resources: &[(&str, &[u8])],
    behaviour: Behaviour,
) {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    file.write_all(&small_program(behaviour)).unwrap();
    drop(file);
    resources_written(
        path,
        version,
        resources,
        &mut std::thread::sleep,
        &mut || {},
    );
}

/// **How many times a resource update of a file another program holds is
/// made**: with the pauses between them (the first [`RESOURCE_FIRST_PAUSE`],
/// each next one twice the last, at most [`RESOURCE_LONGEST_PAUSE`]) about
/// 10 s in all, which outlasts an on-access scan of a file just written (an
/// antivirus, an indexer) the way the applier's own journal write does.
const RESOURCE_ATTEMPTS: usize = 10;
/// The pause after the first refused attempt.
const RESOURCE_FIRST_PAUSE: std::time::Duration = std::time::Duration::from_millis(25);
/// The longest pause between two attempts.
const RESOURCE_LONGEST_PAUSE: std::time::Duration = std::time::Duration::from_secs(4);

/// The step of a resource update that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UpdateStep {
    /// `BeginUpdateResourceW`, which reads the file.
    Begin,
    /// An `UpdateResourceW`, which only stages in memory.
    Stage,
    /// `EndUpdateResourceW`, which writes the file.
    End,
}

/// A refused or failed resource update: the step and its error.
#[derive(Debug)]
struct UpdateFailure {
    step: UpdateStep,
    error: windows::core::Error,
}

impl UpdateFailure {
    /// **Whether another program holding the file is what refused it.**
    ///
    /// The begin reads the file and the end writes it; each opens it, and an
    /// open another program's handle refuses comes back as one of three
    /// codes: `ERROR_SHARING_VIOLATION` or `ERROR_ACCESS_DENIED` passed on, or
    /// `ERROR_OPEN_FAILED`, the update's own word for any open it was refused
    /// (what a hold taken by a test reads as). The file is the one this
    /// harness has just written and still names, so none of the three is
    /// anything but a hold. Staging touches no file, and nothing else is a
    /// hold.
    fn held(&self) -> bool {
        use windows::Win32::Foundation::{
            ERROR_ACCESS_DENIED, ERROR_OPEN_FAILED, ERROR_SHARING_VIOLATION,
        };
        let code = self.error.code();
        match self.step {
            UpdateStep::Begin | UpdateStep::End => [
                ERROR_SHARING_VIOLATION,
                ERROR_ACCESS_DENIED,
                ERROR_OPEN_FAILED,
            ]
            .iter()
            .any(|held| code == held.to_hresult()),
            UpdateStep::Stage => false,
        }
    }

    /// `end 0x80070020`: the step and the code, as a refusal is listed.
    fn named(&self) -> String {
        let step = match self.step {
            UpdateStep::Begin => "begin",
            UpdateStep::Stage => "stage",
            UpdateStep::End => "end",
        };
        format!("{step} {:#010x}", self.error.code().0)
    }
}

/// **`version` and `resources` written into the program at `path`**, the
/// whole update made again while another program holds the file.
///
/// A file this harness has just written is what an on-access scanner opens
/// next, and while it holds the file the update cannot open it
/// ([`UpdateFailure::held`]). Each attempt is the whole update — begin,
/// every resource, end — against the file as written, which a refused begin
/// or end left untouched. `pause` is called between attempts (the harness
/// sleeps; a test hands in what lets its own hold go), and `before_end`
/// just before each end (the harness does nothing there; a test takes its
/// hold there, as a scanner that opens the file mid-update does). Any other
/// failure, or a hold that outlasts [`RESOURCE_ATTEMPTS`], panics naming
/// every refusal.
fn resources_written(
    path: &Path,
    version: FileVersion,
    resources: &[(&str, &[u8])],
    pause: &mut impl FnMut(std::time::Duration),
    before_end: &mut impl FnMut(),
) {
    let block = version_block(version);
    let file = wide(path.as_os_str());
    let names: Vec<Vec<u16>> = resources
        .iter()
        .map(|(name, _)| wide(std::ffi::OsStr::new(name)))
        .collect();
    let mut refused = Vec::new();
    let mut wait = RESOURCE_FIRST_PAUSE;
    loop {
        match update_resources(&file, &block, resources, &names, before_end) {
            Ok(()) => return,
            Err(failure) if failure.held() && refused.len() + 1 < RESOURCE_ATTEMPTS => {
                refused.push(failure.named());
                pause(wait);
                wait = (wait * 2).min(RESOURCE_LONGEST_PAUSE);
            }
            Err(failure) => panic!(
                "{}: the resource update failed at attempt {} of {RESOURCE_ATTEMPTS} at {} ({}); \
                 refused {} times before it while the file was held: [{}]",
                path.display(),
                refused.len() + 1,
                failure.named(),
                failure.error,
                refused.len(),
                refused.join(", "),
            ),
        }
    }
}

/// One resource update of `file`: the version block as `VERSIONINFO` 1, each
/// of `resources` as `RCDATA` under its name, written by the end. A failure
/// while staging discards the update.
fn update_resources(
    file: &[u16],
    block: &[u8],
    resources: &[(&str, &[u8])],
    names: &[Vec<u16>],
    before_end: &mut impl FnMut(),
) -> Result<(), UpdateFailure> {
    let at = |step| move |error| UpdateFailure { step, error };
    // SAFETY: the path and the names are NUL-terminated; the blocks live
    // across the update, and the handle is ended on every path.
    unsafe {
        let update =
            BeginUpdateResourceW(PCWSTR(file.as_ptr()), false).map_err(at(UpdateStep::Begin))?;
        let staged = UpdateResourceW(
            update,
            PCWSTR(16 as _),
            PCWSTR(1 as _),
            0x0409,
            Some(block.as_ptr().cast()),
            block.len() as u32,
        )
        .and_then(|()| {
            resources
                .iter()
                .zip(names)
                .try_for_each(|((_, bytes), name)| {
                    UpdateResourceW(
                        update,
                        PCWSTR(10 as _),
                        PCWSTR(name.as_ptr()),
                        0x0409,
                        Some(bytes.as_ptr().cast()),
                        bytes.len() as u32,
                    )
                })
        });
        if let Err(error) = staged {
            let _ = EndUpdateResourceW(update, true);
            return Err(at(UpdateStep::Stage)(error));
        }
        before_end();
        EndUpdateResourceW(update, false).map_err(at(UpdateStep::End))
    }
}

/// **A PE file in `folder`** ([`program_at`] with no resource), carrying
/// `version` as its `VERSIONINFO`.
pub fn executable(folder: &Folder, name: &str, version: FileVersion) -> PathBuf {
    let path = folder.0.join(name);
    program_at(&path, version, &[]);
    path
}

/// **Sign `path` with `leaf`**, putting `chain` in the signature, time-stamped
/// by `authority` when there is one (RFC 3161, SHA-256 imprint: `signtool
/// sign /fd SHA256 /tr <url> /td SHA256`, the form `sign.ps1` produces).
pub fn sign(path: &Path, leaf: &Made, chain: &[&Made], authority: Option<&Authority>) {
    assert!(
        path.starts_with(std::env::temp_dir()),
        "signs only inside the test's own folder"
    );
    let chain: Vec<Vec<u8>> = chain.iter().map(|made| made.der.clone()).collect();
    let store = crate::trust::arm::Store::of_certificates(&chain).unwrap();
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
                let authority = authority.clone();
                crate::spawn_at_priority(
                    "bt-trust-harness-tsa",
                    crate::ThreadPriority::BelowNormal,
                    move |_| authority.serve(&listener, 1),
                )
                .unwrap();
                signing(Some(&url))
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

pub use super::{IDENTITY, OTHER_IDENTITY, SUBJECT};
/// The version every test file carries unless it says otherwise.
pub const VERSION: FileVersion = FileVersion([0, 4, 6, 0]);

/// A root, and a time-stamp authority under it.
pub struct World {
    pub root: Made,
    pub authority: Arc<Made>,
    serial: std::cell::Cell<u64>,
}

impl World {
    #[expect(
        clippy::new_without_default,
        reason = "a world is made, never defaulted: each one is a new root"
    )]
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
            authority: Arc::new(authority),
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
    pub fn stamping(&self, at: u64) -> Authority {
        Authority {
            certificate: Arc::clone(&self.authority),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt as _;

    /// A fresh program in `folder`, before any resource is written into it.
    fn bare_program(folder: &Folder, name: &str) -> PathBuf {
        let path = folder.0.join(name);
        std::fs::write(&path, small_program(Behaviour::Returns)).unwrap();
        path
    }

    /// `FILE_SHARE_READ | FILE_SHARE_DELETE`: a hold the way an on-access
    /// scanner takes one, letting others read the file and nobody write it.
    const SCANNER: u32 = 0x5;
    /// No sharing at all: a hold that refuses even the update's read.
    const ALONE: u32 = 0x0;

    /// **A hold on `path`**: the file open for reading, with `share`.
    fn hold(path: &Path, share: u32) -> std::fs::File {
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(share)
            .open(path)
            .unwrap()
    }

    /// What the program at `path` carries: its file version and its `NOTE`.
    fn carried(path: &Path) -> (Option<FileVersion>, Vec<u8>) {
        (
            crate::trust::file_version(path).ok(),
            crate::pe_resource::read_rcdata(path, "NOTE", 64).unwrap(),
        )
    }

    /// RED — **an update whose end is refused because another program opened
    /// the file mid-update is made again once the hold is let go**, and the
    /// program then carries what was written. The `EndUpdateResourceW` reds
    /// of 2026-10-09 were this: a scanner opening the file between the begin
    /// and the end.
    ///
    /// The hold is this test's own — taken just before the first end, let go
    /// by the first pause — so no clock decides anything.
    ///
    /// MUTATION: in `UpdateFailure::held`, answer `false` for
    /// `UpdateStep::End` — the first attempt panics at its end.
    #[test]
    fn an_end_refused_by_a_hold_taken_mid_update_is_made_again() {
        let folder = Folder::new("资源 end");
        let path = bare_program(&folder, "程序 end.exe");
        let held = std::cell::RefCell::new(None);
        let mut pauses = Vec::new();
        let note = "公式 note, ½".as_bytes();
        let mut ends = 0;
        resources_written(
            &path,
            FileVersion([4, 8, 15, 16]),
            &[("NOTE", note)],
            &mut |pause| {
                pauses.push(pause);
                held.replace(None);
            },
            &mut || {
                ends += 1;
                if ends == 1 {
                    held.replace(Some(hold(&path, SCANNER)));
                }
            },
        );
        assert_eq!(
            (ends, pauses),
            (2, vec![RESOURCE_FIRST_PAUSE]),
            "the first end was refused, the update waited once, and the second end wrote"
        );
        assert_eq!(
            carried(&path),
            (Some(FileVersion([4, 8, 15, 16])), note.to_vec())
        );
    }

    /// RED — **an update whose begin is refused because another program holds
    /// the file is made again once the hold is let go.** A hold that shares
    /// nothing refuses even the begin's read.
    ///
    /// MUTATION: in `UpdateFailure::held`, answer `false` for
    /// `UpdateStep::Begin` — the first attempt panics at its begin.
    #[test]
    fn a_begin_refused_by_a_hold_is_made_again() {
        let folder = Folder::new("资源 begin");
        let path = bare_program(&folder, "程序 begin.exe");
        let mut held = Some(hold(&path, ALONE));
        let mut pauses = Vec::new();
        let note = "Ω 字".as_bytes();
        resources_written(
            &path,
            FileVersion([1, 2, 3, 4]),
            &[("NOTE", note)],
            &mut |pause| {
                pauses.push(pause);
                held = None;
            },
            &mut || {},
        );
        assert!(held.is_none(), "the hold was let go by the pause");
        assert_eq!(pauses, [RESOURCE_FIRST_PAUSE]);
        assert_eq!(
            carried(&path),
            (Some(FileVersion([1, 2, 3, 4])), note.to_vec())
        );
    }

    /// RED — **a hold that outlasts every attempt is a panic that names each
    /// refusal**, after exactly [`RESOURCE_ATTEMPTS`] attempts with the pauses
    /// doubling up to [`RESOURCE_LONGEST_PAUSE`].
    ///
    /// MUTATION: list no refusal in the panic message — red at the message;
    /// or drop the `RESOURCE_ATTEMPTS` bound — red at the tenth pause.
    #[test]
    fn a_hold_that_outlasts_every_attempt_names_each_refusal() {
        let folder = Folder::new("资源 kept");
        let path = bare_program(&folder, "程序 kept.exe");
        let held = hold(&path, SCANNER);
        let mut pauses = Vec::new();
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            resources_written(
                &path,
                FileVersion([1, 0, 0, 0]),
                &[],
                &mut |pause| {
                    assert!(pauses.len() < RESOURCE_ATTEMPTS, "unbounded: {pauses:?}");
                    pauses.push(pause);
                },
                &mut || {},
            );
        }))
        .expect_err("the hold never ends, so the update cannot be made");
        drop(held);
        let said = failed
            .downcast_ref::<String>()
            .expect("a formatted panic")
            .clone();
        let ms = |ms| std::time::Duration::from_millis(ms);
        assert_eq!(
            pauses,
            [25, 50, 100, 200, 400, 800, 1600, 3200, 4000].map(ms),
            "{said}"
        );
        let before = ["end 0x8007006e"; RESOURCE_ATTEMPTS - 1].join(", ");
        assert!(
            said.contains(&format!(
                "attempt {RESOURCE_ATTEMPTS} of {RESOURCE_ATTEMPTS} at end 0x8007006e"
            )) && said.contains(&format!(
                "refused {} times before it while the file was held: [{before}]",
                RESOURCE_ATTEMPTS - 1
            )),
            "every refusal is named, the last as the failure: {said}"
        );
    }
}
