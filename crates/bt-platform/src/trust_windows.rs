//! The Windows arm of [`crate::trust`]: `WinVerifyTrust`, its provider data,
//! the time stamp, the chain engine and the name, all in process. See the
//! parent module for the order of the steps and why each is there.

use std::ffi::c_void;
use std::io::Read as _;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::null_mut;

use windows::Win32::Foundation::{FILETIME, HANDLE, HWND, INVALID_HANDLE_VALUE};
use windows::Win32::Security::Cryptography::{
    CERT_CHAIN_CACHE_ONLY_URL_RETRIEVAL, CERT_CHAIN_CONTEXT, CERT_CHAIN_ENGINE_CONFIG,
    CERT_CHAIN_PARA, CERT_CHAIN_POLICY_AUTHENTICODE, CERT_CHAIN_POLICY_BASE,
    CERT_CHAIN_POLICY_FLAGS, CERT_CHAIN_POLICY_IGNORE_ALL_REV_UNKNOWN_FLAGS,
    CERT_CHAIN_POLICY_PARA, CERT_CHAIN_POLICY_STATUS,
    CERT_CHAIN_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT, CERT_CONTEXT,
    CERT_FIND_EXT_ONLY_ENHKEY_USAGE_FLAG, CERT_INFO, CERT_NAME_STR_REVERSE_FLAG,
    CERT_OPEN_STORE_FLAGS, CERT_QUERY_ENCODING_TYPE, CERT_STORE_ADD_ALWAYS, CERT_STORE_PROV_MEMORY,
    CERT_STRING_TYPE, CERT_USAGE_MATCH, CERT_X500_NAME_STR, CMSG_CERT_COUNT_PARAM, CMSG_CERT_PARAM,
    CMSG_SIGNER_INFO, CRYPT_TIMESTAMP_CONTEXT, CTL_USAGE, CertAddEncodedCRLToStore,
    CertAddEncodedCertificateToStore, CertCloseStore, CertCreateCertificateChainEngine,
    CertCreateCertificateContext, CertFreeCertificateChain, CertFreeCertificateChainEngine,
    CertFreeCertificateContext, CertGetCertificateChain, CertGetEnhancedKeyUsage,
    CertGetSubjectCertificateFromStore, CertNameToStrW, CertOpenStore,
    CertVerifyCertificateChainPolicy, CryptMemFree, CryptMsgGetParam,
    CryptVerifyTimeStampSignature, HCERTCHAINENGINE, HCERTSTORE, PKCS_7_ASN_ENCODING,
    USAGE_MATCH_TYPE_AND, X509_ASN_ENCODING,
};
use windows::Win32::Security::WinTrust::{
    CRYPT_PROVIDER_DATA, TRUSTERROR_STEP_FINAL_POLICYPROV, TRUSTERROR_STEP_FINAL_WVTINIT,
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
    WTD_CHOICE_FILE, WTD_DISABLE_MD2_MD4, WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT,
    WTD_REVOKE_WHOLECHAIN, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
    WTHelperGetProvSignerFromChain, WTHelperProvDataFromStateData, WinVerifyTrust,
};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
};
use windows::Win32::Storage::Packaging::Appx::{AppxFactory, IAppxFactory};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize, STGM_READ, STGM_SHARE_DENY_WRITE,
};
use windows::Win32::UI::Shell::SHCreateStreamOnFileEx;
use windows::core::{PCSTR, PCWSTR, PSTR};

use super::{
    CODE_SIGNING, Expectation, FileVersion, Machine, Policy, RFC3161_COUNTERSIGNATURE, Refusal,
    Signed, TIME_STAMPING, Verified, compare, identity_oid, publisher_verdict, revocation_verdict,
    timestamp_within, trust_verdict,
};

/// Both encodings a certificate, a message or a store takes here.
const ENCODING: CERT_QUERY_ENCODING_TYPE =
    CERT_QUERY_ENCODING_TYPE(X509_ASN_ENCODING.0 | PKCS_7_ASN_ENCODING.0);

/// How many bytes of a file's head are read to find its machine type. The PE
/// header sits where `e_lfanew` says, which a linker puts in the first page.
const HEADER_BYTES: u64 = 4096;

