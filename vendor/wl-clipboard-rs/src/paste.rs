//! Getting the offered MIME types and the clipboard contents once with [`get_contents`].
//!
//! To watch for selection changes continuously instead, see [`crate::watch`].
//!
//! FOLIO PATCH: Adds a cancellable, deadline-bounded `OfferSession` that keeps one selected
//! Wayland offer across MIME fallback. MODIFIED BY THE FOLIO CONTRIBUTORS; see
//! `vendor/wl-clipboard-rs/CHANGES-FOLIO.md`.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io::{self, Read};
use std::os::fd::AsFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use os_pipe::{pipe, PipeReader};
use rustix::event::{poll, PollFd, PollFlags, Timespec};
use wayland_client::globals::{Global, GlobalListContents};
use wayland_client::protocol::wl_callback::WlCallback;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{
    delegate_dispatch, event_created_child, ConnectError, Connection, Dispatch, DispatchError,
    EventQueue, Proxy,
};
use wayland_protocols::ext::data_control::v1::client::ext_data_control_manager_v1::ExtDataControlManagerV1;
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_manager_v1::ZwlrDataControlManagerV1;

use crate::common::{self, initialize};
use crate::data_control::{
    self, impl_dispatch_device, impl_dispatch_manager, impl_dispatch_offer, Manager,
};
use crate::seat_data::SeatData;
use crate::utils::{is_text, PASSWORD_MANAGER_HINT_MIME_TYPE};

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
}

