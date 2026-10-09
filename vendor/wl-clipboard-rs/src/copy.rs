//! Copying and clearing clipboard contents.
// MODIFIED BY THE FOLIO CONTRIBUTORS: retain and cancel a clipboard owner; see CHANGES-FOLIO.md.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Cursor};
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use std::{iter, thread};

use rustix::event::{poll, PollFd, PollFlags, Timespec};
use rustix::fs::{fcntl_setfl, OFlags};
use wayland_backend::client::WaylandError;
use wayland_client::globals::{Global, GlobalListContents};
use wayland_client::protocol::wl_callback::WlCallback;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{
    delegate_dispatch, event_created_child, ConnectError, Dispatch, DispatchError, EventQueue,
    Proxy,
};

use crate::common::{self, initialize};
use crate::data_control::{
    self, impl_dispatch_device, impl_dispatch_manager, impl_dispatch_offer, impl_dispatch_source,
};
use crate::seat_data::SeatData;
use crate::utils::{is_text, PASSWORD_MANAGER_HINT_MIME_TYPE};

const TEXT_PLAIN_MIME: &str = "text/plain";
const COPY_CANCEL_POLL: Duration = Duration::from_millis(50);

/// The clipboard to operate on.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash, PartialOrd, Ord, Default)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub enum ClipboardType {
    /// The regular clipboard.
    #[default]
    Regular,
    /// The "primary" clipboard.
    ///
    /// Working with the "primary" clipboard requires the compositor to support ext-data-control,
    /// or wlr-data-control version 2 or above.
    Primary,
    /// Operate on both clipboards at once.
    ///
    /// Useful for atomically setting both clipboards at once. This option requires the "primary"
    /// clipboard to be supported.
    Both,
}

/// MIME type to offer the copied data under.
#[derive(Clone, Eq, PartialEq, Debug, Hash, PartialOrd, Ord)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub enum MimeType {
    /// Detect the MIME type automatically from the data.
    #[cfg_attr(test, proptest(skip))]
    Autodetect,
    /// Offer a number of common plain text MIME types.
    Text,
    /// Offer a specific MIME type.
    Specific(String),
}

/// Source for copying.
#[derive(Clone, Eq, PartialEq, Debug, Hash, PartialOrd, Ord)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub enum Source {
    /// Copy contents of the standard input.
    #[cfg_attr(test, proptest(skip))]
    StdIn,
    /// Copy the given bytes.
    Bytes(Box<[u8]>),
}

/// Source for copying, with a MIME type.
///
/// Used for [`copy_multi`].
///
/// [`copy_multi`]: fn.copy_multi.html
#[derive(Clone, Eq, PartialEq, Debug, Hash, PartialOrd, Ord)]
pub struct MimeSource {
    pub source: Source,
    pub mime_type: MimeType,
}

/// Seat to operate on.
#[derive(Clone, Eq, PartialEq, Debug, Hash, PartialOrd, Ord, Default)]
pub enum Seat {
    /// Operate on all existing seats at once.
    #[default]
    All,
    /// Operate on a seat with the given name.
    Specific(String),
}

/// Number of paste requests to serve.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash, PartialOrd, Ord, Default)]
pub enum ServeRequests {
    /// Serve requests indefinitely.
    #[default]
    Unlimited,
    /// Serve only the given number of requests.
    Only(usize),
}

/// Options and flags that are used to customize the copying.
#[derive(Clone, Eq, PartialEq, Debug, Default, Hash, PartialOrd, Ord)]
pub struct Options {
    /// The clipboard to work with.
    clipboard: ClipboardType,

    /// The seat to work with.
    seat: Seat,

    /// Trim the trailing newline character before copying.
    ///
    /// This flag is only applied for text MIME types.
    trim_newline: bool,

    /// Do not spawn a separate thread for serving copy requests.
    ///
    /// Setting this flag will result in the call to `copy()` **blocking** until all data sources
    /// it creates are destroyed, e.g. until someone else copies something into the clipboard.
    foreground: bool,

    /// Number of paste requests to serve.
    ///
    /// Limiting the number of paste requests to one effectively clears the clipboard after the
    /// first paste. It can be used when copying e.g. sensitive data, like passwords. Note however
    /// that certain apps may have issues pasting when this option is used, in particular XWayland
    /// clients are known to suffer from this.
    ///
    /// Requests for the [password manager hint][crate::utils::PASSWORD_MANAGER_HINT_MIME_TYPE]
    /// are not counted toward this limit.
    serve_requests: ServeRequests,

    /// Hint that the copied data contains passwords, keys, or other sensitive content.
    ///
    /// Some clipboard managers may react by not persisting the copied data in clipboard history.
    sensitive: bool,

    /// Omit additional text mime types which are offered by default if at least one text mime type is provided.
    ///
    /// Omits additionally offered `text/plain;charset=utf-8`, `text/plain`, `STRING`, `UTF8_STRING` and
    /// `TEXT` mime types which are offered by default if at least one text mime type is provided.
    omit_additional_text_mime_types: bool,
}

/// A copy operation ready to start serving requests.
pub struct PreparedCopy {
    connection: wayland_client::Connection,
    queue: EventQueue<State>,
    state: State,
    sources: Vec<data_control::Source>,
    cancelled: Arc<AtomicBool>,
}

/// A handle that asks one prepared clipboard owner to release its own live sources.
#[derive(Clone)]
pub struct CopyCancelHandle(Arc<AtomicBool>);

impl CopyCancelHandle {
    /// Requests release without waiting for the Wayland event loop to process it.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
}

/// What an owned-copy worker knows about the compositor's last selection
/// barrier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CopyClaimOutcome {
    FailedBeforeClaim(String),
    Confirmed,
    Unconfirmed(String),
    ConclusiveNoLongerOwner,
}

enum OwnerState {
    Preparing,
    ClaimSent,
    Confirmed,
    FailedBeforeClaim(String),
    Unconfirmed(String),
    NoLongerOwner,
    Stopped,
}

struct OwnerShared {
    state: OwnerState,
    reconciliation_pending: bool,
    reconciliation_result: Option<CopyClaimOutcome>,
}

impl OwnerShared {
    fn new() -> Self {
        Self {
            state: OwnerState::Preparing,
            reconciliation_pending: false,
            reconciliation_result: None,
        }
    }
}

enum OwnerCommand {
    Reconcile {
        deadline: Instant,
        cancelled: Arc<AtomicBool>,
    },
}

/// A clipboard copy running on the thread that prepared its Wayland connection.
pub struct CopyHandle {
    cancel: CopyCancelHandle,
    shared: Arc<(Mutex<OwnerShared>, Condvar)>,
    commands: SyncSender<OwnerCommand>,
    worker: Option<thread::JoinHandle<Result<(), Error>>>,
}

