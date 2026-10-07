//! Worker-side Linux clipboard reads for the native X11 or Wayland backend.
//!
//! Raw reads own short-lived source transactions. Writes retain controlled X11
//! or Wayland candidates; the server, not the process, owns the actual selection.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use wl_clipboard_rs::copy::{
    ClipboardType as WriteClipboardType, CopyClaimOutcome, CopyHandle, MimeSource, MimeType,
    Options, Source, copy_multi_owned_until,
};
use wl_clipboard_rs::paste::{ClipboardType, OfferSession, Seat};

use crate::admission::WorkerCtx;
use crate::clipboard::{
    Candidate, ClipboardPayload, ClipboardPort, ClipboardTypes, MAX_PICTURE_BYTES, PictureBytes,
    PictureEncoding, PictureSource, first_offered_picture, read_payload as read_clipboard_payload,
};
use crate::linux_clipboard_x11::{
    ClaimOutcome as X11ClaimOutcome, ReconcileOutcome as X11ReconcileOutcome, X11OwnerCandidate,
};

const URI_LIST: &str = "text/uri-list";
const PNG_MIME: &str = "image/png";
const UTF8_STRING: &str = "UTF8_STRING";
const STRING: &str = "STRING";
const TEXT: &str = "TEXT";
const TEXT_PLAIN: &str = "text/plain";
const TEXT_UTF8: &str = "text/plain;charset=utf-8";
const TEXT_UTF8_UPPER: &str = "text/plain;charset=UTF-8";
const MAX_URI_LIST_BYTES: usize = 8 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;
const MAX_CLIPBOARD_FILES: usize = 4096;
const PROPERTY_WORKING_BYTES: usize = 64 * 1024;
/// The approved Linux clipboard operation budget from admission through result publication.
/// See `docs/plans/design/linux-clipboard-write.md` for the shared read/write lane contract.
pub const LINUX_CLIPBOARD_OPERATION_BUDGET: Duration = Duration::from_secs(4);
const SUPPORTED_MIMES: &[&str] = &[
    URI_LIST,
    PNG_MIME,
    UTF8_STRING,
    STRING,
    TEXT_PLAIN,
    TEXT_UTF8,
    TEXT_UTF8_UPPER,
];

static BACKEND: OnceLock<LinuxClipboardBackend> = OnceLock::new();
static WRITE_CLIPBOARD: OnceLock<Mutex<WriteOwnerState>> = OnceLock::new();

enum WriteOwner {
    X11(X11OwnerCandidate),
    Wayland(CopyHandle),
}

enum WriteOwnerState {
    Stable {
        current: Option<WriteOwner>,
        retiring: Option<WriteOwner>,
    },
    Unconfirmed {
        candidate: WriteOwner,
        previous: Option<WriteOwner>,
    },
}

impl Default for WriteOwnerState {
    fn default() -> Self {
        Self::Stable {
            current: None,
            retiring: None,
        }
    }
}

/// The display system that owns Folio's native window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinuxClipboardBackend {
    X11,
    Wayland,
}

/// Select the clipboard backend from the native window's actual display backend.
pub fn install_backend(backend: LinuxClipboardBackend) -> Result<(), String> {
    match BACKEND.set(backend) {
        Ok(()) => Ok(()),
        Err(backend) if BACKEND.get() == Some(&backend) => Ok(()),
        Err(_) => Err(
            "Linux clipboard backend was already selected for another display system".to_owned(),
        ),
    }
}