fn wide(path: &Path) -> Result<Vec<u16>, Refusal> {
    let mut value: Vec<u16> = path.as_os_str().encode_wide().collect();
    if value.contains(&0) {
        return Err(Refusal::Unreadable(format!("{} has a NUL", path.display())));
    }
    value.push(0);
    Ok(value)
}

const fn ticks(time: FILETIME) -> u64 {
    ((time.dwHighDateTime as u64) << 32) | time.dwLowDateTime as u64
}

const fn filetime(ticks: u64) -> FILETIME {
    FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    }
}

fn code_of(error: &windows::core::Error) -> i32 {
    error.code().0
}

// ── owned handles ───────────────────────────────────────────────────────────

/// A certificate store this module opened; closed on drop.
pub(crate) struct Store(pub(crate) HCERTSTORE);

impl Store {
    /// **A new memory store** — the only kind this module ever creates. It
    /// lives in this process and is gone when it is closed.
    pub(super) fn memory() -> Result<Self, Refusal> {
        // SAFETY: a memory store takes no parameter and no provider.
        unsafe {
            CertOpenStore(
                CERT_STORE_PROV_MEMORY,
                CERT_QUERY_ENCODING_TYPE(0),
                None,
                CERT_OPEN_STORE_FLAGS(0),
                None,
            )
        }
        .map(Self)
        .map_err(|error| Refusal::Unreadable(format!("a memory store: {error}")))
    }

    /// A memory store holding `certificates` (DER).
    pub(crate) fn of_certificates(certificates: &[Vec<u8>]) -> Result<Self, Refusal> {
        let store = Self::memory()?;
        for der in certificates {
            // SAFETY: the store is the memory store just opened; the bytes are
            // copied into it.
            unsafe {
                CertAddEncodedCertificateToStore(
                    Some(store.0),
                    X509_ASN_ENCODING,
                    der,
                    CERT_STORE_ADD_ALWAYS,
                    None,
                )
            }
            .map_err(|error| Refusal::Unreadable(format!("a certificate: {error}")))?;
        }
        Ok(store)
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by this module and is closed once.
        unsafe {
            let _ = CertCloseStore(Some(self.0), 0);
        }
    }
}

/// A certificate context this module made; freed on drop.
struct Cert(*const CERT_CONTEXT);

impl Cert {
    fn of_der(der: &[u8]) -> Result<Self, Refusal> {
        // SAFETY: the bytes are copied into the new context.
        let context = unsafe { CertCreateCertificateContext(X509_ASN_ENCODING, der) };
        if context.is_null() {
            return Err(Refusal::Unreadable(
                "a certificate that does not decode".into(),
            ));
        }
        Ok(Self(context))
    }

    fn info(&self) -> &CERT_INFO {
        // SAFETY: a live context always points at its decoded info.
        unsafe { &*(*self.0).pCertInfo }
    }
}

impl Drop for Cert {
    fn drop(&mut self) {
        // SAFETY: made by this module and freed once.
        unsafe {
            let _ = CertFreeCertificateContext(Some(self.0));
        }
    }
}

/// A chain context; freed on drop.
struct Chain(*mut CERT_CHAIN_CONTEXT);

impl Chain {
    fn error_status(&self) -> u32 {
        // SAFETY: a live chain context.
        unsafe { (*self.0).TrustStatus.dwErrorStatus }
    }

    /// `CertVerifyCertificateChainPolicy`'s error for `policy`, `0` when it
    /// passes.
    fn policy_error(&self, policy: PCSTR, flags: CERT_CHAIN_POLICY_FLAGS) -> i32 {
        let para = CERT_CHAIN_POLICY_PARA {
            cbSize: size_of::<CERT_CHAIN_POLICY_PARA>() as u32,
            dwFlags: flags,
            pvExtraPolicyPara: null_mut(),
        };
        let mut status = CERT_CHAIN_POLICY_STATUS {
            cbSize: size_of::<CERT_CHAIN_POLICY_STATUS>() as u32,
            ..Default::default()
        };
        // SAFETY: both structures are sized and live; the chain is live.
        let called =
            unsafe { CertVerifyCertificateChainPolicy(policy, self.0, &para, &mut status) };
        if called.as_bool() {
            status.dwError.cast_signed()
        } else {
            windows::core::Error::from_thread().code().0
        }
    }
}

impl Drop for Chain {
    fn drop(&mut self) {
        // SAFETY: built by this module and freed once.
        unsafe { CertFreeCertificateChain(self.0) }
    }
}