impl CopyHandle {
    /// Requests this copy's source to be released.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Wait for the original, post-claim server barrier up to this request's
    /// deadline. A timeout leaves the owner joinable for reconciliation.
    pub fn await_claim_until(&self, deadline: Instant, cancelled: &AtomicBool) -> CopyClaimOutcome {
        let (state_lock, changed) = &*self.shared;
        let mut state = state_lock.lock().unwrap_or_else(|error| error.into_inner());
        loop {
            match &state.state {
                OwnerState::Confirmed => return CopyClaimOutcome::Confirmed,
                OwnerState::FailedBeforeClaim(error) => {
                    return CopyClaimOutcome::FailedBeforeClaim(error.clone());
                }
                OwnerState::Unconfirmed(error) => {
                    return CopyClaimOutcome::Unconfirmed(error.clone());
                }
                OwnerState::NoLongerOwner => return CopyClaimOutcome::ConclusiveNoLongerOwner,
                OwnerState::Stopped => {
                    return CopyClaimOutcome::Unconfirmed(
                        "Wayland clipboard owner stopped before confirmation".to_owned(),
                    );
                }
                OwnerState::Preparing | OwnerState::ClaimSent => {}
            }
            if cancelled.load(Ordering::Acquire) {
                self.cancel.cancel();
                return CopyClaimOutcome::Unconfirmed(
                    "Wayland clipboard owner operation was cancelled".to_owned(),
                );
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return CopyClaimOutcome::Unconfirmed(
                    "Wayland clipboard owner operation exceeded its deadline".to_owned(),
                );
            }
            let wait = remaining.min(COPY_CANCEL_POLL);
            let result = changed
                .wait_timeout(state, wait)
                .unwrap_or_else(|error| error.into_inner());
            state = result.0;
        }
    }

    /// Reconcile an earlier selection claim on its original connection. This
    /// sends a sync barrier only; it never claims the selection again.
    pub fn reconcile_until(
        &self,
        deadline: Instant,
        cancelled: Arc<AtomicBool>,
    ) -> CopyClaimOutcome {
        let (state_lock, changed) = &*self.shared;
        let mut state = state_lock.lock().unwrap_or_else(|error| error.into_inner());
        match &state.state {
            OwnerState::FailedBeforeClaim(error) => {
                return CopyClaimOutcome::FailedBeforeClaim(error.clone());
            }
            OwnerState::NoLongerOwner => return CopyClaimOutcome::ConclusiveNoLongerOwner,
            OwnerState::Stopped => {
                return CopyClaimOutcome::Unconfirmed(
                    "Wayland clipboard owner stopped before reconciliation".to_owned(),
                );
            }
            OwnerState::Preparing
            | OwnerState::ClaimSent
            | OwnerState::Confirmed
            | OwnerState::Unconfirmed(_) => {}
        }
        if !state.reconciliation_pending {
            state.reconciliation_pending = true;
            state.reconciliation_result = None;
            drop(state);
            if self
                .commands
                .try_send(OwnerCommand::Reconcile {
                    deadline,
                    cancelled: Arc::clone(&cancelled),
                })
                .is_err()
            {
                let mut state = state_lock.lock().unwrap_or_else(|error| error.into_inner());
                state.reconciliation_pending = false;
                let outcome = CopyClaimOutcome::Unconfirmed(
                    "Wayland clipboard owner worker is unavailable for reconciliation".to_owned(),
                );
                state.reconciliation_result = Some(outcome.clone());
                changed.notify_all();
                return outcome;
            }
            state = state_lock.lock().unwrap_or_else(|error| error.into_inner());
        }

        loop {
            if let Some(outcome) = state.reconciliation_result.take() {
                state.reconciliation_pending = false;
                return outcome;
            }
            if cancelled.load(Ordering::Acquire) {
                return CopyClaimOutcome::Unconfirmed(
                    "Wayland clipboard owner reconciliation was cancelled".to_owned(),
                );
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return CopyClaimOutcome::Unconfirmed(
                    "Wayland clipboard owner reconciliation exceeded its deadline".to_owned(),
                );
            }
            let result = changed
                .wait_timeout(state, remaining.min(COPY_CANCEL_POLL))
                .unwrap_or_else(|error| error.into_inner());
            state = result.0;
        }
    }

    /// Cancel the owner and wait for its cancellation-aware serving loop to
    /// retire before the caller releases this candidate.
    pub fn retire_until(&mut self, cutoff: Instant) -> Result<(), String> {
        self.retire_until_cancellable(cutoff, None)
    }

    /// Let an admitted operation stop waiting for retirement while preserving
    /// this handle for desktop retirement to join later.
    pub fn retire_until_cancellable(
        &mut self,
        cutoff: Instant,
        operation_cancelled: Option<&AtomicBool>,
    ) -> Result<(), String> {
        if operation_cancelled.is_some_and(|cancelled| cancelled.load(Ordering::Acquire)) {
            return Err("Wayland clipboard owner retirement was canceled by its caller".to_owned());
        }
        self.cancel();
        if Instant::now() >= cutoff {
            return Err("Wayland clipboard owner retirement exceeded its cutoff".to_owned());
        }
        loop {
            let Some(worker) = self.worker.as_ref() else {
                return Ok(());
            };
            if worker.is_finished() {
                self.worker
                    .take()
                    .expect("finished owner worker is present")
                    .join()
                    .map_err(|_| "Wayland clipboard owner worker panicked".to_owned())?
                    .map_err(|error| error.to_string())?;
                return Ok(());
            }
            if operation_cancelled.is_some_and(|cancelled| cancelled.load(Ordering::Acquire)) {
                return Err(
                    "Wayland clipboard owner retirement was canceled by its caller".to_owned(),
                );
            }
            let remaining = cutoff.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("Wayland clipboard owner retirement exceeded its cutoff".to_owned());
            }
            thread::sleep(remaining.min(COPY_CANCEL_POLL));
        }
    }
}