fn write_owner_lock() -> std::sync::MutexGuard<'static, WriteOwnerState> {
    WRITE_CLIPBOARD
        .get_or_init(|| Mutex::new(WriteOwnerState::default()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn take_write_owner() -> WriteOwnerState {
    let mut state = write_owner_lock();
    std::mem::take(&mut *state)
}

fn store_write_owner(owner: WriteOwnerState) {
    *write_owner_lock() = owner;
}

fn start_wayland_write(text: String, deadline: Instant) -> Result<CopyHandle, String> {
    let mut options = Options::new();
    options.clipboard(WriteClipboardType::Regular);
    copy_multi_owned_until(
        options,
        vec![MimeSource {
            source: Source::Bytes(text.into_bytes().into()),
            mime_type: MimeType::Text,
        }],
        deadline,
        |body| {
            crate::spawn_at_priority(
                "bt-wayland-clipboard-owner",
                crate::ThreadPriority::Normal,
                move |_worker| body(),
            )
        },
    )
    .map_err(|error| format!("Wayland clipboard owner startup failed: {error}"))
}

fn start_x11_write(
    worker: &WorkerCtx,
    text: String,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
) -> Result<(X11OwnerCandidate, X11ClaimOutcome), String> {
    let mut candidate = X11OwnerCandidate::start(worker, text, deadline, cancelled.clone())
        .map_err(|error| format!("X11 clipboard owner startup failed: {error}"))?;
    let outcome = candidate.await_claim_until(worker, deadline, &cancelled);
    Ok((candidate, outcome))
}

fn retire_one_write_owner(
    worker: &WorkerCtx,
    owner: &mut WriteOwner,
    cutoff: Instant,
    operation_cancelled: Option<&Arc<AtomicBool>>,
) -> Result<(), String> {
    match owner {
        WriteOwner::X11(owner) => {
            owner.retire_until_cancellable(worker, cutoff, operation_cancelled.cloned())
        }
        WriteOwner::Wayland(owner) => {
            owner.retire_until_cancellable(cutoff, operation_cancelled.map(Arc::as_ref))
        }
    }
}

fn cancel_write_owner(owner: &WriteOwner) {
    match owner {
        WriteOwner::X11(owner) => owner.cancel(),
        WriteOwner::Wayland(owner) => owner.cancel(),
    }
}

fn cancel_write_owner_state(owners: &WriteOwnerState) {
    match owners {
        WriteOwnerState::Stable { current, retiring } => {
            if let Some(owner) = current {
                cancel_write_owner(owner);
            }
            if let Some(owner) = retiring {
                cancel_write_owner(owner);
            }
        }
        WriteOwnerState::Unconfirmed {
            candidate,
            previous,
        } => {
            cancel_write_owner(candidate);
            if let Some(owner) = previous {
                cancel_write_owner(owner);
            }
        }
    }
}

fn retire_optional_write_owner(
    worker: &WorkerCtx,
    owner: &mut Option<WriteOwner>,
    cutoff: Instant,
) -> Result<(), String> {
    if let Some(candidate) = owner.as_mut() {
        retire_one_write_owner(worker, candidate, cutoff, None)?;
        *owner = None;
    }
    Ok(())
}

fn retire_write_owner(
    worker: &WorkerCtx,
    owners: &mut WriteOwnerState,
    cutoff: Instant,
) -> Result<(), String> {
    match std::mem::take(owners) {
        WriteOwnerState::Stable {
            mut current,
            mut retiring,
        } => {
            let retiring_error = retire_optional_write_owner(worker, &mut retiring, cutoff).err();
            let current_error = retire_optional_write_owner(worker, &mut current, cutoff).err();
            *owners = WriteOwnerState::Stable { current, retiring };
            retiring_error.or(current_error).map_or(Ok(()), Err)
        }
        WriteOwnerState::Unconfirmed {
            mut candidate,
            mut previous,
        } => {
            let candidate_error =
                retire_one_write_owner(worker, &mut candidate, cutoff, None).err();
            let previous_error = previous
                .as_mut()
                .and_then(|owner| retire_one_write_owner(worker, owner, cutoff, None).err());
            let candidate = candidate_error.is_some().then_some(candidate);
            let previous = if previous_error.is_some() {
                previous
            } else {
                None
            };
            *owners = match (candidate, previous) {
                (Some(candidate), previous) => WriteOwnerState::Unconfirmed {
                    candidate,
                    previous,
                },
                (None, Some(previous)) => WriteOwnerState::Stable {
                    current: None,
                    retiring: Some(previous),
                },
                (None, None) => WriteOwnerState::default(),
            };
            candidate_error.or(previous_error).map_or(Ok(()), Err)
        }
    }
}

fn reconciliation_result(
    worker: &WorkerCtx,
    owner: &mut WriteOwner,
    deadline: Instant,
    cancelled: &Arc<AtomicBool>,
) -> Result<bool, String> {
    match owner {
        WriteOwner::X11(owner) => {
            match owner.reconcile_until(worker, deadline, Arc::clone(cancelled)) {
                X11ReconcileOutcome::CandidateOwns => Ok(true),
                X11ReconcileOutcome::OtherOwner(_) => Ok(false),
                X11ReconcileOutcome::Unconfirmed(error) => Err(error),
            }
        }
        WriteOwner::Wayland(owner) => {
            match owner.reconcile_until(deadline, Arc::clone(cancelled)) {
                CopyClaimOutcome::Confirmed => Ok(true),
                CopyClaimOutcome::FailedBeforeClaim(_)
                | CopyClaimOutcome::ConclusiveNoLongerOwner => Ok(false),
                CopyClaimOutcome::Unconfirmed(error) => Err(error),
            }
        }
    }
}

fn reconcile_unconfirmed_owner(
    worker: &WorkerCtx,
    owners: &mut WriteOwnerState,
    deadline: Instant,
    cancelled: &Arc<AtomicBool>,
) -> Result<(), String> {
    let result = match owners {
        WriteOwnerState::Unconfirmed { candidate, .. } => {
            reconciliation_result(worker, candidate, deadline, cancelled)
        }
        WriteOwnerState::Stable { .. } => return Ok(()),
    };

    match result {
        Err(error) => Err(format!(
            "previous Linux clipboard write is still unconfirmed: {error}"
        )),
        Ok(candidate_owns) => {
            let WriteOwnerState::Unconfirmed {
                candidate,
                previous,
            } = std::mem::take(owners)
            else {
                unreachable!("write owner state was reconciled under the operation lane")
            };
            if candidate_owns {
                *owners = WriteOwnerState::Stable {
                    current: Some(candidate),
                    retiring: previous,
                };
                Ok(())
            } else {
                let mut candidate = candidate;
                match retire_one_write_owner(worker, &mut candidate, deadline, Some(cancelled)) {
                    Ok(()) => {
                        *owners = WriteOwnerState::Stable {
                            current: previous,
                            retiring: None,
                        };
                        Ok(())
                    }
                    Err(error) => {
                        *owners = WriteOwnerState::Stable {
                            current: previous,
                            retiring: Some(candidate),
                        };
                        let _ = error;
                        Ok(())
                    }
                }
            }
        }
    }
}

fn retire_stable_previous(
    worker: &WorkerCtx,
    owners: &mut WriteOwnerState,
    cutoff: Instant,
    cancelled: &Arc<AtomicBool>,
) -> Result<(), String> {
    let WriteOwnerState::Stable { retiring, .. } = owners else {
        return Ok(());
    };
    let Some(owner) = retiring.as_mut() else {
        return Ok(());
    };
    retire_one_write_owner(worker, owner, cutoff, Some(cancelled))?;
    *retiring = None;
    Ok(())
}

fn retire_failed_candidate(
    worker: &WorkerCtx,
    owners: &mut WriteOwnerState,
    candidate: WriteOwner,
    previous: Option<WriteOwner>,
    cutoff: Instant,
    cancelled: &Arc<AtomicBool>,
) {
    let mut candidate = candidate;
    let retired = retire_one_write_owner(worker, &mut candidate, cutoff, Some(cancelled)).is_ok();
    *owners = WriteOwnerState::Stable {
        current: previous,
        retiring: (!retired).then_some(candidate),
    };
}

fn write_candidate(
    worker: &WorkerCtx,
    backend: LinuxClipboardBackend,
    text: String,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
    owners: &mut WriteOwnerState,
) -> Result<(), String> {
    reconcile_unconfirmed_owner(worker, owners, deadline, &cancelled)?;
    retire_stable_previous(worker, owners, deadline, &cancelled)?;

    let previous = match std::mem::take(owners) {
        WriteOwnerState::Stable {
            current,
            retiring: None,
        } => current,
        state => {
            *owners = state;
            return Err("Linux clipboard owner state is not ready for another write".to_owned());
        }
    };

    let (candidate, outcome) = match backend {
        LinuxClipboardBackend::X11 => {
            let (candidate, outcome) =
                match start_x11_write(worker, text, deadline, Arc::clone(&cancelled)) {
                    Ok(candidate) => candidate,
                    Err(error) => {
                        *owners = WriteOwnerState::Stable {
                            current: previous,
                            retiring: None,
                        };
                        return Err(error);
                    }
                };
            (
                WriteOwner::X11(candidate),
                match outcome {
                    X11ClaimOutcome::FailedBeforeClaim(error) => {
                        CopyClaimOutcome::FailedBeforeClaim(error)
                    }
                    X11ClaimOutcome::Confirmed => CopyClaimOutcome::Confirmed,
                    X11ClaimOutcome::Unconfirmed(error) => CopyClaimOutcome::Unconfirmed(error),
                    X11ClaimOutcome::ConclusiveNoLongerOwner(_) => {
                        CopyClaimOutcome::ConclusiveNoLongerOwner
                    }
                },
            )
        }
        LinuxClipboardBackend::Wayland => {
            let candidate = match start_wayland_write(text, deadline) {
                Ok(candidate) => candidate,
                Err(error) => {
                    *owners = WriteOwnerState::Stable {
                        current: previous,
                        retiring: None,
                    };
                    return Err(error);
                }
            };
            let outcome = candidate.await_claim_until(deadline, &cancelled);
            (WriteOwner::Wayland(candidate), outcome)
        }
    };

    match outcome {
        CopyClaimOutcome::Confirmed => {
            *owners = WriteOwnerState::Stable {
                current: Some(candidate),
                retiring: previous,
            };
            // A confirmed selection is publishable. If retirement reaches
            // this operation's cutoff, keep the old joinable owner for the
            // next operation or desktop retirement.
            let _ = retire_stable_previous(worker, owners, deadline, &cancelled);
            Ok(())
        }
        CopyClaimOutcome::FailedBeforeClaim(error) => {
            retire_failed_candidate(worker, owners, candidate, previous, deadline, &cancelled);
            Err(error)
        }
        CopyClaimOutcome::ConclusiveNoLongerOwner => {
            retire_failed_candidate(worker, owners, candidate, previous, deadline, &cancelled);
            Err("clipboard candidate did not own the selection after its claim".to_owned())
        }
        CopyClaimOutcome::Unconfirmed(error) => {
            *owners = WriteOwnerState::Unconfirmed {
                candidate,
                previous,
            };
            Err(error)
        }
    }
}

fn reconcile_before_read(
    worker: &WorkerCtx,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut owners = take_write_owner();
    let result = reconcile_unconfirmed_owner(worker, &mut owners, deadline, &cancelled)
        .and_then(|()| retire_stable_previous(worker, &mut owners, deadline, &cancelled));
    store_write_owner(owners);
    result
}

fn clipboard_types_for_mimes(mime_types: &[String]) -> ClipboardTypes {
    ClipboardTypes {
        files: mime_types.iter().any(|mime| mime == URI_LIST),
        text: mime_types.iter().any(|mime| is_text_mime(mime)),
        picture: mime_types.iter().any(|mime| mime == PNG_MIME),
        promise: false,
    }
}

fn is_text_mime(mime: &str) -> bool {
    matches!(
        mime,
        UTF8_STRING | STRING | TEXT | TEXT_PLAIN | TEXT_UTF8 | TEXT_UTF8_UPPER
    )
}

fn file_urls_from_uri_list(bytes: &[u8]) -> Candidate<Vec<PathBuf>> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Candidate::Unreadable("clipboard file list is not UTF-8".to_owned());
    };
    let lines = text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let file_count = lines
        .clone()
        .filter(|line| {
            line.get(..5)
                .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file:"))
        })
        .count();
    if file_count > MAX_CLIPBOARD_FILES {
        return Candidate::Unreadable("clipboard file list exceeded 4096 files".to_owned());
    }
    crate::clipboard::file_urls(lines.map(|line| Some(line.to_owned())))
}

enum SelectionRead {
    Absent,
    Bytes {
        bytes: Vec<u8>,
        returned_type: Option<u32>,
    },
    Atoms(Vec<u32>),
    TooLarge,
}

enum RawTransport<'a> {
    X11(Option<Box<X11Selection<'a>>>),
    Wayland(Option<Box<OfferSession<'a>>>),
}

struct RawClipboard<'a> {
    transport: RawTransport<'a>,
    deadline: Instant,
    cancelled: &'a AtomicBool,
}