/// **The chain engine a policy names.** `None` is Windows' own (the current
/// user's) engine; `Some` is an exclusive-root engine made for a test.
pub(super) struct Engine {
    handle: Option<HCERTCHAINENGINE>,
    /// The stores the engine was configured with, kept open for its life:
    /// the exclusive roots first.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "held for the engine's life; read only by the test that counts the exclusive roots"
        )
    )]
    stores: Vec<Store>,
}

impl Engine {
    pub(super) fn of(policy: &Policy) -> Result<Self, Refusal> {
        match policy {
            Policy::System => Ok(Self {
                handle: None,
                stores: Vec::new(),
            }),
            Policy::ExclusiveRoot {
                root,
                revocation_lists,
            } => {
                let roots = Store::of_certificates(std::slice::from_ref(root))?;
                let lists = Store::memory()?;
                for der in revocation_lists {
                    // SAFETY: the memory store just opened; the bytes are
                    // copied.
                    unsafe {
                        CertAddEncodedCRLToStore(
                            Some(lists.0),
                            X509_ASN_ENCODING,
                            der,
                            CERT_STORE_ADD_ALWAYS,
                            None,
                        )
                    }
                    .map_err(|error| Refusal::Unreadable(format!("a revocation list: {error}")))?;
                }
                let mut additional = [lists.0];
                let config = CERT_CHAIN_ENGINE_CONFIG {
                    cbSize: size_of::<CERT_CHAIN_ENGINE_CONFIG>() as u32,
                    cAdditionalStore: 1,
                    rghAdditionalStore: additional.as_mut_ptr(),
                    // The test world has no network: nothing is fetched, so
                    // revocation is what `revocation_lists` says or unknown.
                    dwFlags: CERT_CHAIN_CACHE_ONLY_URL_RETRIEVAL,
                    hExclusiveRoot: roots.0,
                    ..Default::default()
                };
                let mut handle = HCERTCHAINENGINE::default();
                // SAFETY: the configuration is sized and its stores are live;
                // the engine keeps its own references to them.
                unsafe { CertCreateCertificateChainEngine(&config, &mut handle) }.map_err(
                    |error| Refusal::Untrusted {
                        code: code_of(&error),
                    },
                )?;
                Ok(Self {
                    handle: Some(handle),
                    stores: vec![roots, lists],
                })
            }
        }
    }

    /// **How many certificates the store this engine was given as its
    /// exclusive root holds** — `0` for Windows' own engine, which has none.
    #[cfg(test)]
    pub(super) fn exclusive_roots(&self) -> usize {
        use windows::Win32::Security::Cryptography::CertEnumCertificatesInStore;
        let Some(roots) = self.stores.first() else {
            return 0;
        };
        let mut count = 0;
        // SAFETY: enumerating a live memory store; each context is passed
        // back to the next call, which frees it.
        let mut previous = unsafe { CertEnumCertificatesInStore(roots.0, None) };
        while !previous.is_null() {
            count += 1;
            // SAFETY: as above.
            previous = unsafe { CertEnumCertificatesInStore(roots.0, Some(previous)) };
        }
        count
    }

    /// Whether this engine has a handle of its own (an exclusive-root engine).
    pub(super) const fn is_exclusive(&self) -> bool {
        self.handle.is_some()
    }

    /// The chain of `certificate` at `time`, valid for `usage`.
    fn chain(
        &self,
        certificate: &Cert,
        time: u64,
        additional: &Store,
        usage: &str,
        flags: u32,
    ) -> Result<Chain, i32> {
        let mut oid = format!("{usage}\0").into_bytes();
        let mut identifiers = [PSTR(oid.as_mut_ptr())];
        let para = CERT_CHAIN_PARA {
            cbSize: size_of::<CERT_CHAIN_PARA>() as u32,
            RequestedUsage: CERT_USAGE_MATCH {
                dwType: USAGE_MATCH_TYPE_AND,
                Usage: CTL_USAGE {
                    cUsageIdentifier: 1,
                    rgpszUsageIdentifier: identifiers.as_mut_ptr(),
                },
            },
            ..Default::default()
        };
        let at = filetime(time);
        let mut chain: *mut CERT_CHAIN_CONTEXT = null_mut();
        // SAFETY: every pointer is live across the call; the chain is owned
        // by the returned `Chain`.
        unsafe {
            CertGetCertificateChain(
                self.handle,
                certificate.0,
                Some(&at),
                Some(additional.0),
                &para,
                flags,
                None,
                &mut chain,
            )
        }
        .map_err(|error| code_of(&error))?;
        Ok(Chain(chain))
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(handle) = self.handle {
            // SAFETY: made by this module and freed once.
            unsafe { CertFreeCertificateChainEngine(Some(handle)) }
        }
    }
}