/// MIME types that can be requested from the clipboard.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash, PartialOrd, Ord)]
pub enum MimeType<'a> {
    /// Request any available MIME type.
    ///
    /// If multiple MIME types are offered, the requested MIME type is unspecified and depends on
    /// the order they are received from the Wayland compositor. However, plain text formats are
    /// prioritized, so if a plain text format is available among others then it will be requested.
    Any,
    /// Request a plain text MIME type.
    ///
    /// This will request one of the multiple common plain text MIME types. It will prioritize MIME
    /// types known to return UTF-8 text.
    Text,
    /// Request the given MIME type, and if it's not available fall back to `MimeType::Text`.
    ///
    /// Example use-case: pasting `text/html` should try `text/html` first, but if it's not
    /// available, any other plain text format will do fine too.
    TextWithPriority(&'a str),
    /// Request a specific MIME type.
    Specific(&'a str),
}

/// Seat to operate on.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash, PartialOrd, Ord, Default)]
pub enum Seat<'a> {
    /// Operate on one of the existing seats depending on the order returned by the compositor.
    ///
    /// This is perfectly fine when only a single seat is present, so for most configurations.
    #[default]
    Unspecified,
    /// Operate on a seat with the given name.
    Specific(&'a str),
}

struct State {
    common: Option<common::State>,
    offers: HashMap<data_control::Offer, Vec<String>>,
    got_primary_selection: bool,
    registry_globals: Vec<Global>,
    mime_filter: Option<HashSet<String>>,
}

delegate_dispatch!(State: [WlSeat: ()] => common::State);

impl AsMut<common::State> for State {
    fn as_mut(&mut self) -> &mut common::State {
        self.common
            .as_mut()
            .expect("clipboard event dispatch starts after registry setup")
    }
}

/// Errors that can occur for pasting and listing MIME types.
///
/// You may want to ignore some of these errors (rather than show an error message), like
/// `NoSeats`, `ClipboardEmpty` or `NoMimeType` as they are essentially equivalent to an empty
/// clipboard.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("There are no seats")]
    NoSeats,

    #[error("The clipboard of the requested seat is empty")]
    ClipboardEmpty,

    #[error("No suitable type of content copied")]
    NoMimeType,

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

    #[error("Couldn't create a pipe for content transfer")]
    PipeCreation(#[source] io::Error),

    #[error("clipboard read was cancelled")]
    Cancelled,

    #[error("clipboard read exceeded its deadline")]
    DeadlineExceeded,

    #[error("clipboard contents exceeded the requested byte limit")]
    ContentTooLarge,

    #[error("clipboard transfer failed: {0}")]
    Transfer(String),
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

impl Dispatch<WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        _proxy: &WlRegistry,
        event: <WlRegistry as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
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

impl_dispatch_manager!(State);

impl_dispatch_device!(State, WlSeat, |state: &mut Self, event, seat: &WlSeat| {
    match event {
        Event::DataOffer { id } => {
            let offer = data_control::Offer::from(id);
            state.offers.insert(offer, Vec::new());
        }
        Event::Selection { id } => {
            let offer = id.map(data_control::Offer::from);
            let seat = state.as_mut().seats.get_mut(seat).unwrap();
            seat.set_offer(offer);
        }
        Event::Finished => {
            // Destroy the device stored in the seat as it's no longer valid.
            let seat = state.as_mut().seats.get_mut(seat).unwrap();
            seat.set_device(None);
        }
        Event::PrimarySelection { id } => {
            let offer = id.map(data_control::Offer::from);
            state.got_primary_selection = true;
            let seat = state.as_mut().seats.get_mut(seat).unwrap();
            seat.set_primary_offer(offer);
        }
        _ => (),
    }
});

impl_dispatch_offer!(State, |state: &mut Self,
                             offer: data_control::Offer,
                             event| {
    if let Event::Offer { mime_type } = event {
        let Some(mime_types) = state.offers.get_mut(&offer) else {
            return;
        };
        let filtered = state.mime_filter.as_ref();
        if filtered.is_none_or(|filter| filter.contains(&mime_type))
            && (filtered.is_none() || !mime_types.contains(&mime_type))
        {
            mime_types.push(mime_type);
        }
    }
});

fn get_offer(
    primary: bool,
    seat: Seat<'_>,
    socket_name: Option<OsString>,
) -> Result<(EventQueue<State>, State, data_control::Offer), Error> {
    let (_, mut queue, mut common) = initialize(primary, socket_name)?;

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
        offers: HashMap::new(),
        got_primary_selection: false,
        registry_globals: Vec::new(),
        mime_filter: None,
    };

    // Retrieve all seat names and offers.
    queue
        .roundtrip(&mut state)
        .map_err(Error::WaylandCommunication)?;

    // Check if the compositor supports primary selection.
    if primary && !state.got_primary_selection {
        return Err(Error::PrimarySelectionUnsupported);
    }

    // Figure out which offer we're interested in.
    let data = match seat {
        Seat::Unspecified => state.as_mut().seats.values().next(),
        Seat::Specific(name) => state
            .as_mut()
            .seats
            .values()
            .find(|data| data.name.as_deref() == Some(name)),
    };

    let Some(data) = data else {
        return Err(Error::SeatNotFound);
    };

    let offer = if primary {
        &data.primary_offer
    } else {
        &data.offer
    };

    // Check if we found anything.
    match offer.clone() {
        Some(offer) => Ok((queue, state, offer)),
        None => Err(Error::ClipboardEmpty),
    }
}

const CANCELLATION_POLL: Duration = Duration::from_millis(20);
const PIPE_BUFFER_BYTES: usize = 16 * 1024;

impl Dispatch<WlCallback, common::SyncTicket> for State {
    fn event(
        _state: &mut Self,
        _callback: &WlCallback,
        event: <WlCallback as Proxy>::Event,
        ticket: &common::SyncTicket,
        _connection: &Connection,
        _queue: &wayland_client::QueueHandle<Self>,
    ) {
        if matches!(
            event,
            wayland_client::protocol::wl_callback::Event::Done { .. }
        ) {
            ticket.0.store(true, Ordering::Release);
        }
    }
}

fn check_control(deadline: Instant, cancelled: &AtomicBool) -> Result<(), Error> {
    if cancelled.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(Error::DeadlineExceeded);
    }
    Ok(())
}