impl<'a> RawClipboard<'a> {
    fn new(
        worker: &'a WorkerCtx,
        backend: LinuxClipboardBackend,
        deadline: Instant,
        cancelled: &'a AtomicBool,
    ) -> Result<Self, String> {
        let transport = match backend {
            LinuxClipboardBackend::X11 => X11Selection::new(worker, deadline, cancelled)
                .map(|selection| RawTransport::X11(selection.map(Box::new))),
            LinuxClipboardBackend::Wayland => {
                let session = match OfferSession::open_for_mimes(
                    ClipboardType::Regular,
                    Seat::Unspecified,
                    SUPPORTED_MIMES,
                    deadline,
                    cancelled,
                ) {
                    Ok(session) => Some(Box::new(session)),
                    Err(wl_clipboard_rs::paste::Error::ClipboardEmpty)
                    | Err(wl_clipboard_rs::paste::Error::NoMimeType) => None,
                    Err(error) => return Err(format!("Wayland clipboard offer failed: {error}")),
                };
                Ok(RawTransport::Wayland(session))
            }
        }?;
        Ok(Self {
            transport,
            deadline,
            cancelled,
        })
    }

    fn clipboard_types(&mut self) -> Result<ClipboardTypes, String> {
        self.check_control()?;
        match &mut self.transport {
            RawTransport::X11(Some(selection)) => selection.clipboard_types(),
            RawTransport::X11(None) | RawTransport::Wayland(None) => Ok(ClipboardTypes::default()),
            RawTransport::Wayland(Some(session)) => {
                Ok(clipboard_types_for_mimes(session.mime_types()))
            }
        }
    }

    fn get_contents(&mut self, mime: &str, max_bytes: usize) -> Result<SelectionRead, String> {
        self.check_control()?;
        match &mut self.transport {
            RawTransport::X11(Some(selection)) => selection.get_mime(mime, max_bytes),
            RawTransport::X11(None) | RawTransport::Wayland(None) => Ok(SelectionRead::Absent),
            RawTransport::Wayland(Some(session)) => {
                if !session.mime_types().iter().any(|offered| offered == mime) {
                    return Ok(SelectionRead::Absent);
                }
                match session.read_contents(mime, max_bytes) {
                    Ok((bytes, returned_mime)) if returned_mime == mime => {
                        Ok(SelectionRead::Bytes {
                            bytes,
                            returned_type: None,
                        })
                    }
                    Ok((_, returned_mime)) => Err(format!(
                        "Wayland clipboard returned MIME type {returned_mime} for {mime}"
                    )),
                    Err(wl_clipboard_rs::paste::Error::ContentTooLarge) => {
                        Ok(SelectionRead::TooLarge)
                    }
                    Err(error) => Err(format!("Wayland clipboard {mime} read failed: {error}")),
                }
            }
        }
    }

    fn text(&mut self) -> Candidate<String> {
        if let Err(error) = self.check_control() {
            return Candidate::Unreadable(error);
        }
        match &mut self.transport {
            RawTransport::X11(Some(selection)) => selection.text(),
            RawTransport::X11(None) | RawTransport::Wayland(None) => Candidate::Absent,
            RawTransport::Wayland(Some(session)) => wayland_text(session),
        }
    }

    fn begin(&mut self) -> Result<(), String> {
        self.check_control()?;
        match &mut self.transport {
            RawTransport::X11(Some(selection)) => selection.verify_identity(),
            RawTransport::X11(None) | RawTransport::Wayland(_) => Ok(()),
        }
    }

    fn finish(&mut self) -> Result<(), String> {
        self.check_control()?;
        match &mut self.transport {
            RawTransport::X11(Some(selection)) => selection.finish(),
            RawTransport::X11(None) | RawTransport::Wayland(_) => Ok(()),
        }
    }

    fn check_control(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err("clipboard read was cancelled".to_owned());
        }
        if Instant::now() >= self.deadline {
            return Err("clipboard read exceeded its deadline".to_owned());
        }
        Ok(())
    }
}

fn wayland_text(session: &mut OfferSession<'_>) -> Candidate<String> {
    const TEXT_ORDER: &[&str] = &[TEXT_UTF8, TEXT_UTF8_UPPER, UTF8_STRING, TEXT_PLAIN, STRING];
    for mime in TEXT_ORDER {
        if !session.mime_types().iter().any(|offered| offered == mime) {
            continue;
        }
        return match session.read_contents(mime, MAX_TEXT_BYTES) {
            Ok((bytes, returned_mime)) if returned_mime == *mime => decode_text_mime(mime, &bytes),
            Ok((_, returned_mime)) => Candidate::Unreadable(format!(
                "Wayland clipboard returned MIME type {returned_mime} for {mime}"
            )),
            Err(wl_clipboard_rs::paste::Error::ContentTooLarge) => {
                Candidate::Unreadable("clipboard text exceeded 8 MiB".to_owned())
            }
            Err(error) => {
                Candidate::Unreadable(format!("Wayland clipboard text read failed: {error}"))
            }
        };
    }
    Candidate::Absent
}

fn decode_text_mime(mime: &str, bytes: &[u8]) -> Candidate<String> {
    if mime == STRING {
        return Candidate::Present(bytes.iter().map(|byte| char::from(*byte)).collect());
    }
    match String::from_utf8(bytes.to_vec()) {
        Ok(text) => Candidate::Present(text),
        Err(_) => Candidate::Unreadable("clipboard text is not valid UTF-8".to_owned()),
    }
}

struct LinuxClipboard<'a> {
    raw: RawClipboard<'a>,
    types: ClipboardTypes,
}

impl ClipboardPort for LinuxClipboard<'_> {
    fn begin(&mut self) -> Result<(), String> {
        self.raw.begin()
    }

    fn survey(&mut self) -> Result<ClipboardTypes, String> {
        self.types = self.raw.clipboard_types()?;
        Ok(self.types)
    }

    fn files(&mut self) -> Candidate<Vec<PathBuf>> {
        if !self.types.files {
            return Candidate::Absent;
        }
        match self.raw.get_contents(URI_LIST, MAX_URI_LIST_BYTES) {
            Ok(SelectionRead::Bytes { bytes, .. }) => file_urls_from_uri_list(&bytes),
            Ok(SelectionRead::Absent) => Candidate::Absent,
            Ok(SelectionRead::TooLarge) => {
                Candidate::Unreadable("clipboard file list exceeded 8 MiB".to_owned())
            }
            Ok(SelectionRead::Atoms(_)) => {
                Candidate::Unreadable("clipboard file list was not byte data".to_owned())
            }
            Err(error) => Candidate::Unreadable(error),
        }
    }

    fn text(&mut self) -> Candidate<String> {
        if self.types.text {
            self.raw.text()
        } else {
            Candidate::Absent
        }
    }

    fn picture(&mut self) -> Candidate<Vec<PictureBytes>> {
        if !self.types.picture {
            return Candidate::Absent;
        }
        let mut source = RawPictureSource {
            raw: &mut self.raw,
            failure: None,
        };
        let answer = first_offered_picture(&[PictureEncoding::Png], &mut source);
        match source.failure {
            Some(error) => Candidate::Unreadable(error),
            None => answer,
        }
    }

    fn finish(&mut self) -> Result<(), String> {
        self.raw.finish()
    }
}

struct RawPictureSource<'a, 'b> {
    raw: &'a mut RawClipboard<'b>,
    failure: Option<String>,
}

impl PictureSource for RawPictureSource<'_, '_> {
    fn offers(&mut self, encoding: PictureEncoding) -> bool {
        encoding == PictureEncoding::Png
    }

    fn read(&mut self, encoding: PictureEncoding) -> Option<Vec<u8>> {
        if encoding != PictureEncoding::Png {
            return None;
        }
        match self.raw.get_contents(PNG_MIME, MAX_PICTURE_BYTES) {
            Ok(SelectionRead::Bytes { bytes, .. }) => Some(bytes),
            Ok(SelectionRead::Absent) => None,
            Ok(SelectionRead::TooLarge) => {
                self.failure = Some("clipboard image/png exceeded 256 MiB".to_owned());
                None
            }
            Ok(SelectionRead::Atoms(_)) => {
                self.failure = Some("clipboard image/png response was not byte data".to_owned());
                None
            }
            Err(error) => {
                self.failure = Some(error);
                None
            }
        }
    }
}

mod x11_transport {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    #[cfg(test)]
    use std::time::Duration;
    use std::time::Instant;

    use x11rb::connection::{Connection, RequestConnection};
    use x11rb::protocol::Event;
    use x11rb::protocol::xfixes;
    use x11rb::protocol::xproto::{
        Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, GetPropertyReply, Property,
        Time, Window, WindowClass,
    };
    #[cfg(test)]
    use x11rb::rust_connection::DefaultStream;
    use x11rb::rust_connection::{PollMode, RustConnection, Stream};