// ── step 1: what `WinVerifyTrust` said, copied out of its state ─────────────

/// The parts of `WinVerifyTrust`'s answer the later steps read.
struct Answer {
    status: i32,
    /// No provider step recorded an error: the file parsed, its digest
    /// matched, its signature verified and its chain was built — whatever
    /// `WinVerifyTrust` refused, it refused as the policy's verdict on that
    /// chain.
    integrity: bool,
    leaf: Vec<u8>,
    message_certificates: Vec<Vec<u8>>,
    /// The signer's chain as `WinVerifyTrust` built it, when it built one.
    chain_status: Option<u32>,
    encrypted_hash: Vec<u8>,
    rfc3161: Option<Vec<u8>>,
}

fn blob(data: *const u8, length: u32) -> Vec<u8> {
    if data.is_null() || length == 0 {
        return Vec::new();
    }
    // SAFETY: CryptoAPI blobs point at `length` readable bytes.
    unsafe { std::slice::from_raw_parts(data, length as usize) }.to_vec()
}

fn message_certificates(message: *const c_void) -> Vec<Vec<u8>> {
    let mut count = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: a live message; the count is a `u32`.
    let counted = unsafe {
        CryptMsgGetParam(
            message,
            CMSG_CERT_COUNT_PARAM,
            0,
            Some((&raw mut count).cast()),
            &mut size,
        )
    };
    if counted.is_err() {
        return Vec::new();
    }
    let mut certificates = Vec::new();
    for index in 0..count {
        let mut length = 0u32;
        // SAFETY: the size query writes only `length`.
        if unsafe { CryptMsgGetParam(message, CMSG_CERT_PARAM, index, None, &mut length) }.is_err()
        {
            continue;
        }
        let mut der = vec![0u8; length as usize];
        // SAFETY: `der` holds `length` bytes.
        if unsafe {
            CryptMsgGetParam(
                message,
                CMSG_CERT_PARAM,
                index,
                Some(der.as_mut_ptr().cast()),
                &mut length,
            )
        }
        .is_ok()
        {
            der.truncate(length as usize);
            certificates.push(der);
        }
    }
    certificates
}

/// # Safety
///
/// `signer` is a live `CMSG_SIGNER_INFO` from provider data.
unsafe fn rfc3161_token(signer: &CMSG_SIGNER_INFO) -> Option<Vec<u8>> {
    let attributes = &signer.UnauthAttrs;
    for index in 0..attributes.cAttr as usize {
        // SAFETY: `rgAttr` has `cAttr` entries.
        let attribute = unsafe { &*attributes.rgAttr.add(index) };
        // SAFETY: an attribute's OID is a NUL-terminated ASCII string.
        let oid = unsafe { attribute.pszObjId.to_string() }.unwrap_or_default();
        if oid == RFC3161_COUNTERSIGNATURE && attribute.cValue > 0 {
            // SAFETY: `rgValue` has `cValue` entries.
            let value = unsafe { &*attribute.rgValue };
            return Some(blob(value.pbData, value.cbData));
        }
    }
    None
}