fn poll_readable(fd: impl AsFd, deadline: Instant, cancelled: &AtomicBool) -> Result<bool, Error> {
    check_control(deadline, cancelled)?;
    let remaining = deadline.saturating_duration_since(Instant::now());
    let timeout = Timespec::try_from(remaining.min(CANCELLATION_POLL))
        .map_err(|error| Error::Transfer(error.to_string()))?;
    let mut fds = [PollFd::new(&fd, PollFlags::IN)];
    poll(&mut fds, Some(&timeout)).map_err(|error| Error::Transfer(error.to_string()))?;
    check_control(deadline, cancelled)?;
    let events = fds[0].revents();
    Ok(events.intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR))
}

fn roundtrip_cancellable(
    connection: &Connection,
    queue: &mut EventQueue<State>,
    state: &mut State,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<(), Error> {
    common::roundtrip_cancellable(connection, queue, state, deadline, cancelled).map_err(|error| {
        match error {
            common::RoundtripError::Cancelled => Error::Cancelled,
            common::RoundtripError::DeadlineExceeded => Error::DeadlineExceeded,
            common::RoundtripError::Communication(error) => Error::WaylandCommunication(error),
            common::RoundtripError::Poll(error) => Error::Transfer(error.to_string()),
        }
    })
}

fn connect_wayland(socket_name: Option<OsString>) -> Result<Connection, Error> {
    common::connect_wayland(socket_name).map_err(Error::from)
}

fn cancellable_offer(
    primary: bool,
    seat: Seat<'_>,
    socket_name: Option<OsString>,
    supported_mimes: &[&str],
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<
    (
        Connection,
        EventQueue<State>,
        State,
        data_control::Offer,
        Vec<String>,
    ),
    Error,
> {
    check_control(deadline, cancelled)?;
    let connection = connect_wayland(socket_name)?;
    check_control(deadline, cancelled)?;
    let mut queue = connection.new_event_queue::<State>();
    let mut state = State {
        common: None,
        offers: HashMap::new(),
        got_primary_selection: false,
        registry_globals: Vec::new(),
        mime_filter: Some(
            supported_mimes
                .iter()
                .map(|mime| (*mime).to_owned())
                .collect(),
        ),
    };
    let queue_handle = queue.handle();
    let registry = connection.display().get_registry(&queue_handle, ());
    roundtrip_cancellable(&connection, &mut queue, &mut state, deadline, cancelled)?;

    let globals = &state.registry_globals;
    let ext_name = globals
        .iter()
        .find(|global| {
            global.interface == ExtDataControlManagerV1::interface().name && global.version >= 1
        })
        .map(|global| global.name);
    let wlr_version = if primary { 2 } else { 1 };
    let wlr_name = globals
        .iter()
        .find(|global| {
            global.interface == ZwlrDataControlManagerV1::interface().name
                && global.version >= wlr_version
        })
        .map(|global| global.name);

    let clipboard_manager = if let Some(name) = ext_name {
        Manager::Ext(registry.bind(name, 1, &queue_handle, ()))
    } else if let Some(name) = wlr_name {
        Manager::Zwlr(registry.bind(name, wlr_version, &queue_handle, ()))
    } else {
        return Err(Error::MissingProtocol {
            name: "ext-data-control, or wlr-data-control",
            version: wlr_version,
        });
    };

    let seats: HashMap<WlSeat, SeatData> = state
        .registry_globals
        .iter()
        .filter(|global| global.interface == WlSeat::interface().name && global.version >= 2)
        .map(|global| {
            let seat = registry.bind(global.name, 2, &queue_handle, ());
            (seat, SeatData::default())
        })
        .collect();
    if seats.is_empty() {
        return Err(Error::NoSeats);
    }

    state.common = Some(common::State {
        seats,
        clipboard_manager,
    });
    {
        let common = state.as_mut();
        for (seat, data) in &mut common.seats {
            let device =
                common
                    .clipboard_manager
                    .get_data_device(seat, &queue_handle, seat.clone());
            data.set_device(Some(device));
        }
    }
    roundtrip_cancellable(&connection, &mut queue, &mut state, deadline, cancelled)?;

    if primary && !state.got_primary_selection {
        return Err(Error::PrimarySelectionUnsupported);
    }
    let common = state.as_mut();
    let data = match seat {
        Seat::Unspecified => common.seats.values_mut().next(),
        Seat::Specific(name) => common
            .seats
            .values_mut()
            .find(|data| data.name.as_deref() == Some(name)),
    }
    .ok_or(Error::SeatNotFound)?;
    let offer = if primary {
        data.primary_offer.take()
    } else {
        data.offer.take()
    }
    .ok_or(Error::ClipboardEmpty)?;
    let mime_types = state.offers.remove(&offer).unwrap_or_default();
    Ok((connection, queue, state, offer, mime_types))
}

/// One Wayland offer and seat, retained while successive MIME rungs are read.
///
/// Unlike [`get_contents`], this session does not select a fresh offer for every rung. It also
/// bounds reads and makes its event waits cancellable by the caller's absolute deadline.
pub struct OfferSession<'a> {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
    offer: Option<data_control::Offer>,
    mime_types: Vec<String>,
    deadline: Instant,
    cancelled: &'a AtomicBool,
}

impl<'a> OfferSession<'a> {
    /// Select one offer, retaining only MIME types in `supported_mimes`.
    pub fn open_for_mimes(
        clipboard: ClipboardType,
        seat: Seat<'_>,
        supported_mimes: &[&str],
        deadline: Instant,
        cancelled: &'a AtomicBool,
    ) -> Result<Self, Error> {
        Self::open_for_mimes_internal(clipboard, seat, supported_mimes, deadline, cancelled, None)
    }

    pub(crate) fn open_for_mimes_internal(
        clipboard: ClipboardType,
        seat: Seat<'_>,
        supported_mimes: &[&str],
        deadline: Instant,
        cancelled: &'a AtomicBool,
        socket_name: Option<OsString>,
    ) -> Result<Self, Error> {
        let (connection, queue, state, offer, mime_types) = cancellable_offer(
            clipboard == ClipboardType::Primary,
            seat,
            socket_name,
            supported_mimes,
            deadline,
            cancelled,
        )?;
        Ok(Self {
            connection,
            queue,
            state,
            offer: Some(offer),
            mime_types,
            deadline,
            cancelled,
        })
    }

    /// The selected offer's supported MIME types in compositor order.
    #[must_use]
    pub fn mime_types(&self) -> &[String] {
        &self.mime_types
    }

    /// Read one MIME type completely while keeping this offer alive.
    pub fn read_contents(
        &mut self,
        mime_type: &str,
        max_bytes: usize,
    ) -> Result<(Vec<u8>, String), Error> {
        check_control(self.deadline, self.cancelled)?;
        if !self.mime_types.iter().any(|offered| offered == mime_type) {
            return Err(Error::NoMimeType);
        }
        let (mut reader, write) = pipe().map_err(Error::PipeCreation)?;
        self.offer
            .as_ref()
            .expect("an open session owns its selected offer")
            .receive(mime_type.to_owned(), write.as_fd());
        drop(write);
        roundtrip_cancellable(
            &self.connection,
            &mut self.queue,
            &mut self.state,
            self.deadline,
            self.cancelled,
        )?;
        let bytes = read_pipe_bounded(&mut reader, max_bytes, self.deadline, self.cancelled)?;
        Ok((bytes, mime_type.to_owned()))
    }
}

impl Drop for OfferSession<'_> {
    fn drop(&mut self) {
        if let Some(offer) = self.offer.take() {
            offer.destroy();
            let _ = self.connection.flush();
        }
    }
}