    use super::{
        Candidate, MAX_TEXT_BYTES, PNG_MIME, PROPERTY_WORKING_BYTES, STRING, SelectionRead, TEXT,
        TEXT_PLAIN, TEXT_UTF8, TEXT_UTF8_UPPER, URI_LIST, UTF8_STRING, WorkerCtx,
    };
    #[cfg(test)]
    use crate::linux_clipboard_x11_transport::X11TransportControl;
    use crate::linux_clipboard_x11_transport::{DeadlineStream, connect_display};

    const PROPERTY_NAME: &[u8] = b"FOLIO_CLIPBOARD_DATA";
    struct X11Atoms {
        clipboard: Atom,
        targets: Atom,
        atom: Atom,
        integer: Atom,
        incr: Atom,
        property: Atom,
        timestamp: Atom,
        uri_list: Atom,
        png: Atom,
        utf8_string: Atom,
        string: Atom,
        text: Atom,
        plain: Atom,
        plain_utf8: Atom,
        plain_utf8_upper: Atom,
    }

    impl X11Atoms {
        fn new<S: Stream>(
            worker: &WorkerCtx,
            connection: &RustConnection<S>,
        ) -> Result<Self, String> {
            Ok(Self {
                clipboard: intern(worker, connection, b"CLIPBOARD")?,
                targets: intern(worker, connection, b"TARGETS")?,
                atom: intern(worker, connection, b"ATOM")?,
                integer: intern(worker, connection, b"INTEGER")?,
                incr: intern(worker, connection, b"INCR")?,
                property: intern(worker, connection, PROPERTY_NAME)?,
                timestamp: intern(worker, connection, b"TIMESTAMP")?,
                uri_list: intern(worker, connection, URI_LIST.as_bytes())?,
                png: intern(worker, connection, PNG_MIME.as_bytes())?,
                utf8_string: intern(worker, connection, UTF8_STRING.as_bytes())?,
                string: intern(worker, connection, STRING.as_bytes())?,
                text: intern(worker, connection, TEXT.as_bytes())?,
                plain: intern(worker, connection, TEXT_PLAIN.as_bytes())?,
                plain_utf8: intern(worker, connection, TEXT_UTF8.as_bytes())?,
                plain_utf8_upper: intern(worker, connection, TEXT_UTF8_UPPER.as_bytes())?,
            })
        }