impl Drop for CopyHandle {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Errors that can occur for copying the source data to a temporary file.
#[derive(thiserror::Error, Debug)]
pub enum SourceCreationError {
    #[error("Couldn't create a temporary directory")]
    TempDirCreate(#[source] io::Error),

    #[error("Couldn't create a temporary file")]
    TempFileCreate(#[source] io::Error),

    #[error("Couldn't copy data to the temporary file")]
    DataCopy(#[source] io::Error),

    #[error("Couldn't write to the temporary file")]
    TempFileWrite(#[source] io::Error),

    #[error("Couldn't open the temporary file for newline trimming")]
    TempFileOpen(#[source] io::Error),

    #[error("Couldn't get the temporary file metadata for newline trimming")]
    TempFileMetadata(#[source] io::Error),

    #[error("Couldn't seek the temporary file for newline trimming")]
    TempFileSeek(#[source] io::Error),

    #[error("Couldn't read the last byte of the temporary file for newline trimming")]
    TempFileRead(#[source] io::Error),

    #[error("Couldn't truncate the temporary file for newline trimming")]
    TempFileTruncate(#[source] io::Error),
}

/// Errors that can occur for copying and clearing the clipboard.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("There are no seats")]
    NoSeats,

    #[error("Couldn't open the provided Wayland socket")]
    SocketOpenError(#[source] io::Error),

    #[error("Couldn't connect to the Wayland compositor")]
    WaylandConnection(#[source] ConnectError),

    #[error("Wayland compositor communication error")]
    WaylandCommunication(#[source] DispatchError),

    #[error(
        "A required Wayland protocol ({} version {}) is not supported by the compositor",
        name,
        version
    )]
    MissingProtocol { name: &'static str, version: u32 },

    #[error("The compositor does not support primary selection")]
    PrimarySelectionUnsupported,

    #[error("The requested seat was not found")]
    SeatNotFound,

    #[error("Error copying the source into a temporary file")]
    TempCopy(#[source] SourceCreationError),

    #[error("Couldn't remove the temporary file")]
    TempFileRemove(#[source] io::Error),

    #[error("Couldn't remove the temporary directory")]
    TempDirRemove(#[source] io::Error),

    #[error("Error satisfying a paste request")]
    Paste(#[source] DataSourceError),
}

impl From<common::Error> for Error {
    fn from(x: common::Error) -> Self {
        use common::Error::*;

        match x {
            SocketOpenError(err) => Error::SocketOpenError(err),
            WaylandConnection(err) => Error::WaylandConnection(err),
            WaylandCommunication(err) => Error::WaylandCommunication(err.into()),
            MissingProtocol { name, version } => Error::MissingProtocol { name, version },
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum DataSourceError {
    #[error("Couldn't open the data file")]
    FileOpen(#[source] io::Error),

    #[error("Couldn't copy the data to the target file descriptor")]
    Copy(#[source] io::Error),
}

enum SendProgress {
    Complete,
    Cancelled,
}

fn write_cancellable(
    fd: &OwnedFd,
    contents: &[u8],
    cancelled: &AtomicBool,
) -> io::Result<SendProgress> {
    fcntl_setfl(fd, OFlags::NONBLOCK).map_err(io::Error::from)?;
    let poll_timeout = Timespec::try_from(COPY_CANCEL_POLL)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let mut poll_fds = [PollFd::new(
        fd,
        PollFlags::OUT | PollFlags::ERR | PollFlags::HUP,
    )];
    let mut offset = 0;

    while offset < contents.len() {
        if cancelled.load(Ordering::Acquire) {
            return Ok(SendProgress::Cancelled);
        }

        match rustix::io::write(fd, &contents[offset..]) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
            Ok(written) => offset += written,
            Err(rustix::io::Errno::INTR) => continue,
            Err(rustix::io::Errno::AGAIN) => loop {
                if cancelled.load(Ordering::Acquire) {
                    return Ok(SendProgress::Cancelled);
                }
                match poll(&mut poll_fds, Some(&poll_timeout)) {
                    Ok(0) => continue,
                    Ok(_) => {
                        let events = poll_fds[0].revents();
                        if events.intersects(PollFlags::ERR | PollFlags::HUP) {
                            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
                        }
                        if events.contains(PollFlags::OUT) {
                            break;
                        }
                    }
                    Err(rustix::io::Errno::INTR) => continue,
                    Err(error) => return Err(error.into()),
                }
            },
            Err(error) => return Err(error.into()),
        }
    }

    Ok(SendProgress::Complete)
}

/// An inner source of data in-memory.
///
/// This is always cheap to clone.
#[derive(Clone)]
struct DataSourceStorage(Arc<[u8]>);

struct DataSource {
    mime_type: String,
    source: DataSourceStorage,
}

struct State {
    common: Option<common::State>,
    registry_globals: Vec<Global>,
    got_primary_selection: bool,
    // This bool can be set to true when serving a request: either if an error occurs, or if the
    // number of requests to serve was limited and the last request was served.
    should_quit: bool,
    data_sources: HashMap<String, DataSourceStorage>,
    serve_requests: ServeRequests,
    // Set only by the owned cancellable serving path. Legacy copy keeps its blocking write path.
    owned_cancellation: Option<Arc<AtomicBool>>,
    // An error that occurred while serving a request, if any.
    error: Option<DataSourceError>,
}

delegate_dispatch!(State: [WlSeat: ()] => common::State);

impl AsMut<common::State> for State {
    fn as_mut(&mut self) -> &mut common::State {
        self.common
            .as_mut()
            .expect("clipboard event dispatch starts after registry setup")
    }
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        _proxy: &WlRegistry,
        event: <WlRegistry as wayland_client::Proxy>::Event,
        _data: &(),
        _connection: &wayland_client::Connection,
        _queue: &wayland_client::QueueHandle<Self>,
    ) {
        match event {
            wayland_client::protocol::wl_registry::Event::Global {
                name,
                interface,
                version,
            } => state.registry_globals.push(Global {
                name,
                interface,
                version,
            }),
            wayland_client::protocol::wl_registry::Event::GlobalRemove { name } => {
                state.registry_globals.retain(|global| global.name != name);
            }
            _ => {}
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WlRegistry,
        _event: <WlRegistry as wayland_client::Proxy>::Event,
        _data: &GlobalListContents,
        _conn: &wayland_client::Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlCallback, common::SyncTicket> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WlCallback,
        event: <WlCallback as Proxy>::Event,
        data: &common::SyncTicket,
        _connection: &wayland_client::Connection,
        _queue: &wayland_client::QueueHandle<Self>,
    ) {
        if let wayland_client::protocol::wl_callback::Event::Done { .. } = event {
            data.0.store(true, Ordering::Release);
        }
    }
}

impl_dispatch_manager!(State);

impl_dispatch_device!(State, WlSeat, |state: &mut Self, event, seat| {
    match event {
        Event::DataOffer { id } => id.destroy(),
        Event::Finished => {
            state.as_mut().seats.get_mut(seat).unwrap().set_device(None);
        }
        Event::PrimarySelection { .. } => {
            state.got_primary_selection = true;
        }
        _ => (),
    }
});

impl_dispatch_offer!(State);

impl_dispatch_source!(State, |state: &mut Self,
                              source: data_control::Source,
                              event| {
    match event {
        Event::Send { mime_type, fd } => {
            // Check if some other source already handled a paste request and indicated that we should
            // quit.
            if state.should_quit {
                source.destroy();
                return;
            }

            // I'm not sure if it's the compositor's responsibility to check that the mime type is
            // valid. Let's check here just in case.
            let data_source = match state.data_sources.get(&mime_type) {
                Some(source) => source.clone(),
                None => {
                    return;
                }
            };

            let copy_result = if let Some(cancelled) = state.owned_cancellation.as_ref() {
                match write_cancellable(&fd, &data_source.0, cancelled) {
                    Ok(SendProgress::Complete) => Ok(()),
                    Ok(SendProgress::Cancelled) => return,
                    Err(error) => Err(error),
                }
            } else {
                let copy_result = || {
                    // Clear O_NONBLOCK, otherwise io::copy() will stop halfway.
                    fcntl_setfl(&fd, OFlags::empty()).map_err(io::Error::from)?;
                    let mut target_file = File::from(fd);

                    let mut source_content = Cursor::new(&data_source.0);
                    io::copy(&mut source_content, &mut target_file).map(drop)
                };
                copy_result()
            };

            // EPIPE means the destination closed the pipe early, which is valid
            // behavior (e.g. the pasting program only read as much as it needed).
            match copy_result {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::BrokenPipe => {}
                Err(e) => state.error = Some(DataSourceError::Copy(e)),
            }

            // Reading the password-manager hint doesn't count toward serve_requests.
            let done = if mime_type == PASSWORD_MANAGER_HINT_MIME_TYPE {
                false
            } else if let ServeRequests::Only(left) = state.serve_requests {
                let left = left.checked_sub(1).unwrap();
                state.serve_requests = ServeRequests::Only(left);
                left == 0
            } else {
                false
            };

            if done || state.error.is_some() {
                state.should_quit = true;
                source.destroy();
            }
        }
        Event::Cancelled => source.destroy(),
        _ => (),
    }
});

impl Options {
    /// Creates a blank new set of options ready for configuration.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the clipboard to work with.
    #[inline]
    pub fn clipboard(&mut self, clipboard: ClipboardType) -> &mut Self {
        self.clipboard = clipboard;
        self
    }

    /// Sets the seat to use for copying.
    #[inline]
    pub fn seat(&mut self, seat: Seat) -> &mut Self {
        self.seat = seat;
        self
    }

    /// Sets the flag for trimming the trailing newline.
    ///
    /// This flag is only applied for text MIME types.
    #[inline]
    pub fn trim_newline(&mut self, trim_newline: bool) -> &mut Self {
        self.trim_newline = trim_newline;
        self
    }

    /// Sets the flag for not spawning a separate thread for serving copy requests.
    ///
    /// Setting this flag will result in the call to `copy()` **blocking** until all data sources
    /// it creates are destroyed, e.g. until someone else copies something into the clipboard.
    #[inline]
    pub fn foreground(&mut self, foreground: bool) -> &mut Self {
        self.foreground = foreground;
        self
    }

    /// Sets the number of requests to serve.
    ///
    /// Limiting the number of requests to one effectively clears the clipboard after the first
    /// paste. It can be used when copying e.g. sensitive data, like passwords. Note however that
    /// certain apps may have issues pasting when this option is used, in particular XWayland
    /// clients are known to suffer from this.
    ///
    /// Requests for the [password manager hint][crate::utils::PASSWORD_MANAGER_HINT_MIME_TYPE]
    /// are not counted toward this limit.
    #[inline]
    pub fn serve_requests(&mut self, serve_requests: ServeRequests) -> &mut Self {
        self.serve_requests = serve_requests;
        self
    }

    /// Sets the flag for hinting that the copied data contains passwords, keys, or other sensitive content.
    ///
    /// Some clipboard managers may react by not persisting the copied data in clipboard history.
    ///
    /// Offers [`x-kde-passwordManagerHint`][crate::utils::PASSWORD_MANAGER_HINT_MIME_TYPE] with the contents `secret`, unless that MIME
    /// type was supplied explicitly.
    #[inline]
    pub fn sensitive(&mut self, sensitive: bool) -> &mut Self {
        self.sensitive = sensitive;
        self
    }

    /// Sets the flag for omitting additional text mime types which are offered by default if at least one text mime type is provided.
    ///
    /// Omits additionally offered `text/plain;charset=utf-8`, `text/plain`, `STRING`, `UTF8_STRING` and
    /// `TEXT` mime types which are offered by default if at least one text mime type is provided.
    #[inline]
    pub fn omit_additional_text_mime_types(
        &mut self,
        omit_additional_text_mime_types: bool,
    ) -> &mut Self {
        self.omit_additional_text_mime_types = omit_additional_text_mime_types;
        self
    }

    /// Invokes the copy operation. See `copy()`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # extern crate wl_clipboard_rs;
    /// # use wl_clipboard_rs::copy::Error;
    /// # fn foo() -> Result<(), Error> {
    /// use wl_clipboard_rs::copy::{MimeType, Options, Source};
    ///
    /// let opts = Options::new();
    /// opts.copy(Source::Bytes([1, 2, 3][..].into()), MimeType::Autodetect)?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn copy(self, source: Source, mime_type: MimeType) -> Result<(), Error> {
        copy(self, source, mime_type)
    }

    /// Invokes the copy_multi operation. See `copy_multi()`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # extern crate wl_clipboard_rs;
    /// # use wl_clipboard_rs::copy::Error;
    /// # fn foo() -> Result<(), Error> {
    /// use wl_clipboard_rs::copy::{MimeSource, MimeType, Options, Source};
    ///
    /// let opts = Options::new();
    /// opts.copy_multi(vec![
    ///     MimeSource {
    ///         source: Source::Bytes([1, 2, 3][..].into()),
    ///         mime_type: MimeType::Autodetect,
    ///     },
    ///     MimeSource {
    ///         source: Source::Bytes([7, 8, 9][..].into()),
    ///         mime_type: MimeType::Text,
    ///     },
    /// ])?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn copy_multi(self, sources: Vec<MimeSource>) -> Result<(), Error> {
        copy_multi(self, sources)
    }

    /// Invokes the prepare_copy operation. See `prepare_copy()`.
    ///
    /// # Panics
    ///
    /// Panics if `foreground` is `false`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # extern crate wl_clipboard_rs;
    /// # use wl_clipboard_rs::copy::Error;
    /// # fn foo() -> Result<(), Error> {
    /// use wl_clipboard_rs::copy::{MimeSource, MimeType, Options, Source};
    ///
    /// let mut opts = Options::new();
    /// opts.foreground(true);
    /// let prepared_copy =
    ///     opts.prepare_copy(Source::Bytes([1, 2, 3][..].into()), MimeType::Autodetect)?;
    /// prepared_copy.serve()?;
    ///
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn prepare_copy(self, source: Source, mime_type: MimeType) -> Result<PreparedCopy, Error> {
        prepare_copy(self, source, mime_type)
    }

    /// Invokes the prepare_copy_multi operation. See `prepare_copy_multi()`.
    ///
    /// # Panics
    ///
    /// Panics if `foreground` is `false`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # extern crate wl_clipboard_rs;
    /// # use wl_clipboard_rs::copy::Error;
    /// # fn foo() -> Result<(), Error> {
    /// use wl_clipboard_rs::copy::{MimeSource, MimeType, Options, Source};
    ///
    /// let mut opts = Options::new();
    /// opts.foreground(true);
    /// let prepared_copy = opts.prepare_copy_multi(vec![
    ///     MimeSource {
    ///         source: Source::Bytes([1, 2, 3][..].into()),
    ///         mime_type: MimeType::Autodetect,
    ///     },
    ///     MimeSource {
    ///         source: Source::Bytes([7, 8, 9][..].into()),
    ///         mime_type: MimeType::Text,
    ///     },
    /// ])?;
    /// prepared_copy.serve()?;
    ///
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn prepare_copy_multi(self, sources: Vec<MimeSource>) -> Result<PreparedCopy, Error> {
        prepare_copy_multi(self, sources)
    }
}

impl PreparedCopy {
    /// Sends the selection claim and its offers to the compositor.
    ///
    /// Call this before reporting a successful asynchronous start.
    pub fn flush_for_owner(&self) -> Result<(), Error> {
        self.queue
            .flush()
            .map_err(|error| Error::WaylandCommunication(DispatchError::Backend(error)))
    }

    /// Returns a handle for requesting this copy's own sources to be released.
    pub fn cancel_handle(&self) -> CopyCancelHandle {
        CopyCancelHandle(Arc::clone(&self.cancelled))
    }

    /// Starts serving copy requests.
    ///
    /// This function **blocks** until all requests are served or the clipboard is taken over by
    /// some other application.
    pub fn serve(mut self) -> Result<(), Error> {
        // Loop until we're done.
        while !self.state.should_quit {
            self.queue
                .blocking_dispatch(&mut self.state)
                .map_err(Error::WaylandCommunication)?;

            // Check if all sources have been destroyed.
            let all_destroyed = self.sources.iter().all(|x| !x.is_alive());
            if all_destroyed {
                self.state.should_quit = true;
            }
        }

        // Check if an error occurred during data transfer.
        if let Some(err) = self.state.error.take() {
            return Err(Error::Paste(err));
        }

        Ok(())
    }

    /// Serves requests until the selection is replaced or [`CopyCancelHandle::cancel`] is called.
    ///
    /// Cancellation destroys only the live data sources created by this prepared copy and flushes
    /// those requests on their owning Wayland connection.
    pub fn serve_cancellable(mut self) -> Result<(), Error> {
        self.state.owned_cancellation = Some(Arc::clone(&self.cancelled));
        while !self.state.should_quit {
            if self.cancelled.load(Ordering::Acquire) {
                for source in &self.sources {
                    if source.is_alive() {
                        source.destroy();
                    }
                }
                self.queue
                    .flush()
                    .map_err(|error| Error::WaylandCommunication(DispatchError::Backend(error)))?;
                return self.finish_transfer();
            }

            if self.sources.iter().all(|source| !source.is_alive()) {
                break;
            }

            self.dispatch_until_change_or_cancel()?;
        }

        self.finish_transfer()
    }

    fn dispatch_until_change_or_cancel(&mut self) -> Result<(), Error> {
        let dispatched = self
            .queue
            .dispatch_pending(&mut self.state)
            .map_err(Error::WaylandCommunication)?;
        if dispatched > 0 {
            return Ok(());
        }

        self.queue
            .flush()
            .map_err(|error| Error::WaylandCommunication(DispatchError::Backend(error)))?;
        if self.cancelled.load(Ordering::Acquire) {
            return Ok(());
        }

        let Some(guard) = self.queue.prepare_read() else {
            return Ok(());
        };
        let wayland_fd = guard.connection_fd();
        let timeout = Timespec::try_from(Duration::from_millis(50)).map_err(|error| {
            Error::WaylandCommunication(DispatchError::Backend(WaylandError::Io(io::Error::other(
                error.to_string(),
            ))))
        })?;
        let mut poll_fds = [PollFd::new(&wayland_fd, PollFlags::IN | PollFlags::ERR)];
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return Ok(());
            }
            match poll(&mut poll_fds, Some(&timeout)) {
                Ok(0) => return Ok(()),
                Ok(_) => break,
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => {
                    return Err(Error::WaylandCommunication(DispatchError::Backend(
                        WaylandError::Io(error.into()),
                    )));
                }
            }
        }

        guard
            .read()
            .map_err(|error| Error::WaylandCommunication(DispatchError::Backend(error)))?;
        self.queue
            .dispatch_pending(&mut self.state)
            .map_err(Error::WaylandCommunication)?;
        Ok(())
    }

    fn finish_transfer(&mut self) -> Result<(), Error> {
        if let Some(error) = self.state.error.take() {
            return Err(Error::Paste(error));
        }
        Ok(())
    }

    fn serve_owned(
        mut self,
        mut initial_claim: Option<common::PendingRoundtrip>,
        shared: Arc<(Mutex<OwnerShared>, Condvar)>,
        commands: Receiver<OwnerCommand>,
        lifecycle_cancelled: Arc<AtomicBool>,
    ) -> Result<(), Error> {
        self.state.owned_cancellation = Some(Arc::clone(&lifecycle_cancelled));

        loop {
            if lifecycle_cancelled.load(Ordering::Acquire) {
                for source in &self.sources {
                    if source.is_alive() {
                        source.destroy();
                    }
                }
                self.queue
                    .flush()
                    .map_err(|error| Error::WaylandCommunication(DispatchError::Backend(error)))?;
                set_owner_state(&shared, OwnerState::Stopped);
                return self.finish_transfer();
            }

            self.queue
                .dispatch_pending(&mut self.state)
                .map_err(Error::WaylandCommunication)?;

            if initial_claim
                .as_ref()
                .is_some_and(common::PendingRoundtrip::is_complete)
            {
                initial_claim = None;
                if self.sources.iter().all(|source| !source.is_alive()) {
                    set_owner_state(&shared, OwnerState::NoLongerOwner);
                } else {
                    set_owner_state(&shared, OwnerState::Confirmed);
                }
            }

            if self.sources.iter().all(|source| !source.is_alive()) {
                set_owner_state(&shared, OwnerState::NoLongerOwner);
                return self.finish_transfer();
            }

            match commands.try_recv() {
                Ok(OwnerCommand::Reconcile {
                    deadline,
                    cancelled,
                }) => {
                    let outcome = self.reconcile_owner(deadline, &cancelled, &lifecycle_cancelled);
                    match &outcome {
                        CopyClaimOutcome::Confirmed => {
                            set_owner_state(&shared, OwnerState::Confirmed)
                        }
                        CopyClaimOutcome::ConclusiveNoLongerOwner => {
                            set_owner_state(&shared, OwnerState::NoLongerOwner)
                        }
                        CopyClaimOutcome::FailedBeforeClaim(error) => {
                            set_owner_state(&shared, OwnerState::FailedBeforeClaim(error.clone()))
                        }
                        CopyClaimOutcome::Unconfirmed(error) => {
                            set_owner_state(&shared, OwnerState::Unconfirmed(error.clone()))
                        }
                    }
                    let (state, changed) = &*shared;
                    let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
                    state.reconciliation_pending = false;
                    state.reconciliation_result = Some(outcome);
                    changed.notify_all();
                    continue;
                }
                Err(TryRecvError::Disconnected) => {
                    set_owner_state(&shared, OwnerState::Stopped);
                    return self.finish_transfer();
                }
                Err(TryRecvError::Empty) => {}
            }

            self.dispatch_until_change_or_cancel()?;
        }
    }

    fn reconcile_owner(
        &mut self,
        deadline: Instant,
        operation_cancelled: &AtomicBool,
        lifecycle_cancelled: &AtomicBool,
    ) -> CopyClaimOutcome {
        if operation_cancelled.load(Ordering::Acquire) {
            return CopyClaimOutcome::Unconfirmed(
                "Wayland clipboard owner reconciliation was cancelled".to_owned(),
            );
        }
        let pending = match common::begin_roundtrip(
            &self.connection,
            &self.queue,
            deadline,
            lifecycle_cancelled,
        ) {
            Ok(pending) => pending,
            Err(error) => return CopyClaimOutcome::Unconfirmed(error.to_string()),
        };
        match common::wait_roundtrip_cancellable_when(
            &mut self.queue,
            &mut self.state,
            &pending,
            deadline,
            || {
                operation_cancelled.load(Ordering::Acquire)
                    || lifecycle_cancelled.load(Ordering::Acquire)
            },
        ) {
            Ok(()) if self.sources.iter().any(|source| source.is_alive()) => {
                CopyClaimOutcome::Confirmed
            }
            Ok(()) => CopyClaimOutcome::ConclusiveNoLongerOwner,
            Err(error) => CopyClaimOutcome::Unconfirmed(error.to_string()),
        }
    }
}

fn set_owner_state(shared: &Arc<(Mutex<OwnerShared>, Condvar)>, new_state: OwnerState) {
    let (state, changed) = &**shared;
    state
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .state = new_state;
    changed.notify_all();
}

fn make_source(
    source: Source,
    mime_type: MimeType,
    trim_newline: bool,
) -> Result<DataSource, SourceCreationError> {
    let mut output_place = if let Source::Bytes(data) = source {
        data.into_vec()
    } else {
        let mut contents = Cursor::new(Vec::new());
        io::copy(&mut io::stdin(), &mut contents).map_err(SourceCreationError::DataCopy)?;
        contents.into_inner()
    };

    let mime_type = match mime_type {
        MimeType::Autodetect => tree_magic_mini::from_u8(&output_place).to_string(),
        MimeType::Text => TEXT_PLAIN_MIME.to_string(),
        MimeType::Specific(mime_type) => mime_type,
    };
    log::trace!("Base MIME type: {}", mime_type);

    // Trim the trailing newline if needed.
    if trim_newline && is_text(&mime_type) && output_place.last().copied() == Some(b'\n') {
        output_place.pop();
    }

    Ok(DataSource {
        mime_type,
        source: DataSourceStorage(Arc::from(output_place)),
    })
}

fn get_devices(
    primary: bool,
    seat: Seat,
    socket_name: Option<OsString>,
    operation: Option<(Instant, Arc<AtomicBool>)>,
) -> Result<
    (
        wayland_client::Connection,
        EventQueue<State>,
        State,
        Vec<data_control::Device>,
    ),
    Error,
> {
    let (connection, mut queue, mut common) = if let Some((deadline, cancelled)) = &operation {
        initialize_cancellable(primary, socket_name, *deadline, cancelled)?
    } else {
        initialize(primary, socket_name)?
    };

    // Check if there are no seats.
    if common.seats.is_empty() {
        return Err(Error::NoSeats);
    }

    // Go through the seats and get their data devices.
    for (seat, data) in &mut common.seats {
        let device = common
            .clipboard_manager
            .get_data_device(seat, &queue.handle(), seat.clone());
        data.set_device(Some(device));
    }

    let mut state = State {
        common: Some(common),
        registry_globals: Vec::new(),
        got_primary_selection: false,
        should_quit: false,
        data_sources: HashMap::new(),
        serve_requests: ServeRequests::default(),
        owned_cancellation: None,
        error: None,
    };

    // Retrieve all seat names.
    if let Some((deadline, cancelled)) = &operation {
        common::roundtrip_cancellable(&connection, &mut queue, &mut state, *deadline, cancelled)
            .map_err(map_roundtrip_error)?;
    } else {
        queue
            .roundtrip(&mut state)
            .map_err(Error::WaylandCommunication)?;
    }

    // Check if the compositor supports primary selection.
    if primary && !state.got_primary_selection {
        return Err(Error::PrimarySelectionUnsupported);
    }

    // Figure out which devices we're interested in.
    let devices = state
        .common
        .as_ref()
        .expect("clipboard devices were initialized")
        .seats
        .values()
        .filter_map(|data| {
            let SeatData { name, device, .. } = data;

            let device = device.clone();

            match seat {
                Seat::All => {
                    // If no seat was specified, handle all of them.
                    return device;
                }
                Seat::Specific(ref desired_name) => {
                    if name.as_deref() == Some(desired_name) {
                        return device;
                    }
                }
            }

            None
        })
        .collect::<Vec<_>>();

    // If we didn't find the seat, print an error message and exit.
    //
    // This also triggers when we found the seat but it had no data device; is this what we want?
    if devices.is_empty() {
        return Err(Error::SeatNotFound);
    }

    Ok((connection, queue, state, devices))
}

fn initialize_cancellable(
    primary: bool,
    socket_name: Option<OsString>,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<(wayland_client::Connection, EventQueue<State>, common::State), Error> {
    let connection = common::connect_wayland(socket_name)?;
    let mut queue = connection.new_event_queue();
    let registry = connection.display().get_registry(&queue.handle(), ());
    let mut state = State {
        common: None,
        registry_globals: Vec::new(),
        got_primary_selection: false,
        should_quit: false,
        data_sources: HashMap::new(),
        serve_requests: ServeRequests::default(),
        owned_cancellation: None,
        error: None,
    };

    common::roundtrip_cancellable(&connection, &mut queue, &mut state, deadline, cancelled)
        .map_err(map_roundtrip_error)?;
    let common = common::initialize_from_globals(
        primary,
        &registry,
        &state.registry_globals,
        &queue.handle(),
    )?;
    Ok((connection, queue, common))
}

fn map_roundtrip_error(error: common::RoundtripError) -> Error {
    match error {
        common::RoundtripError::Cancelled => {
            Error::WaylandCommunication(DispatchError::Backend(WaylandError::Io(io::Error::other(
                "Wayland clipboard operation was cancelled",
            ))))
        }
        common::RoundtripError::DeadlineExceeded => {
            Error::WaylandCommunication(DispatchError::Backend(WaylandError::Io(io::Error::new(
                io::ErrorKind::TimedOut,
                "Wayland clipboard deadline exceeded",
            ))))
        }
        common::RoundtripError::Communication(error) => Error::WaylandCommunication(error),
        common::RoundtripError::Poll(error) => {
            Error::WaylandCommunication(DispatchError::Backend(WaylandError::Io(error)))
        }
    }
}

/// Clears the clipboard for the given seat.
///
/// If `seat` is `None`, clears clipboards of all existing seats.
///
/// # Examples
///
/// ```no_run
/// # extern crate wl_clipboard_rs;
/// # use wl_clipboard_rs::copy::Error;
/// # fn foo() -> Result<(), Error> {
/// use wl_clipboard_rs::copy::{clear, ClipboardType, Seat};
///
/// clear(ClipboardType::Regular, Seat::All)?;
/// # Ok(())
/// # }
/// ```
#[inline]
pub fn clear(clipboard: ClipboardType, seat: Seat) -> Result<(), Error> {
    clear_internal(clipboard, seat, None)
}

pub(crate) fn clear_internal(
    clipboard: ClipboardType,
    seat: Seat,
    socket_name: Option<OsString>,
) -> Result<(), Error> {
    let primary = clipboard != ClipboardType::Regular;
    let (_connection, mut queue, mut state, devices) =
        get_devices(primary, seat, socket_name, None)?;

    for device in devices {
        if clipboard == ClipboardType::Primary || clipboard == ClipboardType::Both {
            device.set_primary_selection(None);
        }
        if clipboard == ClipboardType::Regular || clipboard == ClipboardType::Both {
            device.set_selection(None);
        }
    }

    // We're clearing the clipboard so just do one roundtrip and quit.
    queue
        .roundtrip(&mut state)
        .map_err(Error::WaylandCommunication)?;

    Ok(())
}

/// Prepares a data copy to the clipboard.
///
/// The data is copied from `source` and offered in the `mime_type` MIME type. See `Options` for
/// customizing the behavior of this operation.
///
/// This function can be used instead of `copy()` when it's desirable to separately prepare the
/// copy operation, handle any errors that this may produce, and then start the serving loop,
/// potentially past a fork (which is how `wl-copy` uses it). It is meant to be used in the
/// foreground mode and does not spawn any threads.
///
/// # Panics
///
/// Panics if `foreground` is `false`.
///
/// # Examples
///
/// ```no_run
/// # extern crate wl_clipboard_rs;
/// # use wl_clipboard_rs::copy::Error;
/// # fn foo() -> Result<(), Error> {
/// use wl_clipboard_rs::copy::{MimeSource, MimeType, Options, Source};
///
/// let mut opts = Options::new();
/// opts.foreground(true);
/// let prepared_copy =
///     opts.prepare_copy(Source::Bytes([1, 2, 3][..].into()), MimeType::Autodetect)?;
/// prepared_copy.serve()?;
///
/// # Ok(())
/// # }
/// ```
#[inline]
pub fn prepare_copy(
    options: Options,
    source: Source,
    mime_type: MimeType,
) -> Result<PreparedCopy, Error> {
    assert!(options.foreground);

    let sources = vec![MimeSource { source, mime_type }];

    prepare_copy_internal(options, sources, None, None)
}

/// Prepares a data copy to the clipboard, offering multiple data sources.
///
/// The data from each source in `sources` is copied and offered in the corresponding MIME type.
/// See `Options` for customizing the behavior of this operation.
///
/// If multiple sources specify the same MIME type, the first one is offered. If one of the MIME
/// types is text, all automatically added plain text offers will fall back to the first source
/// with a text MIME type.
///
/// This function can be used instead of `copy()` when it's desirable to separately prepare the
/// copy operation, handle any errors that this may produce, and then start the serving loop,
/// potentially past a fork (which is how `wl-copy` uses it). It is meant to be used in the
/// foreground mode and does not spawn any threads.
///
/// # Panics
///
/// Panics if `foreground` is `false`.
///
/// # Examples
///
/// ```no_run
/// # extern crate wl_clipboard_rs;
/// # use wl_clipboard_rs::copy::Error;
/// # fn foo() -> Result<(), Error> {
/// use wl_clipboard_rs::copy::{MimeSource, MimeType, Options, Source};
///
/// let mut opts = Options::new();
/// opts.foreground(true);
/// let prepared_copy = opts.prepare_copy_multi(vec![
///     MimeSource {
///         source: Source::Bytes([1, 2, 3][..].into()),
///         mime_type: MimeType::Autodetect,
///     },
///     MimeSource {
///         source: Source::Bytes([7, 8, 9][..].into()),
///         mime_type: MimeType::Text,
///     },
/// ])?;
/// prepared_copy.serve()?;
///
/// # Ok(())
/// # }
/// ```
#[inline]
pub fn prepare_copy_multi(
    options: Options,
    sources: Vec<MimeSource>,
) -> Result<PreparedCopy, Error> {
    assert!(options.foreground);

    prepare_copy_internal(options, sources, None, None)
}

fn prepare_copy_internal(
    options: Options,
    sources: Vec<MimeSource>,
    socket_name: Option<OsString>,
    operation: Option<(Instant, Arc<AtomicBool>)>,
) -> Result<PreparedCopy, Error> {
    let Options {
        clipboard,
        seat,
        trim_newline,
        serve_requests,
        sensitive,
        ..
    } = options;

    let primary = clipboard != ClipboardType::Regular;
    let (connection, queue, mut state, devices) =
        get_devices(primary, seat, socket_name, operation)?;

    state.serve_requests = serve_requests;

    // Collect the source data to copy.
    state.data_sources = {
        let mut data_sources = HashMap::new();
        let mut text_data = None;

        for MimeSource { source, mime_type } in sources.into_iter() {
            let DataSource { mime_type, source } =
                make_source(source, mime_type, trim_newline).map_err(Error::TempCopy)?;

            match data_sources.entry(mime_type) {
                // This MIME type has already been specified, so ignore it.
                Entry::Occupied(_) => drop(source),
                Entry::Vacant(entry) => {
                    if !options.omit_additional_text_mime_types
                        && text_data.is_none()
                        && is_text(entry.key())
                    {
                        text_data = Some(source.clone());
                    }

                    entry.insert(source);
                }
            }
        }

        // If the MIME type is text, offer it in some other common formats.
        if let Some(text_data) = text_data {
            let text_mimes = [
                "text/plain;charset=utf-8",
                TEXT_PLAIN_MIME,
                "STRING",
                "UTF8_STRING",
                "TEXT",
            ];

            for mime_type in text_mimes {
                // We don't want to overwrite an explicit mime type, because it might be bound to a
                // different data_path
                if !data_sources.contains_key(mime_type) {
                    data_sources.insert(mime_type.to_string(), text_data.clone());
                }
            }
        }

        if sensitive {
            data_sources
                .entry(PASSWORD_MANAGER_HINT_MIME_TYPE.to_owned())
                .or_insert_with(|| DataSourceStorage(Arc::from(&b"secret"[..])));
        }

        data_sources
    };

    // Create an iterator over (device, primary) for source creation later.
    //
    // This is needed because for ClipboardType::Both each device needs to appear twice because
    // separate data sources need to be made for the regular and the primary clipboards (data
    // sources cannot be reused).
    let devices_iter = devices.iter().flat_map(|device| {
        let first = match clipboard {
            ClipboardType::Regular => iter::once((device, false)),
            ClipboardType::Primary => iter::once((device, true)),
            ClipboardType::Both => iter::once((device, false)),
        };

        let second = if clipboard == ClipboardType::Both {
            iter::once(Some((device, true)))
        } else {
            iter::once(None)
        };

        first.chain(second.flatten())
    });

    // Create the data sources and set them as selections.
    let sources = devices_iter
        .map(|(device, primary)| {
            let data_source = state
                .common
                .as_mut()
                .expect("clipboard devices were initialized")
                .clipboard_manager
                .create_data_source(&queue.handle());

            for mime_type in state.data_sources.keys() {
                if mime_type != PASSWORD_MANAGER_HINT_MIME_TYPE {
                    data_source.offer(mime_type.clone());
                }
            }
            // Advertise the hint after the actual contents. Some tools will choose the first
            // offered MIME type as the "best" one, and we don't want that type to be the password
            // manager hint.
            if state
                .data_sources
                .contains_key(PASSWORD_MANAGER_HINT_MIME_TYPE)
            {
                data_source.offer(PASSWORD_MANAGER_HINT_MIME_TYPE.to_owned());
            }

            if primary {
                device.set_primary_selection(Some(&data_source));
            } else {
                device.set_selection(Some(&data_source));
            }

            // If we need to serve 0 requests, kill the data source right away.
            if let ServeRequests::Only(0) = state.serve_requests {
                data_source.destroy();
            }
            data_source
        })
        .collect::<Vec<_>>();

    Ok(PreparedCopy {
        connection,
        queue,
        state,
        sources,
        cancelled: Arc::new(AtomicBool::new(false)),
    })
}

/// Copies data to the clipboard.
///
/// The data is copied from `source` and offered in the `mime_type` MIME type. See `Options` for
/// customizing the behavior of this operation.
///
/// # Examples
///
/// ```no_run
/// # extern crate wl_clipboard_rs;
/// # use wl_clipboard_rs::copy::Error;
/// # fn foo() -> Result<(), Error> {
/// use wl_clipboard_rs::copy::{copy, MimeType, Options, Source};
///
/// let opts = Options::new();
/// copy(
///     opts,
///     Source::Bytes([1, 2, 3][..].into()),
///     MimeType::Autodetect,
/// )?;
/// # Ok(())
/// # }
/// ```
#[inline]
pub fn copy(options: Options, source: Source, mime_type: MimeType) -> Result<(), Error> {
    let sources = vec![MimeSource { source, mime_type }];
    copy_internal(options, sources, None)
}

/// Copies data to the clipboard, offering multiple data sources.
///
/// The data from each source in `sources` is copied and offered in the corresponding MIME type.
/// See `Options` for customizing the behavior of this operation.
///
/// If multiple sources specify the same MIME type, the first one is offered. If one of the MIME
/// types is text, all automatically added plain text offers will fall back to the first source
/// with a text MIME type.
///
/// # Examples
///
/// ```no_run
/// # extern crate wl_clipboard_rs;
/// # use wl_clipboard_rs::copy::Error;
/// # fn foo() -> Result<(), Error> {
/// use wl_clipboard_rs::copy::{MimeSource, MimeType, Options, Source};
///
/// let opts = Options::new();
/// opts.copy_multi(vec![
///     MimeSource {
///         source: Source::Bytes([1, 2, 3][..].into()),
///         mime_type: MimeType::Autodetect,
///     },
///     MimeSource {
///         source: Source::Bytes([7, 8, 9][..].into()),
///         mime_type: MimeType::Text,
///     },
/// ])?;
/// # Ok(())
/// # }
/// ```
#[inline]
pub fn copy_multi(options: Options, sources: Vec<MimeSource>) -> Result<(), Error> {
    copy_internal(options, sources, None)
}

/// Copies multiple MIME sources and retains a handle to the serving owner.
///
/// `spawn` must create a named owner thread and run the supplied body there.
/// Preparation, selection requests, roundtrip confirmation, and serving all
/// remain on that thread because `PreparedCopy` is not `Send`.
pub fn copy_multi_owned_until<F>(
    options: Options,
    sources: Vec<MimeSource>,
    deadline: Instant,
    spawn: F,
) -> Result<CopyHandle, Error>
where
    F: FnOnce(
        Box<dyn FnOnce() -> Result<(), Error> + Send + 'static>,
    ) -> io::Result<thread::JoinHandle<Result<(), Error>>>,
{
    start_copy_multi_owned(options, sources, None, deadline, spawn)
}

#[cfg(test)]
pub(crate) fn copy_multi_owned_with_socket(
    options: Options,
    sources: Vec<MimeSource>,
    socket_name: OsString,
) -> Result<CopyHandle, Error> {
    let deadline = Instant::now() + Duration::from_secs(4);
    start_copy_multi_owned(options, sources, Some(socket_name), deadline, |body| {
        thread::Builder::new()
            .name("wl-owned-copy-test".to_owned())
            .spawn(body)
    })
}

fn start_copy_multi_owned<F>(
    options: Options,
    sources: Vec<MimeSource>,
    socket_name: Option<OsString>,
    deadline: Instant,
    spawn: F,
) -> Result<CopyHandle, Error>
where
    F: FnOnce(
        Box<dyn FnOnce() -> Result<(), Error> + Send + 'static>,
    ) -> io::Result<thread::JoinHandle<Result<(), Error>>>,
{
    let cancelled = Arc::new(AtomicBool::new(false));
    let shared = Arc::new((Mutex::new(OwnerShared::new()), Condvar::new()));
    let (commands, command_rx) = sync_channel(1);
    let worker_shared = Arc::clone(&shared);
    let worker_cancelled = Arc::clone(&cancelled);
    let body = Box::new(move || {
        run_copy_owner(
            options,
            sources,
            socket_name,
            deadline,
            worker_cancelled,
            worker_shared,
            command_rx,
        )
    });
    let worker = spawn(body).map_err(|error| {
        Error::WaylandCommunication(DispatchError::Backend(WaylandError::Io(error)))
    })?;

    Ok(CopyHandle {
        cancel: CopyCancelHandle(cancelled),
        shared,
        commands,
        worker: Some(worker),
    })
}

fn run_copy_owner(
    options: Options,
    sources: Vec<MimeSource>,
    socket_name: Option<OsString>,
    deadline: Instant,
    lifecycle_cancelled: Arc<AtomicBool>,
    shared: Arc<(Mutex<OwnerShared>, Condvar)>,
    commands: Receiver<OwnerCommand>,
) -> Result<(), Error> {
    let mut prepared = match prepare_copy_internal(
        options,
        sources,
        socket_name,
        Some((deadline, Arc::clone(&lifecycle_cancelled))),
    ) {
        Ok(prepared) => prepared,
        Err(error) => {
            set_owner_state(&shared, OwnerState::FailedBeforeClaim(error.to_string()));
            return Ok(());
        }
    };

    // This flush sends the selection request. From this point onward, an
    // error or timeout is uncertain until an ordered same-connection barrier.
    if let Err(error) = prepared.queue.flush() {
        set_owner_state(
            &shared,
            OwnerState::Unconfirmed(format!("Wayland clipboard claim flush failed: {error}")),
        );
        return prepared.serve_owned(None, shared, commands, lifecycle_cancelled);
    }
    set_owner_state(&shared, OwnerState::ClaimSent);

    let pending = match common::begin_roundtrip(
        &prepared.connection,
        &prepared.queue,
        deadline,
        &lifecycle_cancelled,
    ) {
        Ok(pending) => Some(pending),
        Err(error) => {
            set_owner_state(&shared, OwnerState::Unconfirmed(error.to_string()));
            None
        }
    };
    if let Some(pending) = pending.as_ref() {
        match common::wait_roundtrip_cancellable(
            &mut prepared.queue,
            &mut prepared.state,
            pending,
            deadline,
            &lifecycle_cancelled,
        ) {
            Ok(()) if prepared.sources.iter().any(|source| source.is_alive()) => {
                set_owner_state(&shared, OwnerState::Confirmed);
            }
            Ok(()) => set_owner_state(&shared, OwnerState::NoLongerOwner),
            Err(error) => set_owner_state(&shared, OwnerState::Unconfirmed(error.to_string())),
        }
    }

    let result = prepared.serve_owned(pending, shared.clone(), commands, lifecycle_cancelled);
    if let Err(error) = &result {
        let (state, changed) = &*shared;
        let mut state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !matches!(state.state, OwnerState::NoLongerOwner | OwnerState::Stopped) {
            state.state = OwnerState::Unconfirmed(error.to_string());
        }
        changed.notify_all();
    }
    result
}

pub(crate) fn copy_internal(
    options: Options,
    sources: Vec<MimeSource>,
    socket_name: Option<OsString>,
) -> Result<(), Error> {
    if options.foreground {
        prepare_copy_internal(options, sources, socket_name, None)?.serve()
    } else {
        // The copy must be prepared on the thread because PreparedCopy isn't Send.
        // To receive errors from prepare_copy, use a channel.
        let (tx, rx) = sync_channel(1);

        thread::spawn(
            move || match prepare_copy_internal(options, sources, socket_name, None) {
                Ok(prepared_copy) => {
                    // prepare_copy completed successfully, report that.
                    drop(tx.send(None));

                    // There's nobody listening for errors at this point, just drop it.
                    drop(prepared_copy.serve());
                }
                Err(err) => drop(tx.send(Some(err))),
            },
        );

        if let Some(err) = rx.recv().unwrap() {
            return Err(err);
        }

        Ok(())
    }
}