fn read_answer(status: i32, state: HANDLE) -> Result<Answer, Refusal> {
    let unsigned_or = |status: i32| {
        trust_verdict(status)
            .err()
            .unwrap_or(Refusal::Untrusted { code: status })
    };
    // SAFETY: the state handle `WinVerifyTrust` returned, still open.
    let data = unsafe { WTHelperProvDataFromStateData(state) };
    if data.is_null() {
        return Err(unsigned_or(status));
    }
    // SAFETY: non-null provider data, alive until the state is closed.
    let data: &CRYPT_PROVIDER_DATA = unsafe { &*data };
    let steps = if data.padwTrustStepErrors.is_null() {
        &[][..]
    } else {
        // SAFETY: the array holds `cdwTrustStepErrors` entries.
        unsafe {
            std::slice::from_raw_parts(data.padwTrustStepErrors, data.cdwTrustStepErrors as usize)
        }
    };
    let step = |index: u32| steps.get(index as usize).copied().unwrap_or(u32::MAX);
    let integrity = (TRUSTERROR_STEP_FINAL_WVTINIT..=TRUSTERROR_STEP_FINAL_POLICYPROV)
        .all(|index| step(index) == 0);

    // SAFETY: provider data from a verify call; index 0 is the primary
    // signer and null when there is none.
    let signer =
        unsafe { WTHelperGetProvSignerFromChain(std::ptr::from_ref(data).cast_mut(), 0, false, 0) };
    if signer.is_null() || data.hMsg.is_null() {
        return Err(unsigned_or(status));
    }
    // SAFETY: non-null, owned by the provider data.
    let signer = unsafe { &*signer };
    if signer.psSigner.is_null() {
        return Err(unsigned_or(status));
    }
    // SAFETY: non-null, owned by the provider data.
    let info = unsafe { &*signer.psSigner };
    let message_certificates = message_certificates(data.hMsg);
    let store = Store::of_certificates(&message_certificates)?;
    let id = CERT_INFO {
        Issuer: info.Issuer,
        SerialNumber: info.SerialNumber,
        ..Default::default()
    };
    // SAFETY: the store is live; `id` names the issuer and serial by the
    // signer info's own blobs, alive with the provider data.
    let leaf = unsafe { CertGetSubjectCertificateFromStore(store.0, ENCODING, &id) };
    if leaf.is_null() {
        return Err(unsigned_or(status));
    }
    let leaf = Cert(leaf);
    // SAFETY: a live context.
    let leaf_der = unsafe { blob((*leaf.0).pbCertEncoded, (*leaf.0).cbCertEncoded) };
    let chain_status = (!signer.pChainContext.is_null())
        // SAFETY: a chain owned by the provider data.
        .then(|| unsafe { (*signer.pChainContext).TrustStatus.dwErrorStatus });
    Ok(Answer {
        status,
        integrity,
        leaf: leaf_der,
        message_certificates,
        chain_status,
        encrypted_hash: blob(info.EncryptedHash.pbData, info.EncryptedHash.cbData),
        // SAFETY: `info` is the provider's live signer info.
        rfc3161: unsafe { rfc3161_token(info) },
    })
}

fn win_verify_trust(path: &Path) -> Result<Answer, Refusal> {
    let file = wide(path)?;
    let mut info = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(file.as_ptr()),
        hFile: HANDLE::default(),
        pgKnownSubject: null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_WHOLECHAIN,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 {
            pFile: &raw mut info,
        },
        dwStateAction: WTD_STATEACTION_VERIFY,
        // The system's revocation, the whole chain but the root; MD2 and MD4
        // refused; and no `WTD_LIFETIME_SIGNING_FLAG`: the time stamp keeps a
        // signature valid past its certificate, and that is the point.
        dwProvFlags: WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT | WTD_DISABLE_MD2_MD4,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let no_window = HWND(INVALID_HANDLE_VALUE.0);
    // SAFETY: `data` and `info` are live and sized; the path is NUL-terminated
    // and outlives both calls; the state is closed below.
    let status = unsafe { WinVerifyTrust(no_window, &mut action, (&raw mut data).cast()) };
    let answer = read_answer(status, data.hWVTStateData);
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    // SAFETY: closes the state the call above opened.
    unsafe { WinVerifyTrust(no_window, &mut action, (&raw mut data).cast()) };
    answer
}

// ── steps 1 to 3 ────────────────────────────────────────────────────────────

/// What steps 1 to 3 leave for the rest.
struct Signature {
    signed: Signed,
    leaf: Cert,
}

fn key_usages(leaf: &Cert) -> Vec<String> {
    let flags = CERT_FIND_EXT_ONLY_ENHKEY_USAGE_FLAG.0;
    let mut size = 0u32;
    // SAFETY: the size query.
    if unsafe { CertGetEnhancedKeyUsage(leaf.0, flags, None, &mut size) }.is_err() || size == 0 {
        return Vec::new();
    }
    let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
    let usage = buffer.as_mut_ptr().cast::<CTL_USAGE>();
    // SAFETY: `buffer` is `size` bytes, aligned for the structure.
    if unsafe { CertGetEnhancedKeyUsage(leaf.0, flags, Some(usage), &mut size) }.is_err() {
        return Vec::new();
    }
    // SAFETY: filled above; the identifiers point inside `buffer`.
    let usage = unsafe { &*usage };
    (0..usage.cUsageIdentifier as usize)
        // SAFETY: `cUsageIdentifier` NUL-terminated OIDs.
        .filter_map(|index| unsafe { (*usage.rgpszUsageIdentifier.add(index)).to_string() }.ok())
        .collect()
}