        fn supported(&self) -> Vec<Atom> {
            vec![
                self.uri_list,
                self.png,
                self.utf8_string,
                self.string,
                self.text,
                self.plain,
                self.plain_utf8,
                self.plain_utf8_upper,
            ]
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum TextEncoding {
        Utf8,
        Latin1,
    }

    fn text_encoding(requested: Atom, returned: Atom, atoms: &X11Atoms) -> Option<TextEncoding> {
        if requested == atoms.text {
            return match returned {
                atom if atom == atoms.utf8_string => Some(TextEncoding::Utf8),
                atom if atom == atoms.string => Some(TextEncoding::Latin1),
                _ => None,
            };
        }
        if requested == atoms.string {
            return (returned == atoms.string).then_some(TextEncoding::Latin1);
        }
        if requested == atoms.utf8_string {
            return (returned == atoms.utf8_string).then_some(TextEncoding::Utf8);
        }
        if requested == atoms.plain_utf8 || requested == atoms.plain_utf8_upper {
            return (returned == requested || returned == atoms.utf8_string)
                .then_some(TextEncoding::Utf8);
        }
        if requested == atoms.plain {
            return (returned == atoms.plain || returned == atoms.utf8_string)
                .then_some(TextEncoding::Utf8);
        }
        None
    }

    fn intern<S: Stream>(
        _worker: &WorkerCtx,
        connection: &RustConnection<S>,
        name: &[u8],
    ) -> Result<Atom, String> {
        connection
            .intern_atom(false, name)
            .map_err(|error| format!("X11 clipboard atom request failed: {error}"))?
            .reply()
            .map(|reply| reply.atom)
            .map_err(|error| format!("X11 clipboard atom reply failed: {error}"))
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct SourceIdentity {
        owner: Window,
        change_generation: u64,
        timestamp: Option<u32>,
    }

    fn same_source_identity(snapshot: SourceIdentity, current: SourceIdentity) -> bool {
        snapshot == current
    }

    fn next_selection_generation(
        generation: u64,
        selection: Atom,
        event: &Event,
    ) -> Result<Option<u64>, String> {
        if !matches!(event, Event::XfixesSelectionNotify(event) if event.selection == selection) {
            return Ok(None);
        }
        generation
            .checked_add(1)
            .map(Some)
            .ok_or_else(|| "X11 clipboard source generation overflowed".to_owned())
    }

    pub(super) struct X11Selection<'a> {
        connection: RustConnection<DeadlineStream<'a, 'a>>,
        worker: &'a WorkerCtx,
        window: Option<Window>,
        atoms: X11Atoms,
        source: SourceIdentity,
        change_generation: u64,
        xfixes_tracking: bool,
        pending_events: VecDeque<Event>,
        supported_targets: Vec<Atom>,
        deadline: Instant,
        cancelled: &'a AtomicBool,
    }

    impl<'a> X11Selection<'a> {
        pub(super) fn new(
            worker: &'a WorkerCtx,
            deadline: Instant,
            cancelled: &'a AtomicBool,
        ) -> Result<Option<Self>, String> {
            if cancelled.load(Ordering::Acquire) {
                return Err("X11 clipboard read was cancelled".to_owned());
            }
            if Instant::now() >= deadline {
                return Err("X11 clipboard read exceeded its deadline".to_owned());
            }
            let (connection, screen_num) = connect_display(worker, deadline, cancelled)?;
            let (root, root_visual) = connection
                .setup()
                .roots
                .get(screen_num)
                .map(|screen| (screen.root, screen.root_visual))
                .ok_or_else(|| "X11 clipboard connection has no screen".to_owned())?;
            let atoms = X11Atoms::new(worker, &connection)?;

            let window = connection
                .generate_id()
                .map_err(|error| format!("X11 clipboard requestor window failed: {error}"))?;
            connection
                .create_window(
                    x11rb::COPY_DEPTH_FROM_PARENT,
                    window,
                    root,
                    0,
                    0,
                    1,
                    1,
                    0,
                    WindowClass::INPUT_OUTPUT,
                    root_visual,
                    &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
                )
                .map_err(|error| format!("X11 clipboard requestor window failed: {error}"))?
                .check()
                .map_err(|error| format!("X11 clipboard requestor window failed: {error}"))?;
            connection
                .flush()
                .map_err(|error| format!("X11 clipboard connection failed: {error}"))?;

            let mut selection = Self {
                connection,
                worker,
                window: Some(window),
                atoms,
                source: SourceIdentity {
                    owner: x11rb::NONE,
                    change_generation: 0,
                    timestamp: None,
                },
                change_generation: 0,
                xfixes_tracking: false,
                pending_events: VecDeque::new(),
                supported_targets: Vec::new(),
                deadline,
                cancelled,
            };
            selection.xfixes_tracking = selection.enable_xfixes_tracking(worker)?;
            let Some(source) = selection.capture_source()? else {
                return Ok(None);
            };
            selection.source = source;
            Ok(Some(selection))
        }

        pub(super) fn clipboard_types(&mut self) -> Result<super::ClipboardTypes, String> {
            let supported = self.atoms.supported();
            let targets = match self.selection(
                self.atoms.targets,
                Some(self.atoms.atom),
                32,
                None,
                Some(&supported),
            )? {
                SelectionRead::Absent => return Ok(super::ClipboardTypes::default()),
                SelectionRead::Atoms(targets) => targets,
                SelectionRead::TooLarge => unreachable!("TARGETS has no total-size cap"),
                SelectionRead::Bytes { .. } => {
                    return Err("X11 clipboard TARGETS reply was not an atom list".to_owned());
                }
            };
            self.supported_targets.clone_from(&targets);
            Ok(super::ClipboardTypes {
                files: targets.contains(&self.atoms.uri_list),
                text: self
                    .text_targets()
                    .iter()
                    .any(|atom| targets.contains(atom)),
                picture: targets.contains(&self.atoms.png),
                promise: false,
            })
        }

        pub(super) fn get_mime(
            &mut self,
            mime: &str,
            max_bytes: usize,
        ) -> Result<SelectionRead, String> {
            let target = match mime {
                URI_LIST => self.atoms.uri_list,
                PNG_MIME => self.atoms.png,
                _ => return Err(format!("X11 clipboard MIME type is not supported: {mime}")),
            };
            self.selection(target, Some(target), 8, Some(max_bytes), None)
        }

        pub(super) fn text(&mut self) -> Candidate<String> {
            let order = [
                self.atoms.utf8_string,
                self.atoms.plain_utf8,
                self.atoms.plain_utf8_upper,
                self.atoms.text,
                self.atoms.string,
                self.atoms.plain,
            ];
            for target in order {
                if !self.supported_targets.contains(&target) {
                    continue;
                }
                let read = match self.selection(target, None, 8, Some(MAX_TEXT_BYTES), None) {
                    Ok(SelectionRead::Absent) => continue,
                    Ok(SelectionRead::TooLarge) => {
                        return Candidate::Unreadable("clipboard text exceeded 8 MiB".to_owned());
                    }
                    Ok(SelectionRead::Bytes {
                        bytes,
                        returned_type,
                    }) => (bytes, returned_type),
                    Ok(SelectionRead::Atoms(_)) => {
                        return Candidate::Unreadable(
                            "X11 clipboard text was not byte data".to_owned(),
                        );
                    }
                    Err(error) => return Candidate::Unreadable(error),
                };
                let (bytes, Some(returned_type)) = read else {
                    return Candidate::Unreadable("X11 clipboard text type was missing".to_owned());
                };
                match text_encoding(target, returned_type, &self.atoms) {
                    Some(TextEncoding::Latin1) => {
                        return Candidate::Present(
                            bytes.iter().map(|byte| char::from(*byte)).collect(),
                        );
                    }
                    Some(TextEncoding::Utf8) => {
                        return match String::from_utf8(bytes) {
                            Ok(text) => Candidate::Present(text),
                            Err(_) => Candidate::Unreadable(
                                "X11 clipboard text is not valid UTF-8".to_owned(),
                            ),
                        };
                    }
                    None => {
                        return Candidate::Unreadable(format!(
                            "X11 clipboard text target returned unsupported property type {returned_type}"
                        ));
                    }
                }
            }
            Candidate::Absent
        }

        pub(super) fn verify_identity(&mut self) -> Result<(), String> {
            self.check_control()?;
            let owner_before =
                selection_owner(self.worker, &self.connection, self.atoms.clipboard)?;
            self.drain_events()?;
            self.check_source(owner_before)?;

            if self.source.timestamp.is_some() {
                let timestamp = self.read_timestamp_raw()?;
                self.drain_events()?;
                if timestamp != self.source.timestamp {
                    return Err("X11 clipboard TIMESTAMP changed during read".to_owned());
                }
            }

            let owner_after = selection_owner(self.worker, &self.connection, self.atoms.clipboard)?;
            self.drain_events()?;
            self.check_source(owner_after)?;
            self.check_control()
        }

        pub(super) fn finish(&mut self) -> Result<(), String> {
            if self.window.is_none() {
                return Ok(());
            }
            let result = self.verify_identity();
            if result.is_err() {
                self.destroy_requestor();
            }
            result
        }

        fn text_targets(&self) -> [Atom; 6] {
            [
                self.atoms.utf8_string,
                self.atoms.string,
                self.atoms.text,
                self.atoms.plain,
                self.atoms.plain_utf8,
                self.atoms.plain_utf8_upper,
            ]
        }

        fn enable_xfixes_tracking(&self, _worker: &WorkerCtx) -> Result<bool, String> {
            let Some(_) = self
                .connection
                .extension_information(xfixes::X11_EXTENSION_NAME)
                .map_err(|error| format!("X11 clipboard XFixes query failed: {error}"))?
            else {
                return Ok(false);
            };
            let version = xfixes::query_version(&self.connection, 5, 0)
                .map_err(|error| format!("X11 clipboard XFixes version query failed: {error}"))?
                .reply()
                .map_err(|error| format!("X11 clipboard XFixes version reply failed: {error}"))?;
            if (version.major_version, version.minor_version) < (2, 0) {
                return Ok(false);
            }
            let window = self
                .window
                .ok_or_else(|| "X11 clipboard requestor window is closed".to_owned())?;
            xfixes::select_selection_input(
                &self.connection,
                window,
                self.atoms.clipboard,
                xfixes::SelectionEventMask::SET_SELECTION_OWNER
                    | xfixes::SelectionEventMask::SELECTION_WINDOW_DESTROY
                    | xfixes::SelectionEventMask::SELECTION_CLIENT_CLOSE,
            )
            .map_err(|error| format!("X11 clipboard XFixes subscription failed: {error}"))?
            .check()
            .map_err(|error| format!("X11 clipboard XFixes subscription failed: {error}"))?;
            Ok(true)
        }

        fn capture_source(&mut self) -> Result<Option<SourceIdentity>, String> {
            loop {
                self.check_control()?;
                self.drain_events()?;
                let generation_before = self.change_generation;
                let owner_before =
                    selection_owner(self.worker, &self.connection, self.atoms.clipboard)?;
                if owner_before == x11rb::NONE {
                    self.drain_events()?;
                    if self.change_generation == generation_before {
                        return Ok(None);
                    }
                    continue;
                }

                let timestamp = self.read_timestamp_raw()?;
                self.drain_events()?;
                let owner_after =
                    selection_owner(self.worker, &self.connection, self.atoms.clipboard)?;
                self.drain_events()?;
                if self.change_generation != generation_before || owner_before != owner_after {
                    continue;
                }
                if timestamp.is_none() && !self.xfixes_tracking {
                    return Err(
                        "X11 clipboard owner has no TIMESTAMP and XFixes tracking is unavailable"
                            .to_owned(),
                    );
                }
                return Ok(Some(SourceIdentity {
                    owner: owner_after,
                    change_generation: self.change_generation,
                    timestamp,
                }));
            }
        }

        fn check_source(&self, owner: Window) -> Result<(), String> {
            if !same_source_identity(
                self.source,
                SourceIdentity {
                    owner,
                    change_generation: self.change_generation,
                    timestamp: self.source.timestamp,
                },
            ) {
                return Err("X11 clipboard source changed during read".to_owned());
            }
            Ok(())
        }

        fn drain_events(&mut self) -> Result<(), String> {
            loop {
                let event = self
                    .connection
                    .poll_for_event()
                    .map_err(|error| format!("X11 clipboard event drain failed: {error}"))?;
                let Some(event) = event else {
                    return Ok(());
                };
                if !self.record_selection_change(&event)? && self.is_transfer_event(&event) {
                    self.pending_events.push_back(event);
                }
            }
        }

        fn record_selection_change(&mut self, event: &Event) -> Result<bool, String> {
            let Some(generation) =
                next_selection_generation(self.change_generation, self.atoms.clipboard, event)?
            else {
                return Ok(false);
            };
            self.change_generation = generation;
            Ok(true)
        }

        fn is_transfer_event(&self, event: &Event) -> bool {
            match event {
                Event::SelectionNotify(event) => {
                    Some(event.requestor) == self.window && event.selection == self.atoms.clipboard
                }
                Event::PropertyNotify(event) => {
                    Some(event.window) == self.window && event.atom == self.atoms.property
                }
                _ => false,
            }
        }

        fn read_timestamp_raw(&mut self) -> Result<Option<u32>, String> {
            match self.selection_raw(
                self.atoms.timestamp,
                Some(self.atoms.integer),
                32,
                Some(4),
                None,
            )? {
                SelectionRead::Atoms(values) if values.len() == 1 => Ok(Some(values[0])),
                SelectionRead::TooLarge => {
                    Err("X11 clipboard TIMESTAMP exceeded four bytes".to_owned())
                }
                SelectionRead::Absent => Ok(None),
                _ => Err("X11 clipboard TIMESTAMP reply was malformed".to_owned()),
            }
        }

        fn selection(
            &mut self,
            target: Atom,
            expected_type: Option<Atom>,
            expected_format: u8,
            max_bytes: Option<usize>,
            retain_atoms: Option<&[Atom]>,
        ) -> Result<SelectionRead, String> {
            let result = (|| {
                self.verify_identity()?;
                let result = self.selection_raw(
                    target,
                    expected_type,
                    expected_format,
                    max_bytes,
                    retain_atoms,
                )?;
                self.verify_identity()?;
                Ok(result)
            })();
            if result.is_err() || matches!(&result, Ok(SelectionRead::TooLarge)) {
                self.destroy_requestor();
            }
            result
        }

        fn selection_raw(
            &mut self,
            target: Atom,
            expected_type: Option<Atom>,
            expected_format: u8,
            max_bytes: Option<usize>,
            retain_atoms: Option<&[Atom]>,
        ) -> Result<SelectionRead, String> {
            self.check_control()?;
            let window = self
                .window
                .ok_or_else(|| "X11 clipboard requestor window is closed".to_owned())?;
            self.connection
                .delete_property(window, self.atoms.property)
                .map_err(|error| format!("X11 clipboard property reset failed: {error}"))?;
            self.connection
                .convert_selection(
                    window,
                    self.atoms.clipboard,
                    target,
                    self.atoms.property,
                    Time::CURRENT_TIME,
                )
                .map_err(|error| {
                    format!("X11 clipboard target {target} request failed: {error}")
                })?;
            self.connection.flush().map_err(|error| {
                format!("X11 clipboard target {target} request failed: {error}")
            })?;

            loop {
                let event = self.next_event(self.worker, "selection response")?;
                let Event::SelectionNotify(event) = event else {
                    continue;
                };
                if event.requestor != window
                    || event.selection != self.atoms.clipboard
                    || event.target != target
                {
                    continue;
                }
                if event.property == x11rb::NONE {
                    return Ok(SelectionRead::Absent);
                }
                if event.property != self.atoms.property {
                    continue;
                }
                let first = self.get_property(self.worker, window, false, 0, max_bytes)?;
                if first.type_ == self.atoms.incr {
                    let result = self.read_incr(
                        first,
                        expected_type,
                        expected_format,
                        max_bytes,
                        retain_atoms,
                    );
                    if result.is_err() || matches!(&result, Ok(SelectionRead::TooLarge)) {
                        self.destroy_requestor();
                    }
                    return result;
                }
                let (bytes, returned_type, _received_bytes) = self.read_property(
                    window,
                    first,
                    expected_type,
                    expected_format,
                    max_bytes,
                    retain_atoms,
                )?;
                self.connection
                    .delete_property(window, self.atoms.property)
                    .map_err(|error| format!("X11 clipboard property release failed: {error}"))?;
                self.connection
                    .flush()
                    .map_err(|error| format!("X11 clipboard property release failed: {error}"))?;
                return Ok(match bytes {
                    PropertyValue::Bytes(bytes) => SelectionRead::Bytes {
                        bytes,
                        returned_type: Some(returned_type),
                    },
                    PropertyValue::Atoms(atoms) => SelectionRead::Atoms(atoms),
                });
            }
        }

        fn read_property(
            &mut self,
            window: Window,
            first: GetPropertyReply,
            expected_type: Option<Atom>,
            expected_format: u8,
            max_bytes: Option<usize>,
            retain_atoms: Option<&[Atom]>,
        ) -> Result<(PropertyValue, Atom, usize), String> {
            let mut value = PropertyValue::empty(expected_format);
            let mut reply = first;
            let mut offset = 0_u32;
            let mut total_bytes = 0_usize;
            let returned_type = reply.type_;
            loop {
                self.check_control()?;
                if expected_type.is_some_and(|expected| expected != reply.type_)
                    || reply.type_ != returned_type
                    || reply.format != expected_format
                {
                    return Err(format!(
                        "X11 clipboard returned type {} format {}, expected {:?} format {expected_format}",
                        reply.type_, reply.format, expected_type
                    ));
                }
                let chunk_bytes = reply.value.len();
                if max_bytes.is_some_and(|limit| !within_limit(total_bytes, chunk_bytes, limit)) {
                    return Err("X11 clipboard property exceeded its byte limit".to_owned());
                }
                total_bytes = total_bytes.saturating_add(chunk_bytes);
                value.append(&reply, retain_atoms)?;
                if reply.bytes_after == 0 {
                    return Ok((value, returned_type, total_bytes));
                }
                if chunk_bytes == 0 {
                    return Err("X11 clipboard property stream made no progress".to_owned());
                }
                offset = offset.saturating_add(chunk_bytes.div_ceil(4) as u32);
                reply = self.get_property(
                    self.worker,
                    window,
                    false,
                    offset,
                    max_bytes.map(|limit| limit.saturating_sub(total_bytes)),
                )?;
            }
        }

        fn read_incr(
            &mut self,
            header: GetPropertyReply,
            expected_type: Option<Atom>,
            expected_format: u8,
            max_bytes: Option<usize>,
            retain_atoms: Option<&[Atom]>,
        ) -> Result<SelectionRead, String> {
            if header.format != 32 {
                return Err("X11 clipboard INCR header was not 32-bit".to_owned());
            }
            let announced = header
                .value32()
                .and_then(|mut values| values.next())
                .unwrap_or(0) as usize;
            if max_bytes.is_some_and(|limit| announced > limit) {
                self.destroy_requestor();
                return Ok(SelectionRead::TooLarge);
            }
            let window = self
                .window
                .ok_or_else(|| "X11 clipboard requestor window is closed".to_owned())?;
            self.connection
                .delete_property(window, self.atoms.property)
                .map_err(|error| format!("X11 clipboard INCR acknowledgement failed: {error}"))?;
            self.connection
                .flush()
                .map_err(|error| format!("X11 clipboard INCR acknowledgement failed: {error}"))?;

            let mut value = PropertyValue::empty(expected_format);
            let mut actual_type = None;
            let mut total_bytes = 0_usize;
            loop {
                let event = self.next_event(self.worker, "INCR data")?;
                let Event::PropertyNotify(event) = event else {
                    continue;
                };
                if event.window != window
                    || event.atom != self.atoms.property
                    || event.state != Property::NEW_VALUE
                {
                    continue;
                }
                let first = self.get_property(self.worker, window, false, 0, max_bytes)?;
                if first.format != expected_format
                    || expected_type.is_some_and(|expected| expected != first.type_)
                    || actual_type.is_some_and(|actual| actual != first.type_)
                {
                    return Err("X11 clipboard INCR data type or format changed".to_owned());
                }
                actual_type = Some(first.type_);
                let (chunk, chunk_type, received_bytes) = self.read_property(
                    window,
                    first,
                    expected_type,
                    expected_format,
                    max_bytes.map(|limit| limit.saturating_sub(total_bytes)),
                    retain_atoms,
                )?;
                if actual_type != Some(chunk_type) {
                    return Err("X11 clipboard INCR response type changed".to_owned());
                }
                let chunk_len = received_bytes;
                if chunk_len == 0 {
                    self.connection
                        .delete_property(window, self.atoms.property)
                        .map_err(|error| {
                            format!("X11 clipboard INCR terminator release failed: {error}")
                        })?;
                    self.connection.flush().map_err(|error| {
                        format!("X11 clipboard INCR terminator release failed: {error}")
                    })?;
                    return Ok(match value {
                        PropertyValue::Bytes(bytes) => SelectionRead::Bytes {
                            bytes,
                            returned_type: actual_type,
                        },
                        PropertyValue::Atoms(atoms) => SelectionRead::Atoms(atoms),
                    });
                }
                total_bytes = total_bytes.saturating_add(chunk_len);
                if max_bytes.is_some_and(|limit| total_bytes > limit) {
                    self.destroy_requestor();
                    return Ok(SelectionRead::TooLarge);
                }
                value.append_value(chunk)?;
                self.connection
                    .delete_property(window, self.atoms.property)
                    .map_err(|error| {
                        format!("X11 clipboard INCR acknowledgement failed: {error}")
                    })?;
                self.connection.flush().map_err(|error| {
                    format!("X11 clipboard INCR acknowledgement failed: {error}")
                })?;
            }
        }

        fn get_property(
            &self,
            _worker: &WorkerCtx,
            window: Window,
            delete: bool,
            offset: u32,
            max_bytes: Option<usize>,
        ) -> Result<GetPropertyReply, String> {
            self.check_control()?;
            let working = max_bytes.map_or(PROPERTY_WORKING_BYTES, |remaining| {
                remaining.min(PROPERTY_WORKING_BYTES)
            });
            self.connection
                .get_property(
                    delete,
                    window,
                    self.atoms.property,
                    AtomEnum::ANY,
                    offset,
                    property_words(working),
                )
                .map_err(|error| format!("X11 clipboard property request failed: {error}"))?
                .reply()
                .map_err(|error| format!("X11 clipboard property reply failed: {error}"))
        }

        fn next_event(&mut self, _worker: &WorkerCtx, operation: &str) -> Result<Event, String> {
            loop {
                self.check_control()?;
                if let Some(event) = self.pending_events.pop_front() {
                    return Ok(event);
                }
                if let Some(event) = self
                    .connection
                    .poll_for_event()
                    .map_err(|error| format!("X11 clipboard {operation} failed: {error}"))?
                {
                    self.record_selection_change(&event)?;
                    return Ok(event);
                }
                self.connection
                    .stream()
                    .poll(PollMode::Readable)
                    .map_err(|error| format!("X11 clipboard {operation} failed: {error}"))?;
            }
        }

        fn check_control(&self) -> Result<(), String> {
            if self.cancelled.load(Ordering::Acquire) {
                return Err("X11 clipboard read was cancelled".to_owned());
            }
            if Instant::now() >= self.deadline {
                return Err("X11 clipboard read exceeded its deadline".to_owned());
            }
            Ok(())
        }

        fn destroy_requestor(&mut self) {
            if let Some(window) = self.window.take() {
                let _ = self.connection.destroy_window(window);
                let _ = self.connection.flush();
            }
        }
    }

    impl Drop for X11Selection<'_> {
        fn drop(&mut self) {
            self.destroy_requestor();
        }
    }