fn read_pipe_bounded(
    reader: &mut PipeReader,
    max_bytes: usize,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    let mut buffer = [0; PIPE_BUFFER_BYTES];
    loop {
        check_control(deadline, cancelled)?;
        let remaining = max_bytes.saturating_sub(bytes.len());
        let ready = poll_readable(&mut *reader, deadline, cancelled)?;
        if !ready {
            continue;
        }
        let buffer_len = if remaining == 0 {
            1
        } else {
            remaining.min(buffer.len())
        };
        let count = reader
            .read(&mut buffer[..buffer_len])
            .map_err(|error| Error::Transfer(error.to_string()))?;
        if count == 0 {
            return Ok(bytes);
        }
        if remaining == 0 {
            return Err(Error::ContentTooLarge);
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
}

/// Retrieves the offered MIME types.
///
/// Also see [`get_mime_types_ordered()`], an order-preserving version.
///
/// If `seat` is `None`, uses an unspecified seat (it depends on the order returned by the
/// compositor). This is perfectly fine when only a single seat is present, so for most
/// configurations.
///
/// # Examples
///
/// ```no_run
/// # extern crate wl_clipboard_rs;
/// # use wl_clipboard_rs::paste::Error;
/// # fn foo() -> Result<(), Error> {
/// use wl_clipboard_rs::paste::{get_mime_types, ClipboardType, Seat};
///
/// let mime_types = get_mime_types(ClipboardType::Regular, Seat::Unspecified)?;
/// for mime_type in mime_types {
///     println!("{}", mime_type);
/// }
/// # Ok(())
/// # }
/// ```
#[inline]
pub fn get_mime_types(clipboard: ClipboardType, seat: Seat<'_>) -> Result<HashSet<String>, Error> {
    Ok(get_mime_types_internal(clipboard, seat, None)?
        .into_iter()
        .collect())
}

/// Retrieves the offered MIME types, preserving their original order.
///
/// Applications are generally expected to offer not just the "native" data type, but some
/// conversions generated on the fly. For example, when copying a PNG image from a browser, it will
/// offer `image/png` as well as `image/jpeg`, `image/webp`, and others, to maximize compatibility.
/// When these converted MIME types are pasted, the application will generate the data on the fly
/// (by converting the image to the requested MIME type).
///
/// There's no defined way to know which of the offered MIME types is native (if any). However,
/// some applications will offer the native data types first, followed by converted ones. While
/// [`get_mime_types()`] loses this order (a `HashSet` is unordered), this function returns the
/// MIME types in their original order.
///
/// If `seat` is `None`, uses an unspecified seat (it depends on the order returned by the
/// compositor). This is perfectly fine when only a single seat is present, so for most
/// configurations.
///
/// # Examples
///
/// ```no_run
/// # extern crate wl_clipboard_rs;
/// # use wl_clipboard_rs::paste::Error;
/// # fn foo() -> Result<(), Error> {
/// use wl_clipboard_rs::paste::{get_mime_types_ordered, ClipboardType, Seat};
///
/// let mime_types = get_mime_types_ordered(ClipboardType::Regular, Seat::Unspecified)?;
/// for mime_type in mime_types {
///     println!("{}", mime_type);
/// }
/// # Ok(())
/// # }
/// ```
#[inline]
pub fn get_mime_types_ordered(
    clipboard: ClipboardType,
    seat: Seat<'_>,
) -> Result<Vec<String>, Error> {
    get_mime_types_internal(clipboard, seat, None)
}

// The internal function accepts the socket name, used for tests.
pub(crate) fn get_mime_types_internal(
    clipboard: ClipboardType,
    seat: Seat<'_>,
    socket_name: Option<OsString>,
) -> Result<Vec<String>, Error> {
    let primary = clipboard == ClipboardType::Primary;
    let (_, mut state, offer) = get_offer(primary, seat, socket_name)?;
    Ok(state.offers.remove(&offer).unwrap())
}

/// Retrieves the clipboard contents.
///
/// This function returns a tuple of the reading end of a pipe containing the clipboard contents
/// and the actual MIME type of the contents.
///
/// If `seat` is `None`, uses an unspecified seat (it depends on the order returned by the
/// compositor). This is perfectly fine when only a single seat is present, so for most
/// configurations.
///
/// # Examples
///
/// ```no_run
/// # extern crate wl_clipboard_rs;
/// # fn foo() -> Result<(), Box<dyn std::error::Error>> {
/// use std::io::Read;
///
/// use wl_clipboard_rs::paste::{get_contents, ClipboardType, Error, MimeType, Seat};
///
/// let result = get_contents(ClipboardType::Regular, Seat::Unspecified, MimeType::Any);
/// match result {
///     Ok((mut pipe, mime_type)) => {
///         println!("Got data of the {} MIME type", &mime_type);
///
///         let mut contents = vec![];
///         pipe.read_to_end(&mut contents)?;
///         println!("Read {} bytes of data", contents.len());
///     }
///
///     Err(Error::NoSeats) | Err(Error::ClipboardEmpty) | Err(Error::NoMimeType) => {
///         // The clipboard is empty, nothing to worry about.
///     }
///
///     Err(err) => Err(err)?,
/// }
/// # Ok(())
/// # }
/// ```
#[inline]
pub fn get_contents(
    clipboard: ClipboardType,
    seat: Seat<'_>,
    mime_type: MimeType<'_>,
) -> Result<(PipeReader, String), Error> {
    get_contents_internal(clipboard, seat, mime_type, None)
}

// The internal function accepts the socket name, used for tests.
pub(crate) fn get_contents_internal(
    clipboard: ClipboardType,
    seat: Seat<'_>,
    mime_type: MimeType<'_>,
    socket_name: Option<OsString>,
) -> Result<(PipeReader, String), Error> {
    let primary = clipboard == ClipboardType::Primary;
    let (mut queue, mut state, offer) = get_offer(primary, seat, socket_name)?;

    let mime_types = state.offers.remove(&offer).unwrap();
    let Some(mime_type) = select_mime_type(mime_types, mime_type) else {
        return Err(Error::NoMimeType);
    };

    // Create a pipe for content transfer.
    let (read, write) = pipe().map_err(Error::PipeCreation)?;

    // Start the transfer.
    offer.receive(mime_type.clone(), write.as_fd());
    drop(write);

    // A flush() is not enough here, it will result in sometimes pasting empty contents. I suspect this is due to a
    // race between the compositor reacting to the receive request, and the compositor reacting to wl-paste
    // disconnecting after queue is dropped. The roundtrip solves that race.
    queue
        .roundtrip(&mut state)
        .map_err(Error::WaylandCommunication)?;

    Ok((read, mime_type))
}

/// Selects the best MIME type from `available` according to `requested`.
///
/// When text types are available, these will generally be preferred. See
/// [`MimeType`] for details. Returns the chosen type, or `None` if none of the
/// available types satisfy the request.
pub fn select_mime_type(available: Vec<String>, requested: MimeType<'_>) -> Option<String> {
    let mut v = available;

    macro_rules! take {
        ($pred:expr) => {
            'block: {
                for i in 0..v.len() {
                    if $pred(&v[i]) {
                        // We only remove once, so the swap doesn't affect anything.
                        break 'block Some(v.swap_remove(i));
                    }
                }
                None
            }
        };
    }

    match requested {
        MimeType::Any => take!(|x| x == "text/plain;charset=utf-8")
            .or_else(|| take!(|x| x == "UTF8_STRING"))
            .or_else(|| take!(is_text))
            // Only consider the password-manager hint if no other MIME type is offered.
            .or_else(|| take!(|x| x != PASSWORD_MANAGER_HINT_MIME_TYPE))
            .or_else(|| take!(|_| true)),
        MimeType::Text => take!(|x| x == "text/plain;charset=utf-8")
            .or_else(|| take!(|x| x == "UTF8_STRING"))
            .or_else(|| take!(is_text)),
        MimeType::TextWithPriority(priority) => take!(|x: &String| x == priority)
            .or_else(|| take!(|x| x == "text/plain;charset=utf-8"))
            .or_else(|| take!(|x| x == "UTF8_STRING"))
            .or_else(|| take!(is_text)),
        MimeType::Specific(mime_type) => take!(|x| x == mime_type),
    }
}