/// The leaf's subject as Windows prints a subject: `CERT_X500_NAME_STR`,
/// most specific first (`CN=…, O=…, …, C=…`) — the form `PowerShell`'s
/// `Subject`, `signtool` and an msix manifest's `Publisher` use.
fn subject(leaf: &Cert) -> String {
    let kind = CERT_STRING_TYPE(CERT_X500_NAME_STR.0 | CERT_NAME_STR_REVERSE_FLAG);
    let name = &leaf.info().Subject;
    // SAFETY: the size query.
    let length = unsafe { CertNameToStrW(X509_ASN_ENCODING, name, kind, None) };
    let mut text = vec![0u16; length as usize];
    // SAFETY: `text` holds `length` units.
    let written = unsafe { CertNameToStrW(X509_ASN_ENCODING, name, kind, Some(&mut text)) };
    String::from_utf16_lossy(&text[..(written as usize).saturating_sub(1)])
}

fn signature_with(path: &Path, engine: &Engine) -> Result<Signature, Refusal> {
    let answer = win_verify_trust(path)?;

    // Step 1. Under an exclusive root, `WinVerifyTrust`'s one expected refusal
    // is the policy's verdict on a chain that ends in a root Windows does not
    // trust, with no provider step in error (a changed byte is the signature
    // step's `TRUST_E_BAD_DIGEST`); that verdict is re-asked of the engine
    // below, at the stamped time.
    if engine.is_exclusive() {
        if answer.status != 0 && !answer.integrity {
            trust_verdict(answer.status)?;
        }
    } else {
        trust_verdict(answer.status)?;
    }
    let leaf = Cert::of_der(&answer.leaf)?;
    let store = Store::of_certificates(&answer.message_certificates)?;

    // Step 2: the RFC 3161 time stamp over this signature.
    let token = answer.rfc3161.as_deref().ok_or(Refusal::NoTimestamp)?;
    let mut context: *mut CRYPT_TIMESTAMP_CONTEXT = null_mut();
    let mut authority: *mut CERT_CONTEXT = null_mut();
    let mut authority_store = HCERTSTORE::default();
    // SAFETY: the token and the stamped signature are live slices; the three
    // outputs are freed below.
    unsafe {
        CryptVerifyTimeStampSignature(
            token,
            Some(&answer.encrypted_hash),
            Some(store.0),
            &mut context,
            &mut authority,
            Some(&mut authority_store),
        )
    }
    .map_err(|error| Refusal::TimestampInvalid {
        code: code_of(&error),
    })?;
    let authority = Cert(authority);
    let authority_store = Store(authority_store);
    // SAFETY: a context `CryptVerifyTimeStampSignature` returned. CryptoAPI
    // packs the decoded structure into one allocation without aligning it,
    // so the time is read unaligned.
    let stamped_at = unsafe {
        let info = std::ptr::addr_of!((*context).pTimeStamp).read_unaligned();
        ticks(std::ptr::addr_of!((*info).ftTime).read_unaligned())
    };
    // SAFETY: allocated by CryptoAPI and freed once.
    unsafe { CryptMemFree(Some(context.cast_const().cast())) };
    let authority_chain = engine
        .chain(&authority, stamped_at, &authority_store, TIME_STAMPING, 0)
        .map_err(|code| Refusal::TimestampInvalid { code })?;
    let authority_error =
        authority_chain.policy_error(CERT_CHAIN_POLICY_BASE, CERT_CHAIN_POLICY_FLAGS(0));
    if authority_error != 0 {
        return Err(Refusal::TimestampInvalid {
            code: authority_error,
        });
    }
    let info = leaf.info();
    timestamp_within(stamped_at, ticks(info.NotBefore), ticks(info.NotAfter))?;

    // Step 3: revocation, read from the signer's chain. Under an exclusive
    // root the chain is this engine's, at the stamped time, and its
    // Authenticode policy is step 1's decision; unknown revocation is left to
    // the verdict, which names it.
    let chain_status = if engine.is_exclusive() {
        let chain = engine
            .chain(
                &leaf,
                stamped_at,
                &store,
                CODE_SIGNING,
                CERT_CHAIN_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT,
            )
            .map_err(|code| Refusal::Untrusted { code })?;
        let code = chain.policy_error(
            CERT_CHAIN_POLICY_AUTHENTICODE,
            CERT_CHAIN_POLICY_IGNORE_ALL_REV_UNKNOWN_FLAGS,
        );
        if code != 0 {
            trust_verdict(code)?;
        }
        chain.error_status()
    } else {
        answer
            .chain_status
            .unwrap_or(super::CHAIN_REVOCATION_UNKNOWN)
    };
    revocation_verdict(chain_status)?;

    Ok(Signature {
        signed: Signed {
            subject: subject(&leaf),
            key_usages: key_usages(&leaf),
            stamped_at,
        },
        leaf,
    })
}