    enum PropertyValue {
        Bytes(Vec<u8>),
        Atoms(Vec<u32>),
    }

    impl PropertyValue {
        fn empty(format: u8) -> Self {
            if format == 32 {
                Self::Atoms(Vec::new())
            } else {
                Self::Bytes(Vec::new())
            }
        }

        fn append(
            &mut self,
            reply: &GetPropertyReply,
            retain_atoms: Option<&[Atom]>,
        ) -> Result<(), String> {
            match self {
                Self::Bytes(bytes) => {
                    if reply.format != 8 {
                        return Err(format!(
                            "X11 clipboard property format {} was not 8-bit",
                            reply.format
                        ));
                    }
                    bytes.extend_from_slice(&reply.value);
                }
                Self::Atoms(atoms) => {
                    if reply.format != 32 {
                        return Err(format!(
                            "X11 clipboard property format {} was not 32-bit",
                            reply.format
                        ));
                    }
                    let values = reply
                        .value32()
                        .ok_or_else(|| "X11 clipboard atom list was malformed".to_owned())?;
                    append_supported_atoms(atoms, values, retain_atoms);
                }
            }
            Ok(())
        }

        fn append_value(&mut self, other: Self) -> Result<(), String> {
            match (self, other) {
                (Self::Bytes(bytes), Self::Bytes(chunk)) => bytes.extend_from_slice(&chunk),
                (Self::Atoms(atoms), Self::Atoms(chunk)) => {
                    for atom in chunk {
                        if !atoms.contains(&atom) {
                            atoms.push(atom);
                        }
                    }
                }
                _ => return Err("X11 clipboard INCR format changed during transfer".to_owned()),
            }
            Ok(())
        }
    }

    fn selection_owner<S: Stream>(
        _worker: &WorkerCtx,
        connection: &RustConnection<S>,
        selection: Atom,
    ) -> Result<Window, String> {
        connection
            .get_selection_owner(selection)
            .map_err(|error| format!("X11 clipboard owner query failed: {error}"))?
            .reply()
            .map(|reply| reply.owner)
            .map_err(|error| format!("X11 clipboard owner query failed: {error}"))
    }

    fn property_words(bytes: usize) -> u32 {
        bytes.max(4).div_ceil(4).min((u32::MAX / 4) as usize) as u32
    }

    fn within_limit(current_bytes: usize, next_bytes: usize, limit: usize) -> bool {
        current_bytes <= limit && next_bytes <= limit - current_bytes
    }

    fn append_supported_atoms(
        retained: &mut Vec<Atom>,
        incoming: impl Iterator<Item = Atom>,
        supported: Option<&[Atom]>,
    ) {
        for atom in incoming {
            if supported.is_none_or(|supported| supported.contains(&atom))
                && !retained.contains(&atom)
            {
                retained.push(atom);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::{Read, Write};
        use std::os::unix::net::UnixStream;
        use x11rb::protocol::xproto::{ImageOrder, Screen, Setup};
        use x11rb::x11_utils::Serialize;

        #[test]
        fn x11_source_identity_catches_replacement_by_same_owner() {
            let original = SourceIdentity {
                owner: 7,
                change_generation: 12,
                timestamp: None,
            };
            assert!(same_source_identity(original, original));
            assert!(!same_source_identity(
                original,
                SourceIdentity {
                    owner: 8,
                    ..original
                }
            ));
            assert!(!same_source_identity(
                original,
                SourceIdentity {
                    change_generation: 13,
                    ..original
                }
            ));
            assert!(!same_source_identity(
                original,
                SourceIdentity {
                    timestamp: Some(19),
                    ..original
                }
            ));
        }

        #[test]
        fn xfixes_same_owner_notification_advances_the_source_generation() {
            let original = SourceIdentity {
                owner: 7,
                change_generation: 12,
                timestamp: None,
            };
            let notification = Event::XfixesSelectionNotify(xfixes::SelectionNotifyEvent {
                subtype: xfixes::SelectionEvent::SET_SELECTION_OWNER,
                window: 9,
                owner: 7,
                selection: 4,
                timestamp: 22,
                selection_timestamp: 19,
                ..xfixes::SelectionNotifyEvent::default()
            });
            let generation =
                next_selection_generation(original.change_generation, 4, &notification)
                    .unwrap()
                    .unwrap();
            assert_eq!(generation, 13);
            assert!(!same_source_identity(
                original,
                SourceIdentity {
                    change_generation: generation,
                    ..original
                }
            ));
        }

        #[test]
        fn x11_text_decodes_the_returned_text_type_not_the_requested_target() {
            let atoms = X11Atoms {
                clipboard: 1,
                targets: 2,
                atom: 3,
                integer: 4,
                incr: 5,
                property: 6,
                timestamp: 7,
                uri_list: 8,
                png: 9,
                utf8_string: 10,
                string: 11,
                text: 12,
                plain: 13,
                plain_utf8: 14,
                plain_utf8_upper: 15,
            };
            assert_eq!(
                text_encoding(atoms.text, atoms.utf8_string, &atoms),
                Some(TextEncoding::Utf8)
            );
            assert_eq!(
                text_encoding(atoms.text, atoms.string, &atoms),
                Some(TextEncoding::Latin1)
            );
            assert_eq!(text_encoding(atoms.text, 16, &atoms), None);
        }

        #[test]
        fn target_chunks_keep_only_supported_atoms() {
            let supported = [3, 5, 8];
            let mut retained = Vec::new();
            append_supported_atoms(&mut retained, [1, 3, 4].into_iter(), Some(&supported));
            append_supported_atoms(&mut retained, [3, 5, 8, 9].into_iter(), Some(&supported));
            assert_eq!(retained, [3, 5, 8]);
        }

        #[test]
        fn content_limit_accepts_exact_bytes_and_rejects_overflow() {
            assert!(within_limit(0, 8, 8));
            assert!(within_limit(7, 1, 8));
            assert!(!within_limit(8, 1, 8));
            assert!(!within_limit(usize::MAX, 1, usize::MAX));
        }

        #[test]
        fn x11_setup_observes_preset_cancellation_and_expired_deadline() {
            for (cancelled, deadline, message) in [
                (
                    true,
                    Instant::now() + Duration::from_secs(30),
                    "X11 clipboard read was cancelled",
                ),
                (
                    false,
                    Instant::now() - Duration::from_secs(1),
                    "X11 clipboard read exceeded its deadline",
                ),
            ] {
                let (client, _server) = UnixStream::pair().unwrap();
                crate::spawn_at_priority(
                    "linux-x11-setup-test",
                    crate::ThreadPriority::Normal,
                    move |worker| {
                        let (inner, _) = DefaultStream::from_unix_stream(client).unwrap();
                        let cancelled = AtomicBool::new(cancelled);
                        let control =
                            std::sync::Arc::new(X11TransportControl::reader(deadline, &cancelled));
                        let stream = DeadlineStream::new(inner, worker, control);
                        let error = match RustConnection::connect_to_stream_with_auth_info(
                            stream,
                            0,
                            Vec::new(),
                            Vec::new(),
                        ) {
                            Ok(_) => panic!("preset stream control should refuse setup"),
                            Err(error) => error,
                        };
                        assert!(error.to_string().contains(message));
                    },
                )
                .unwrap()
                .join()
                .unwrap();
            }
        }

        #[test]
        fn stalled_x11_reply_observes_controlled_cancellation() {
            let (client, mut server) = UnixStream::pair().unwrap();
            let (query_seen_tx, query_seen_rx) = std::sync::mpsc::channel();
            let server_thread = std::thread::spawn(move || {
                let setup = Setup {
                    status: 1,
                    protocol_major_version: 11,
                    length: 18,
                    resource_id_base: 0x100000,
                    resource_id_mask: 0x000fff,
                    maximum_request_length: u16::MAX,
                    image_byte_order: ImageOrder::LSB_FIRST,
                    bitmap_format_bit_order: ImageOrder::LSB_FIRST,
                    bitmap_format_scanline_unit: 8,
                    bitmap_format_scanline_pad: 8,
                    min_keycode: 8,
                    max_keycode: 255,
                    roots: vec![Screen {
                        root: 1,
                        root_visual: 3,
                        root_depth: 24,
                        ..Screen::default()
                    }],
                    ..Setup::default()
                };
                server.write_all(&setup.serialize()).unwrap();
                let mut setup_request = [0; 12];
                server.read_exact(&mut setup_request).unwrap();
                let mut request = [0; 64];
                assert_ne!(server.read(&mut request).unwrap(), 0);
                query_seen_tx.send(()).unwrap();
                while server.read(&mut request).unwrap_or(0) != 0 {}
            });
            crate::spawn_at_priority(
                "linux-x11-reply-test",
                crate::ThreadPriority::Normal,
                move |worker| {
                    let (inner, _) = DefaultStream::from_unix_stream(client).unwrap();
                    let cancelled = AtomicBool::new(false);
                    let control = std::sync::Arc::new(X11TransportControl::reader(
                        Instant::now() + Duration::from_secs(30),
                        &cancelled,
                    ));
                    let stream = DeadlineStream::new(inner, worker, control);
                    let connection = RustConnection::connect_to_stream_with_auth_info(
                        stream,
                        0,
                        Vec::new(),
                        Vec::new(),
                    )
                    .unwrap();

                    std::thread::scope(|threads| {
                        let cancelled = &cancelled;
                        threads.spawn(move || {
                            query_seen_rx.recv().unwrap();
                            cancelled.store(true, Ordering::Release);
                        });
                        let error = connection
                            .intern_atom(false, b"FOLIO_DEADLINE_TEST")
                            .unwrap()
                            .reply()
                            .unwrap_err();
                        assert!(
                            error
                                .to_string()
                                .contains("X11 clipboard read was cancelled")
                        );
                    });
                    drop(connection);
                },
            )
            .unwrap()
            .join()
            .unwrap();
            server_thread.join().unwrap();
        }

        #[test]
        fn deadline_stream_poll_checks_the_borrowed_cancel_signal() {
            let (client, _server) = UnixStream::pair().unwrap();
            crate::spawn_at_priority(
                "linux-x11-poll-test",
                crate::ThreadPriority::Normal,
                move |worker| {
                    let (inner, _) = DefaultStream::from_unix_stream(client).unwrap();
                    let cancelled = AtomicBool::new(true);
                    let control = std::sync::Arc::new(X11TransportControl::reader(
                        Instant::now() + Duration::from_secs(1),
                        &cancelled,
                    ));
                    let stream = DeadlineStream::new(inner, worker, control);

                    let error = stream.poll(PollMode::Readable).unwrap_err();
                    assert!(
                        error
                            .to_string()
                            .contains("X11 clipboard read was cancelled")
                    );
                },
            )
            .unwrap()
            .join()
            .unwrap();
        }
    }
}

use x11_transport::X11Selection;

fn read_payload(
    worker: &WorkerCtx,
    backend: LinuxClipboardBackend,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<ClipboardPayload, String> {
    let raw = RawClipboard::new(worker, backend, deadline, cancelled)?;
    read_clipboard_payload(&mut LinuxClipboard {
        raw,
        types: ClipboardTypes::default(),
    })
}

fn read_text(
    worker: &WorkerCtx,
    backend: LinuxClipboardBackend,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<String, String> {
    let mut raw = RawClipboard::new(worker, backend, deadline, cancelled)?;
    raw.begin()?;
    let types = raw.clipboard_types();
    let answer = match types {
        Ok(types) if types.text => match raw.text() {
            Candidate::Present(text) => Ok(text),
            Candidate::Absent => Err("clipboard has no plain text".to_owned()),
            Candidate::Unreadable(error) => Err(error),
        },
        Ok(_) => Err("clipboard has no plain text".to_owned()),
        Err(error) => Err(error),
    };
    raw.finish().and(answer)
}

/// Read rich clipboard data on a worker, using the backend captured from the native window.
pub fn clipboard_payload_on_worker(
    worker: &WorkerCtx,
    backend: LinuxClipboardBackend,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
) -> Result<ClipboardPayload, String> {
    reconcile_before_read(worker, deadline, Arc::clone(&cancelled))?;
    read_payload(worker, backend, deadline, &cancelled)
}

/// Read plain clipboard text on a worker, using the backend captured from the native window.
pub fn clipboard_text_on_worker(
    worker: &WorkerCtx,
    backend: LinuxClipboardBackend,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
) -> Result<String, String> {
    reconcile_before_read(worker, deadline, Arc::clone(&cancelled))?;
    read_text(worker, backend, deadline, &cancelled)
}

/// Put plain text on the clipboard on the Linux worker lane.
pub fn set_clipboard_text_on_worker(
    worker: &WorkerCtx,
    backend: LinuxClipboardBackend,
    text: String,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut owners = take_write_owner();
    let result = write_candidate(worker, backend, text, deadline, cancelled, &mut owners);
    store_write_owner(owners);
    result
}

/// Request cancellation of retained writers without waiting. Desktop retirement
/// later reaps their joinable handles on its bounded worker.
pub fn shutdown() {
    cancel_write_owner_state(&write_owner_lock());
}

/// Release and reap the retained owner on a worker thread during desktop retirement.
pub fn shutdown_on_worker(worker: &WorkerCtx, cutoff: Instant) -> Result<(), String> {
    let mut owners = take_write_owner();
    let result = retire_write_owner(worker, &mut owners, cutoff);
    store_write_owner(owners);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

    #[test]
    fn backend_surveys_only_the_supported_file_text_and_png_rungs() {
        let types = clipboard_types_for_mimes(&[
            URI_LIST.to_owned(),
            TEXT_UTF8.to_owned(),
            PNG_MIME.to_owned(),
            "text/html".to_owned(),
        ]);
        assert!(types.files);
        assert!(types.text);
        assert!(types.picture);
        assert!(!types.promise);

        let unsupported_only = clipboard_types_for_mimes(&["text/html".to_owned()]);
        assert!(!unsupported_only.files);
        assert!(!unsupported_only.text);
        assert!(!unsupported_only.picture);
    }

    #[test]
    fn uri_list_uses_shared_percent_decoder_and_preserves_non_utf8_path_bytes() {
        let Candidate::Present(paths) =
            file_urls_from_uri_list(b"# copied\r\nfile:///tmp/name%20%FF%20one\r\n")
        else {
            panic!("the file URI list should produce one path");
        };
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].as_os_str().as_bytes(), b"/tmp/name \xff one");
    }

    #[test]
    fn uri_list_refuses_more_than_the_approved_file_count() {
        let mut bytes = Vec::new();
        for index in 0..=MAX_CLIPBOARD_FILES {
            bytes.extend_from_slice(format!("file:///tmp/item-{index}\n").as_bytes());
        }
        assert!(matches!(
            file_urls_from_uri_list(&bytes),
            Candidate::Unreadable(message) if message.contains("4096")
        ));
    }

    #[test]
    fn text_prefers_utf8_and_decodes_icccm_string_as_latin1() {
        assert_eq!(
            decode_text_mime(TEXT_UTF8, "snowman ☃".as_bytes()),
            Candidate::Present("snowman ☃".to_owned())
        );
        assert_eq!(
            decode_text_mime(STRING, &[0xe9]),
            Candidate::Present("é".to_owned())
        );
        assert!(matches!(
            decode_text_mime(UTF8_STRING, &[0xff]),
            Candidate::Unreadable(_)
        ));
    }
}