pub(super) fn signature(path: &Path, policy: &Policy) -> Result<Signed, Refusal> {
    let engine = Engine::of(policy)?;
    signature_with(path, &engine).map(|signature| signature.signed)
}

// ── steps 4 to 6 ────────────────────────────────────────────────────────────

fn file_version(path: &Path) -> Result<FileVersion, Refusal> {
    let file = wide(path)?;
    // SAFETY: the path is NUL-terminated.
    let size = unsafe { GetFileVersionInfoSizeW(PCWSTR(file.as_ptr()), None) };
    if size == 0 {
        return Err(Refusal::NoVersion);
    }
    let mut block = vec![0u8; size as usize];
    // SAFETY: `block` holds `size` bytes.
    unsafe { GetFileVersionInfoW(PCWSTR(file.as_ptr()), None, size, block.as_mut_ptr().cast()) }
        .map_err(|_| Refusal::NoVersion)?;
    let mut fixed: *mut c_void = null_mut();
    let mut length = 0u32;
    let root = [u16::from(b'\\'), 0];
    // SAFETY: `block` is the version block just read; the query points
    // inside it.
    let found = unsafe {
        VerQueryValueW(
            block.as_ptr().cast(),
            PCWSTR(root.as_ptr()),
            &mut fixed,
            &mut length,
        )
    };
    if !found.as_bool() || fixed.is_null() || (length as usize) < size_of::<VS_FIXEDFILEINFO>() {
        return Err(Refusal::NoVersion);
    }
    // SAFETY: `VerQueryValueW` found a `VS_FIXEDFILEINFO` of that length.
    let fixed = unsafe { &*fixed.cast::<VS_FIXEDFILEINFO>() };
    Ok(FileVersion::from_halves(
        fixed.dwFileVersionMS,
        fixed.dwFileVersionLS,
    ))
}

fn machine(path: &Path) -> Result<Machine, Refusal> {
    let reader = crate::file_reads::open(crate::file_reads::Lane::Update, path)
        .map_err(|error| Refusal::Unreadable(error.to_string()))?;
    let mut head = Vec::new();
    reader
        .take(HEADER_BYTES)
        .read_to_end(&mut head)
        .map_err(|error| Refusal::Unreadable(error.to_string()))?;
    Machine::of_image_header(&head)
        .ok_or_else(|| Refusal::Unreadable(format!("{} is not a PE image", path.display())))
}

pub(super) fn identity_of(path: &Path, policy: &Policy) -> Result<(Expectation, Signed), Refusal> {
    let engine = Engine::of(policy)?;
    let signature = signature_with(path, &engine)?;
    let identity = Expectation {
        identity_oid: identity_oid(&signature.signed.key_usages)?,
        subject_dn: crate::msix::distinguished_name(&signature.signed.subject),
        version: file_version(path)?,
        machine: machine(path)?,
    };
    drop(signature.leaf);
    Ok((identity, signature.signed))
}

// ── step 7: the package ─────────────────────────────────────────────────────

/// What an msix manifest says about its package.
pub(super) struct PackageIdentity {
    pub(super) publisher: String,
    pub(super) version: FileVersion,
    pub(super) architecture: Option<&'static str>,
}

/// **The manifest of an msix package**, read by Windows' own package reader
/// (`IAppxFactory::CreatePackageReader`), which validates the package's
/// footprint as it opens it.
pub(super) fn package_identity(path: &Path) -> Result<PackageIdentity, Refusal> {
    use windows::Win32::Storage::Packaging::Appx::{
        APPX_PACKAGE_ARCHITECTURE_ARM64, APPX_PACKAGE_ARCHITECTURE_NEUTRAL,
        APPX_PACKAGE_ARCHITECTURE_X64, APPX_PACKAGE_ARCHITECTURE_X86,
    };
    let file = wide(path)?;
    let unreadable = |error: windows::core::Error| Refusal::ManifestUnreadable(error.to_string());
    // SAFETY: this worker's apartment; undone below only when this call
    // entered it (a thread already in another apartment keeps its own).
    let entered = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let read = (|| {
        // SAFETY: an in-process class with no aggregation.
        let factory: IAppxFactory =
            unsafe { CoCreateInstance(&AppxFactory, None, CLSCTX_INPROC_SERVER) }
                .map_err(unreadable)?;
        // SAFETY: the path is NUL-terminated; read-only, sharing reads.
        let stream = unsafe {
            SHCreateStreamOnFileEx(
                PCWSTR(file.as_ptr()),
                (STGM_READ | STGM_SHARE_DENY_WRITE).0,
                0,
                false,
                None,
            )
        }
        .map_err(unreadable)?;
        // SAFETY: COM calls on live interfaces.
        let id = unsafe {
            factory
                .CreatePackageReader(&stream)
                .and_then(|reader| reader.GetManifest())
                .and_then(|manifest| manifest.GetPackageId())
        }
        .map_err(unreadable)?;
        // SAFETY: as above; the string is the callee's, freed below.
        let publisher = unsafe { id.GetPublisher() }.map_err(unreadable)?;
        // SAFETY: a NUL-terminated string from the reader.
        let text = unsafe { publisher.to_string() }.unwrap_or_default();
        // SAFETY: allocated by the reader with the COM allocator.
        unsafe { CoTaskMemFree(Some(publisher.0.cast_const().cast())) };
        // SAFETY: COM calls on a live interface.
        let packed = unsafe { id.GetVersion() }.map_err(unreadable)?;
        // SAFETY: as above.
        let architecture = unsafe { id.GetArchitecture() }.map_err(unreadable)?;
        let architecture = match architecture {
            APPX_PACKAGE_ARCHITECTURE_X64 => Some("x64"),
            APPX_PACKAGE_ARCHITECTURE_ARM64 => Some("arm64"),
            APPX_PACKAGE_ARCHITECTURE_X86 => Some("x86"),
            APPX_PACKAGE_ARCHITECTURE_NEUTRAL => Some("neutral"),
            _ => None,
        };
        Ok(PackageIdentity {
            publisher: text,
            version: FileVersion([
                (packed >> 48) as u16,
                (packed >> 32) as u16,
                (packed >> 16) as u16,
                packed as u16,
            ]),
            architecture,
        })
    })();
    if entered {
        // SAFETY: balances the `CoInitializeEx` that succeeded above.
        unsafe { CoUninitialize() };
    }
    read
}

pub(super) fn verify_package(
    path: &Path,
    expectation: &Expectation,
    policy: &Policy,
) -> Result<Verified, Refusal> {
    let engine = Engine::of(policy)?;
    let signature = signature_with(path, &engine)?;
    let signed = signature.signed;
    let oid = identity_oid(&signed.key_usages)?;
    let subject_dn = crate::msix::distinguished_name(&signed.subject);
    let package = package_identity(path)?;
    publisher_verdict(&package.publisher, &signed.subject)?;
    let found = Expectation {
        identity_oid: oid,
        subject_dn,
        version: package.version,
        machine: expectation.machine,
    };
    compare(expectation, &found)?;
    if package.architecture != expectation.machine.package_architecture() {
        return Err(Refusal::MachineDiffers {
            expected: expectation.machine.to_string(),
            found: package.architecture.unwrap_or("unknown").to_owned(),
        });
    }
    Ok(Verified {
        identity: found,
        signed,
    })
}

/// The engine a policy builds, for the test that pins what it is.
#[cfg(test)]
pub(super) fn engine_of(policy: &Policy) -> Result<Engine, Refusal> {
    Engine::of(policy)
}
