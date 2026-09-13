//! **The application's own lifecycle — reopen, opened paths, termination, the
//! last window closing** (ticket M3-1, probe X-4).
//!
//! # What this door is, and what it is not
//!
//! Every other door in this crate is about *a window*: where it is, what it
//! wears, what the reader just chose in it. This one is about **the process**,
//! and the four things it carries are the four questions macOS asks an
//! application rather than a window:
//!
//! * a **reopen** — a second Finder launch, a Dock click, `open -a Folio` from
//!   a terminal. It is not a second process: LaunchServices activates the one
//!   that is running and tells it somebody asked for it again;
//! * **paths to open** — `open -a Folio <file>`, a document dropped on the Dock
//!   tile, and (M4-9) a Service;
//! * a **termination request** — `⌘Q`, the Dock's *Quit*, a logout;
//! * the **last window closing**, which on this platform is a question rather
//!   than an ending.
//!
//! **It is not the launch socket.** On Windows a second `folio.exe` really does
//! start, and `launch_pipe` is how it hands its command line to the first one
//! (§7.59). That mechanism is still needed on a Mac for `folio --tab x` typed
//! into a shell — a second executable really does start there too — but it can
//! never see a *Finder* launch, because Finder starts no second executable at
//! all. Two different facts, two different doors, and X-4 exists because the
//! plan's first draft thought one covered the other.
//!
//! # Why the shape is a channel and not a trait
//!
//! The AppKit side of this is four C functions hung on a class this program
//! does not own (see [`macos_app`](crate::macos_app)). A C function has no
//! `self` of ours to borrow, cannot hold a `&mut dyn` anything, and — for
//! `applicationShouldTerminate:` — has to return a value from its own stack. So
//! what crosses is a **value on a channel**: [`AppDelegateEvent`], handed to the
//! sender `bt-app` gave [`AppDelegate::install`], which is `bt-app`'s wrapper
//! around `EventLoopProxy::send_event`.
//!
//! **The sender is called on AppKit's stack and must only park.** It runs
//! inside the delegate method, with AppKit's frame underneath it and — for a
//! termination — AppKit waiting on the answer. Anything that turns the event
//! loop from there is the re-entrance X-4 measured as a live process at 100%
//! CPU. `bt-app`'s sender pushes into an inbox and posts one wake, which is the
//! same footing `AttentionSpoke` and `LaunchAsked` already stand on.
//!
//! # The two rules this file is arranged around, both X-4's
//!
//! ① **`-[NSApplication terminate:]` is never called from inside an
//! `ApplicationHandler` callback.** `NSTerminateLater` makes AppKit spin a
//! nested run loop until the answer arrives; winit's run-loop observers are in
//! `kCFRunLoopCommonModes` and do run inside it, so if the call came from a
//! winit callback the handler's `RefCell` is already borrowed and winit answers
//! the re-entry with a panic, once per turn, forever.
//!
//! ② **The deferred answer comes from winit's handler and from nowhere else.**
//! `applicationShouldTerminate:` answers `NSTerminateLater` and returns at once;
//! [`TerminationAnswer::answer`] delivers the real answer later, through
//! `-[NSApplication replyToApplicationShouldTerminate:]`. It cannot be posted to
//! the main dispatch queue instead: X-4 measured that AppKit's deferred
//! termination loop **does not drain it** — neither a queued answer nor a later
//! `dispatch_async` ran, and the application hung. winit's handler *is* driven
//! inside that loop, and the answer from there unwound it in 1 ms.
//!
//! # Why the buffer
//!
//! A cold `application:openURLs:` arrives **before** `resumed` — X-4 timed the
//! cold delivery at t=222 ms against a `resumed` at t=247 ms, with no window in
//! existence. `bt-app` cannot act on it then: it has not yet decided whether the
//! restored session has windows of its own, so a path routed at that moment
//! would open a window the restore is about to open a second time. So the door
//! **holds** every event until [`AppDelegate::ready`], and then releases them in
//! the order they arrived. One rule for all four kinds rather than one for the
//! paths: a quit asked for during launch is honoured the moment launch is up,
//! which is the same sentence.
//!
//! # What is portable here, and why that is not a pretence
//!
//! Everything in this file except the four lines that reach AppKit. The buffer,
//! the one-answer-per-request rule and the URL decoding are facts about
//! **Folio's** lifecycle rather than about macOS, they are what a defect would
//! live in, and they are the half a Windows workstation can actually run. The
//! platform half — looking winit's private delegate class up by name and adding
//! four selectors to it — is [`macos_app`](crate::macos_app), and it is the only
//! part that changes shape per machine. `ShellPickKind` is the precedent
//! (`docs/DESIGN.md` §13.17 ⑦): a fact about the product is written once.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::menu::MenuChoice;

// ── what crosses ───────────────────────────────────────────────────────────

/// **Which delegate method this event came from.**
///
/// Separate from [`AppDelegateEventKind`] rather than folded into it, because
/// the two are not one-to-one and the place that already is not is the one this
/// ticket left open: M4-9's Services deliver [`AppDelegateEventKind::OpenPaths`]
/// too, through `-[NSApplication setServicesProvider:]`, which never touches the
/// delegate at all. An application that wants to say *opened from Finder* and
/// *opened from the Services menu* differently needs the origin; one that does
/// not can match on the kind and ignore it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppDelegateOrigin {
    /// `applicationShouldHandleReopen:hasVisibleWindows:`.
    Reopen,
    /// `application:openURLs:`.
    OpenUrls,
    /// `applicationShouldTerminate:`.
    Termination,
    /// `applicationShouldTerminateAfterLastWindowClosed:`.
    LastWindowClosed,
    /// **Finder's *Services ▸ Open in Folio*** (M4-9), whose method is
    /// `openInFolio:userData:error:` on the provider object
    /// [`crate::macos_services`] owns.
    ///
    /// Not a delegate selector either, and it is the origin this enum was
    /// written for: the row above already said that a Service delivers
    /// [`AppDelegateEventKind::OpenPaths`] "with an origin of its own", and this
    /// is it. **What the origin buys is a real difference in what happens
    /// next**, which is why the two are not one arm at the landing:
    ///
    /// * [`Self::OpenUrls`] is *open this document* — `open -a Folio notes.md`,
    ///   a file dropped on the Dock tile. A folder opens a tab standing in it
    ///   and a **file opens a preview pane**, because the file is the thing the
    ///   reader named;
    /// * `Services` is *open Folio **here*** — the same verb Explorer's
    ///   first-page row is on the other platform, where a clicked file has
    ///   always meant the folder that contains it (`explorer_menu::folder_for`).
    ///   A file selected in Finder opens a tab in **its folder**, not a preview
    ///   of it.
    ///
    /// One list of paths, two readings of what a file among them means, and the
    /// origin is the only thing that separates them.
    Services,
    /// **A row of the application menu bar** (M3-2), whose action is
    /// `folioMenuChosen:` on a target [`crate::macos_menu`] owns.
    ///
    /// Not a delegate selector at all, and it is on this channel on purpose:
    /// what a menu press has in common with a reopen is everything that made
    /// this channel the shape it is. It arrives at **AppKit** rather than at any
    /// window of ours, on the main thread, inside a callback with a framework
    /// frame underneath it — for a menu, AppKit's own tracking loop — so it is
    /// under rule (1) of this module's header word for word, and it needs the
    /// same buffer, the same ordering and the same one drain on the loop's own
    /// turn. A second channel beside this one would be a second answer to the
    /// same question about the same stack.
    Menu,
}

impl AppDelegateOrigin {
    /// The selector this origin is, for a diagnostic line.
    #[must_use]
    pub fn selector(self) -> &'static str {
        match self {
            Self::Reopen => "applicationShouldHandleReopen:hasVisibleWindows:",
            Self::OpenUrls => "application:openURLs:",
            Self::Termination => "applicationShouldTerminate:",
            Self::LastWindowClosed => "applicationShouldTerminateAfterLastWindowClosed:",
            Self::Services => "openInFolio:userData:error:",
            Self::Menu => "folioMenuChosen:",
        }
    }
}

/// **What the application is being asked to do.**
#[derive(Debug)]
pub enum AppDelegateEventKind {
    /// Somebody asked for this application again.
    ///
    /// `had_visible_windows` is AppKit's own flag **verbatim, and it is
    /// advisory**. X-4 measured what it actually answers: a *minimised* window
    /// is YES, a window hidden with `-[NSApplication hide:]` is YES, and only an
    /// application with no windows at all is NO. So it does not mean "the reader
    /// can see something" and must not be read as though it did — the door
    /// reports it and the application decides from its own window list, which is
    /// the only list that knows what a window is *for*.
    Reopen { had_visible_windows: bool },
    /// Open these, one tab each.
    ///
    /// Already decoded: see [`path_from_file_url`] for what "decoded" means and
    /// why it is done here rather than by asking `-[NSURL path]`.
    OpenPaths(Vec<PathBuf>),
    /// The reader asked this application to quit, and **AppKit is waiting**.
    ///
    /// The answer is the carried [`TerminationAnswer`], and it must be given
    /// from the event loop's own handler — see rule ② in this module's header.
    /// Until it is given, the application is alive but AppKit is spinning a
    /// nested run loop on the main thread.
    TerminationRequested(TerminationAnswer),
    /// The last window of this application closed.
    ///
    /// Reported rather than acted on: the door has already answered AppKit NO
    /// (the application stays in the Dock, plan Q10), and this says only that
    /// the moment happened.
    LastWindowClosed,
    /// **A row of the menu bar was pressed** (M3-2), carrying the row's own
    /// choice: the stable id of a shortcut-table row, or one of the few verbs
    /// that has no row there.
    ///
    /// The application answers it through the same `run_shortcut` a chord
    /// reaches — see `docs/DESIGN.md` §13.26 ③. Rows AppKit answers by
    /// itself never arrive here at all; they have no choice behind them, which
    /// is what "standard" means in [`crate::menu::MenuAction`].
    MenuChosen(MenuChoice),
}

/// One application-level event, with the selector it came from.
#[derive(Debug)]
pub struct AppDelegateEvent {
    pub origin: AppDelegateOrigin,
    pub kind: AppDelegateEventKind,
}

// ── the termination answer ─────────────────────────────────────────────────

/// **The two answers to "may I quit".**
///
/// The names are AppKit's own `NSTerminateNow` and `NSTerminateCancel`.
/// `NSTerminateLater`, the third, is not here on purpose: it is what the door
/// has *already* answered on the delegate's stack, and this type is the answer
/// that completes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminationDecision {
    /// The application has finished saving and may go — `NSTerminateNow`.
    Now,
    /// The application refuses; the reader cancelled something —
    /// `NSTerminateCancel`.
    Cancel,
}

impl TerminationDecision {
    /// What `-[NSApplication replyToApplicationShouldTerminate:]` takes.
    #[must_use]
    fn as_reply(self) -> bool {
        matches!(self, Self::Now)
    }
}

/// The one answer a termination request gets, and the flag that makes it one.
#[derive(Debug)]
struct TerminationSlot {
    answered: AtomicBool,
}

/// **AppKit, waiting** — the handle that ends one `applicationShouldTerminate:`.
///
/// It is carried in the event rather than fetched from the door, so that there
/// is no way to answer a request the application was never told about, and no
/// way for two requests to be confused for one.
///
/// Answering is **exactly once**: a second call is refused with a reason rather
/// than delivered, because `replyToApplicationShouldTerminate:` twice is a
/// message to an AppKit that is no longer listening, and the second answer is
/// always the one that is wrong about something.
#[derive(Debug, Clone)]
pub struct TerminationAnswer {
    slot: Arc<TerminationSlot>,
}

impl TerminationAnswer {
    /// A fresh, unanswered request.
    ///
    /// Built by `applicationShouldTerminate:` and by nothing else, which is why
    /// it is unreachable on a machine with no such selector — the whole of the
    /// gate below.
    #[cfg_attr(
        not(any(target_os = "macos", test)),
        expect(
            dead_code,
            reason = "M3-1: a termination request is made by AppKit, and there is no AppKit here"
        )
    )]
    pub(crate) fn new() -> Self {
        Self {
            slot: Arc::new(TerminationSlot {
                answered: AtomicBool::new(false),
            }),
        }
    }

    /// Whether this request has already been answered.
    #[must_use]
    pub fn is_answered(&self) -> bool {
        self.slot.answered.load(Ordering::SeqCst)
    }

    /// **Complete the request.** Call this from the event loop's own handler.
    ///
    /// Not from inside the sender `install` was given, and not from a worker
    /// thread: `replyToApplicationShouldTerminate:` is `NSApplication`'s and
    /// `NSApplication` is the main thread's, and the whole reason the answer is
    /// deferred is so that it leaves AppKit's delegate stack before it is given
    /// (rule ② in this module's header).
    ///
    /// # Errors
    ///
    /// If this request has already been answered, or — on macOS — if the caller
    /// is not on the main thread.
    pub fn answer(&self, decision: TerminationDecision) -> Result<(), String> {
        if self.slot.answered.swap(true, Ordering::SeqCst) {
            return Err(
                "this termination request has already been answered; AppKit asked once and is \
                 waiting for one reply"
                    .to_owned(),
            );
        }
        #[cfg(target_os = "macos")]
        {
            crate::macos_app::reply_to_should_terminate(decision.as_reply()).inspect_err(|_| {
                // The reply never left, so the request is still outstanding and
                // the caller may try again from the right thread.
                self.slot.answered.store(false, Ordering::SeqCst);
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = decision.as_reply();
            Ok(())
        }
    }
}

// ── the buffer, and the sender it eventually reaches ───────────────────────

/// What the door is holding, and whether it is still holding it.
#[derive(Default)]
struct Held {
    /// False until [`AppDelegate::ready`]; every event waits behind it.
    ready: bool,
    queued: VecDeque<AppDelegateEvent>,
}

/// **The door's own half of the channel**: buffer until ready, then in order.
///
/// A type of its own rather than three fields on [`AppDelegate`], because this
/// is the part with a rule in it and therefore the part the tests are about. It
/// is what the four delegate methods hold, through an `Arc` they reach out of a
/// `static` — a C function has no `self` of ours.
pub(crate) struct Outbox {
    send: Box<dyn Fn(AppDelegateEvent) + Send + Sync>,
    held: Mutex<Held>,
}

impl Outbox {
    fn new(send: Box<dyn Fn(AppDelegateEvent) + Send + Sync>) -> Self {
        Self {
            send,
            held: Mutex::new(Held::default()),
        }
    }

    /// Take one event from AppKit. Never blocks and never runs application code
    /// while the door's own lock is held.
    pub(crate) fn post(&self, event: AppDelegateEvent) {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .queued
            .push_back(event);
        self.drain();
    }

    /// The application is up; release what was held and stop holding.
    fn release(&self) {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .ready = true;
        self.drain();
    }

    fn is_ready(&self) -> bool {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .ready
    }

    /// Hand over everything that may go, oldest first.
    ///
    /// One event is taken per turn of the loop and the lock is dropped before
    /// the sender is called. That is not caution about threads — every one of
    /// these arrives on the main thread — but about **order**: the sender is
    /// `bt-app`'s, it is called with AppKit's frame underneath it, and a lock
    /// held across it would be a lock held across code this crate does not own.
    fn drain(&self) {
        loop {
            let next = {
                let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
                if !held.ready {
                    return;
                }
                match held.queued.pop_front() {
                    Some(event) => event,
                    None => return,
                }
            };
            (self.send)(next);
        }
    }
}

// ── the door ───────────────────────────────────────────────────────────────

/// One process, one delegate. See [`AppDelegate::install`].
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// **The application delegate bridge.**
///
/// Hold it for as long as the process runs: dropping it does not take the four
/// selectors back off winit's class — they cannot be taken back off, the
/// Objective-C runtime has no `class_removeMethod` — so a dropped door would be
/// a delegate posting into a channel nobody reads.
pub struct AppDelegate {
    outbox: Arc<Outbox>,
}

impl AppDelegate {
    /// **Open the door.** Call it once, *after* the event loop has been built.
    ///
    /// The order is not a style preference on macOS: building winit's event loop
    /// is what registers the delegate class this door adds its four selectors to,
    /// and before that there is no class to find. See
    /// [`macos_app`](crate::macos_app) for why the selectors are added to
    /// winit's own class rather than to a delegate of Folio's, and for why
    /// winit's documentation says the opposite.
    ///
    /// `send` is called **on AppKit's stack**, inside the delegate method, and
    /// must only park what it is given — see this module's header. Nothing is
    /// sent through it until [`AppDelegate::ready`].
    ///
    /// # Errors
    ///
    /// If a delegate has already been installed in this process, or — on macOS —
    /// if winit's delegate class cannot be found or a selector cannot be added.
    /// Off macOS the door opens and stays quiet: there is no application
    /// delegate on Windows or on Linux, and the second launch and the quit
    /// request arrive there as `launch_pipe` and as the window's own close.
    pub fn install(
        send: impl Fn(AppDelegateEvent) + Send + Sync + 'static,
    ) -> Result<Self, String> {
        if INSTALLED.swap(true, Ordering::SeqCst) {
            return Err(
                "the application delegate is installed once per process; a second install would \
                 be a second channel the first one's events never reach"
                    .to_owned(),
            );
        }
        let outbox = Arc::new(Outbox::new(Box::new(send)));
        #[cfg(target_os = "macos")]
        if let Err(reason) = crate::macos_app::add_the_four_selectors(Arc::clone(&outbox)) {
            INSTALLED.store(false, Ordering::SeqCst);
            return Err(reason);
        }
        Ok(Self { outbox })
    }

    /// **The application is up; release what arrived before it was.**
    ///
    /// Called from `resumed`, once. Everything held is handed over in arrival
    /// order and everything after it goes straight through. Calling it twice is
    /// harmless and means nothing the second time.
    pub fn ready(&self) {
        self.outbox.release();
    }

    /// Whether [`AppDelegate::ready`] has been called.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.outbox.is_ready()
    }

    /// **A third AppKit door into this same channel: Finder's *Services ▸ Open
    /// in Folio*** (M4-9).
    ///
    /// Registers the provider object AppKit sends `openInFolio:userData:error:`
    /// to, and asks LaunchServices to re-read this bundle's Services table so
    /// that a Folio which has just been downloaded, moved or built has the row
    /// now rather than after a logout.
    ///
    /// **It is a call of its own and not a line inside [`AppDelegate::install`]**,
    /// for one reason: the two can fail separately and the consequences are not
    /// the same size. A delegate that could not be installed is a Folio that
    /// never answers a Dock click; a Service that could not be registered is one
    /// row missing from one Finder menu, with `folio <folder>` and every other
    /// way in untouched. Folding them together would make the second refusal
    /// cost the first.
    ///
    /// Called **after** `install` and from the main thread. After, because what
    /// the provider posts into is the channel `install` fills, and a Service
    /// arriving into an empty cell is a delivery on the floor. It is held until
    /// [`AppDelegate::ready`] by the same buffer the four selectors are, which
    /// is what makes the **cold** case — LaunchServices starting this process
    /// *because of* the Service, X-4's delivery at t=222 ms against a `resumed`
    /// at 247 ms — reach a window instead of a program that has not got one yet.
    ///
    /// # Errors
    ///
    /// On macOS, if this is called off the main thread. Off macOS it answers
    /// `Ok(())` and registers nothing, which is [`AppDelegate::install`]'s own
    /// shape and the same sentence: there is no Services menu on Windows or on a
    /// Linux desktop, the verb reaches this program there through
    /// [`crate::explorer_command`] and through the command line, and a refusal
    /// printed on every launch of a platform that was never going to have the row
    /// would be a fault report about the machine being itself.
    pub fn offer_open_in_folio(&self) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            crate::macos_services::install()
        }
        #[cfg(not(target_os = "macos"))]
        {
            Ok(())
        }
    }

    /// **A second AppKit door, speaking into this same channel** (M3-2).
    ///
    /// The menu bar is not a delegate selector, and it is on this channel for
    /// the reason [`AppDelegateOrigin::Menu`] gives: the press arrives at
    /// AppKit inside a callback with a framework frame underneath it, so it
    /// wants this channel's buffer, this channel's ordering and this channel's
    /// single drain on the loop's own turn. A sender of its own would be a
    /// second answer to the same question about the same stack — and, less
    /// abstractly, a reopen and the `New window` row a reader pressed a
    /// millisecond later would arrive in two inboxes with no order between them.
    ///
    /// It is handed out rather than the outbox itself, so that nothing outside
    /// this module can release the buffer or read what is held.
    pub fn sender(&self) -> impl Fn(AppDelegateEvent) + Send + Sync + Clone + 'static {
        let outbox = Arc::clone(&self.outbox);
        move |event| outbox.post(event)
    }
}

// ── a `file:` URL, as the path it names ────────────────────────────────────

/// **The path a `file:` URL names, as bytes rather than as text.**
///
/// `application:openURLs:` hands over `NSURL`s and `bt-app` wants paths. There
/// is an AppKit answer — `-[NSURL path]` — and this is not it, for the reason
/// M2-2 gave in the other direction (`docs/DESIGN.md` §13.18 ①): that method
/// answers an `NSString`, and a path is **not required to be text**. A name on
/// an SMB or exFAT mount can be bytes that are not UTF-8, the file column can be
/// rooted on one, and a round trip through a Rust `String` hands back a
/// different file. The percent escapes in the URL *are* those bytes, so decoding
/// them is the faithful route and asking for a string is not.
///
/// It is also the half of the door a machine with no AppKit can run, which is
/// why the spaces and the CJK in the ticket are checked on the workstation and
/// confirmed against AppKit's own URLs on the Mac rather than only there.
///
/// What is accepted: a `file:` scheme in any case, an empty or `localhost`
/// authority, and an absolute path. What is refused, each with its own sentence:
/// another scheme (`https:` is a page, not a file), another host (a URL naming
/// somebody else's machine is not a local path), a relative path (`open` never
/// sends one, and the directory it would be relative to is the one the launch
/// happened to stand in rather than the one the reader meant — `handoff`'s own
/// ruling), and an embedded NUL, which is the terminator of the C string the
/// kernel is eventually handed.
///
/// # Errors
///
/// One sentence per refusal above.
pub fn path_from_file_url(url: &str) -> Result<PathBuf, String> {
    let Some(rest) = strip_scheme(url, "file:") else {
        return Err(format!("{url} is not a file: URL"));
    };
    // `file:/path` and `file:///path` are both legal spellings of the same
    // thing; only the second carries an authority, and only then is there a
    // host to check.
    let path = match rest.strip_prefix("//") {
        Some(after) => {
            let (authority, path) = match after.find('/') {
                Some(at) => after.split_at(at),
                None => (after, ""),
            };
            if !(authority.is_empty() || authority.eq_ignore_ascii_case("localhost")) {
                return Err(format!(
                    "{url} names the host {authority}, which is another machine rather than a \
                     path on this one"
                ));
            }
            path
        }
        None => rest,
    };
    // A query or a fragment ends the path. Neither can occur inside one: AppKit
    // writes `?` as `%3F` and `#` as `%23` in a file URL, which is exactly what
    // makes this split safe rather than lossy.
    let path = &path[..path.find(['?', '#']).unwrap_or(path.len())];
    if !path.starts_with('/') {
        return Err(format!(
            "{url} names a relative path, and the directory that would be relative to is the one \
             this process was started in rather than the one the reader meant"
        ));
    }
    let bytes = percent_decode(path)?;
    if bytes.contains(&0) {
        return Err(format!(
            "{url} decodes to a path with a NUL in it, which ends the name the kernel is handed \
             early and opens a different file"
        ));
    }
    path_from_bytes(bytes, url)
}

/// `url` after `scheme`, if that is what it starts with — the scheme of a URL
/// is case-insensitive.
///
/// `get` rather than an index, because the argument is whatever AppKit handed
/// over: slicing a `str` at a byte offset that is not a character boundary is a
/// panic, and a URL that begins with a non-ASCII character would reach one.
fn strip_scheme<'a>(url: &'a str, scheme: &str) -> Option<&'a str> {
    url.get(..scheme.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(scheme))
        .then(|| &url[scheme.len()..])
}

/// The bytes a percent-encoded path component spells.
///
/// Bytes and not characters: `%E6%96%87` is one CJK character in three escapes
/// and nothing here has to know that. Everything that is not an escape passes
/// through as its own UTF-8 bytes, so a URL that carries a raw non-ASCII
/// character — which AppKit does not write, but a hand-made one might — decodes
/// to the same path as the encoded spelling of it.
fn percent_decode(path: &str) -> Result<Vec<u8>, String> {
    let source = path.as_bytes();
    let mut out = Vec::with_capacity(source.len());
    let mut at = 0;
    while at < source.len() {
        if source[at] != b'%' {
            out.push(source[at]);
            at += 1;
            continue;
        }
        let digits = source
            .get(at + 1..at + 3)
            .ok_or_else(|| format!("{path} ends in the middle of a percent escape"))?;
        let high = hex_value(digits[0]);
        let low = hex_value(digits[1]);
        match (high, low) {
            (Some(high), Some(low)) => out.push(high * 16 + low),
            _ => {
                return Err(format!(
                    "{path} carries %{}{}, which is not a percent escape",
                    digits[0] as char, digits[1] as char
                ));
            }
        }
        at += 3;
    }
    Ok(out)
}

/// One hexadecimal digit, in either case.
fn hex_value(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

/// The decoded bytes as a path.
///
/// **The one place in this file that is not the same on every machine**, and it
/// is the platform's own fact rather than a shim: a Unix path *is* bytes, so
/// there is nothing to decide; a Windows path is UTF-16 and has no byte
/// spelling, so bytes that are not UTF-8 name nothing there and saying so is
/// more honest than inventing replacement characters. Nothing on Windows sends
/// a `file:` URL through this door — the arm exists because the rule above it
/// is checked there.
#[cfg(unix)]
fn path_from_bytes(bytes: Vec<u8>, _url: &str) -> Result<PathBuf, String> {
    use std::os::unix::ffi::OsStringExt;
    Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: Vec<u8>, url: &str) -> Result<PathBuf, String> {
    String::from_utf8(bytes)
        .map(PathBuf::from)
        .map_err(|_| format!("{url} decodes to bytes that are not a name on this platform"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sender that keeps what it was given, in order.
    fn collector() -> (
        Arc<Mutex<Vec<String>>>,
        impl Fn(AppDelegateEvent) + Send + Sync,
    ) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let into = Arc::clone(&seen);
        (seen, move |event: AppDelegateEvent| {
            into.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(describe(&event));
        })
    }

    /// One event as one line, so that order and content are one assertion.
    fn describe(event: &AppDelegateEvent) -> String {
        match &event.kind {
            AppDelegateEventKind::Reopen {
                had_visible_windows,
            } => format!("{:?} reopen visible={had_visible_windows}", event.origin),
            AppDelegateEventKind::OpenPaths(paths) => format!(
                "{:?} paths {:?}",
                event.origin,
                paths
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
            ),
            AppDelegateEventKind::TerminationRequested(_) => {
                format!("{:?} terminate", event.origin)
            }
            AppDelegateEventKind::LastWindowClosed => format!("{:?} last-window", event.origin),
            AppDelegateEventKind::MenuChosen(choice) => {
                format!("{:?} menu {choice:?}", event.origin)
            }
        }
    }

    fn reopen(had_visible_windows: bool) -> AppDelegateEvent {
        AppDelegateEvent {
            origin: AppDelegateOrigin::Reopen,
            kind: AppDelegateEventKind::Reopen {
                had_visible_windows,
            },
        }
    }

    fn paths(of: &[&str]) -> AppDelegateEvent {
        AppDelegateEvent {
            origin: AppDelegateOrigin::OpenUrls,
            kind: AppDelegateEventKind::OpenPaths(of.iter().map(PathBuf::from).collect()),
        }
    }

    fn seen(of: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
        of.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// The door, without a platform under it.
    fn a_door(send: impl Fn(AppDelegateEvent) + Send + Sync + 'static) -> AppDelegate {
        AppDelegate {
            outbox: Arc::new(Outbox::new(Box::new(send))),
        }
    }

    /// RED — **nothing crosses before the application says it is up, and then
    /// everything does, oldest first.**
    ///
    /// This is X-4's cold Service measured as a rule: the delivery landed 25 ms
    /// *before* `resumed`, with no window in existence. A door that forwarded it
    /// then would hand `bt-app` a path to open before it had decided whether the
    /// restored session opens a window of its own.
    ///
    /// MUTATION: forward in `post` without consulting `ready` and the first
    /// assertion fails; release with `pop_back` and the order one does.
    #[test]
    fn nothing_is_delivered_until_the_application_is_ready_and_then_in_order() {
        let (seen_here, send) = collector();
        let door = a_door(send);
        door.outbox.post(paths(&["/one"]));
        door.outbox.post(reopen(false));
        door.outbox.post(paths(&["/two"]));
        assert!(!door.is_ready());
        assert!(
            seen(&seen_here).is_empty(),
            "a cold delivery arrives before `resumed` and must wait for it"
        );

        door.ready();
        assert!(door.is_ready());
        assert_eq!(
            seen(&seen_here),
            vec![
                "OpenUrls paths [\"/one\"]".to_owned(),
                "Reopen reopen visible=false".to_owned(),
                "OpenUrls paths [\"/two\"]".to_owned(),
            ],
            "everything held is released in the order AppKit delivered it"
        );

        door.outbox.post(paths(&["/three"]));
        assert_eq!(
            seen(&seen_here).len(),
            4,
            "and after that nothing is held at all"
        );
    }

    /// RED — **`ready` twice is not a second delivery.**
    ///
    /// MUTATION: re-queue on release, or leave the released events in the
    /// buffer, and the second `ready` doubles every event `bt-app` has already
    /// acted on — one tab per path becomes two.
    #[test]
    fn a_second_ready_delivers_nothing_a_second_time() {
        let (seen_here, send) = collector();
        let door = a_door(send);
        door.outbox.post(reopen(true));
        door.ready();
        door.ready();
        assert_eq!(
            seen(&seen_here),
            vec!["Reopen reopen visible=true".to_owned()]
        );
    }

    /// RED — **the flag AppKit sets is carried verbatim; the door decides
    /// nothing from it.**
    ///
    /// X-4 measured that `hasVisibleWindows` is YES for a *minimised* window and
    /// YES for one hidden with `-[NSApplication hide:]`, so a door that dropped
    /// the NO case, or that answered the reopen itself when the flag was YES,
    /// would be answering a question it cannot see the answer to.
    ///
    /// MUTATION: filter either value out in `post` and one half of this fails.
    #[test]
    fn a_reopen_carries_the_raw_flag_with_windows_and_without() {
        let (seen_here, send) = collector();
        let door = a_door(send);
        door.ready();
        door.outbox.post(reopen(true));
        door.outbox.post(reopen(false));
        assert_eq!(
            seen(&seen_here),
            vec![
                "Reopen reopen visible=true".to_owned(),
                "Reopen reopen visible=false".to_owned(),
            ]
        );
    }

    /// RED — **one termination request gets exactly one answer, and an answer
    /// that never left is not one of them.**
    ///
    /// AppKit asked once and is spinning a nested run loop on the answer;
    /// `replyToApplicationShouldTerminate:` a second time is a message to
    /// nobody, and whichever of the two answers is wrong is the one that would
    /// have been believed.
    ///
    /// **Two branches, because on a Mac this case is not on the main thread.**
    /// `docs/DESIGN.md` §13.17 measured that libtest never hands a case the
    /// process's main thread, so the delivery underneath refuses there — and a
    /// reply that never left must leave the request outstanding, which is the
    /// other half of the same rule and the half that branch states. The
    /// delivered half is stated here on a machine with no AppKit, and on a Mac
    /// by `tests/macos_app_delegate.rs` ⑦, which is on the main thread and has
    /// a real `applicationShouldTerminate:` waiting on it.
    ///
    /// MUTATION: drop the `swap` for a `load`/`store` pair, or let `answer`
    /// return `Ok` when it has already been answered, and the second call
    /// succeeds; drop the rollback and the other branch fails.
    #[test]
    fn a_termination_request_is_answered_exactly_once() {
        let answer = TerminationAnswer::new();
        assert!(!answer.is_answered());
        match answer.answer(TerminationDecision::Now) {
            Ok(()) => {
                assert!(answer.is_answered());
                let second = answer.answer(TerminationDecision::Cancel);
                assert!(
                    second.is_err_and(|reason| reason.contains("already been answered")),
                    "the second answer is refused with a reason rather than sent"
                );
            }
            Err(reason) => {
                assert!(
                    !reason.contains("already been answered"),
                    "the first answer was refused as a repeat: {reason}"
                );
                assert!(
                    !answer.is_answered(),
                    "the reply never left, so this request is still outstanding and a caller on \
                     the right thread may still answer it"
                );
            }
        }
    }

    /// RED — **two handles are still one request.**
    ///
    /// The handle is `Clone` because the event that carries it is the
    /// application's to keep. Branched for the case above's reason.
    ///
    /// MUTATION: give `TerminationAnswer` its own `AtomicBool` instead of
    /// sharing the slot through the `Arc` and the clone answers a second time.
    #[test]
    fn a_cloned_answer_is_the_same_request() {
        let answer = TerminationAnswer::new();
        let also = answer.clone();
        match answer.answer(TerminationDecision::Cancel) {
            Ok(()) => {
                assert!(also.is_answered(), "the clone is the same request");
                assert!(also.answer(TerminationDecision::Now).is_err());
            }
            Err(_) => assert!(
                !also.is_answered(),
                "the clone is the same request, and that request is still outstanding"
            ),
        }
    }

    /// RED — **the delegate is installed once, and a door that could not be
    /// opened has not spent that one install.**
    ///
    /// The only case in this file that calls `install`, because the flag it
    /// checks is the process's.
    ///
    /// Branched for the reason the two cases above are: on a Mac `install`
    /// reaches AppKit and a libtest case is not on the main thread (§13.17), so
    /// what this states there is the rollback — the half that matters most,
    /// because a refused install that had consumed the flag would leave the
    /// process unable to ever open the door. `tests/macos_app_delegate.rs`
    /// opens the real one, on the main thread.
    ///
    /// MUTATION: drop the `swap` and a second `install` builds a second channel
    /// that AppKit never posts into, which looks exactly like a working door;
    /// drop the rollback and the other branch fails.
    #[test]
    fn the_delegate_is_installed_once_per_process() {
        match AppDelegate::install(|_| {}) {
            Ok(_door) => {
                let second = AppDelegate::install(|_| {});
                assert!(
                    second
                        .err()
                        .is_some_and(|reason| reason.contains("once per process")),
                    "a second install is refused with a reason"
                );
            }
            Err(reason) => {
                assert!(
                    !reason.contains("once per process"),
                    "the first install was refused as a repeat: {reason}"
                );
                let again = AppDelegate::install(|_| {});
                assert_eq!(
                    again.err().as_deref(),
                    Some(reason.as_str()),
                    "a door that could not be opened has spent the one install this process gets"
                );
            }
        }
    }

    /// RED — **a `file:` URL decodes to the bytes it spells.**
    ///
    /// The three the ticket names and the shapes around them. `%20` is a space
    /// and `%E4%B8%AD%E6%96%87` is 中文; an unencoded space would be a URL AppKit
    /// never writes, and it decodes to the same path anyway.
    ///
    /// MUTATION: decode `+` as a space (form encoding, not path encoding) and
    /// the plus case fails; stop at the first `%` and every escaped case does.
    #[test]
    fn a_file_url_decodes_to_the_path_it_names() {
        for (url, path) in [
            ("file:///Users/example/notes.md", "/Users/example/notes.md"),
            (
                "file:///tmp/a%20folder/My%20Notes",
                "/tmp/a folder/My Notes",
            ),
            ("file:///tmp/%E4%B8%AD%E6%96%87", "/tmp/中文"),
            ("file:///tmp/%e4%b8%ad%e6%96%87", "/tmp/中文"),
            ("FILE:///tmp/x", "/tmp/x"),
            ("file://localhost/tmp/x", "/tmp/x"),
            ("file:/tmp/x", "/tmp/x"),
            ("file:///tmp/a+b", "/tmp/a+b"),
            ("file:///tmp/%23hash%3Fq", "/tmp/#hash?q"),
            ("file:///tmp/folder/", "/tmp/folder/"),
            ("file:///tmp/x?v=1", "/tmp/x"),
            ("file:///tmp/x#top", "/tmp/x"),
            ("file:///tmp/caf%C3%A9 au lait", "/tmp/café au lait"),
            // A URL that begins with a non-ASCII character: nothing sends one,
            // and slicing the scheme off by byte offset would have panicked
            // rather than refused it.
        ] {
            assert_eq!(
                path_from_file_url(url).as_deref(),
                Ok(std::path::Path::new(path)),
                "{url}"
            );
        }
    }

    /// RED — **every refusal says which one it is.**
    ///
    /// MUTATION: accept any scheme and the `https:` case passes silently, which
    /// is a web address opened as a tab in a folder that does not exist.
    #[test]
    fn a_url_that_is_not_a_local_path_is_refused_with_its_own_reason() {
        for (url, because) in [
            ("https://example.com/x", "not a file: URL"),
            ("folio://open/x", "not a file: URL"),
            ("file://elsewhere/tmp/x", "another machine"),
            ("file:tmp/x", "relative path"),
            ("file:///tmp/%zz", "not a percent escape"),
            ("file:///tmp/%4", "middle of a percent escape"),
            ("file:///tmp/a%00b", "NUL"),
            ("中文", "not a file: URL"),
            ("fil", "not a file: URL"),
            ("", "not a file: URL"),
        ] {
            let refusal = path_from_file_url(url);
            assert!(
                refusal
                    .as_ref()
                    .err()
                    .is_some_and(|reason| reason.contains(because)),
                "{url} should be refused for {because}, and was {refusal:?}"
            );
        }
    }

    /// RED — **a Service is `OpenPaths` from an origin of its own, and it waits
    /// behind the same door a cold `application:openURLs:` waits behind**
    /// (M4-9).
    ///
    /// Both halves matter and neither is obvious from the type. The **kind** is
    /// shared, so a `bt-app` that matched on the kind alone would answer a
    /// Service as though it were a document; the **origin** is not, and it is
    /// the only thing carrying the difference. And the **buffer** is the one
    /// thing this ticket did not have to build: a Service can be the reason this
    /// process exists, LaunchServices delivers it before `resumed` (X-4 timed
    /// t=222 ms against 247 ms), and it reaches a window only because it is held
    /// with everything else.
    ///
    /// MUTATION: give the Services door a channel of its own that forwards at
    /// once and the first assertion fails; post it with
    /// `AppDelegateOrigin::OpenUrls` and the second does.
    #[test]
    fn a_service_is_open_paths_from_its_own_origin_and_waits_for_ready() {
        let (seen_here, send) = collector();
        let door = a_door(send);
        door.outbox.post(AppDelegateEvent {
            origin: AppDelegateOrigin::Services,
            kind: AppDelegateEventKind::OpenPaths(
                ["/tmp/a folder", "/tmp/中文", "/tmp/plain"]
                    .iter()
                    .map(PathBuf::from)
                    .collect(),
            ),
        });
        assert!(
            seen(&seen_here).is_empty(),
            "a cold Service arrives before the application is up and must wait for it"
        );

        door.ready();
        assert_eq!(
            seen(&seen_here),
            vec!["Services paths [\"/tmp/a folder\", \"/tmp/中文\", \"/tmp/plain\"]".to_owned()],
            "a multi-selection crosses as one event, in the reader's own order, \
             from the Services origin"
        );
    }

    /// PIN — **the selector the origin names is the method the provider
    /// answers, and the `NSMessage` the bundle declares is that selector's
    /// first word** (M4-9).
    ///
    /// Three files have to agree for one Finder row to work and no compiler
    /// reads any of the joins: `Info.plist`'s `NSMessage`, the
    /// `#[unsafe(method(…))]` on the provider class, and this enum's own
    /// sentence about where a Service comes from. A mismatch is not a build
    /// failure — it is a row that draws, is pressed, and reports a Service that
    /// failed to the reader.
    ///
    /// `NSMessage` is the **first word only**: AppKit appends `:userData:error:`
    /// itself. Writing the whole selector into the plist is the plausible
    /// mistake, and this is the test that has it.
    ///
    /// MUTATION: write `openInFolio:userData:error:` into the template's
    /// `NSMessage`, or rename the method on the class, and this fails.
    #[test]
    fn the_service_this_bundle_declares_is_the_one_the_provider_answers() {
        const SELECTOR: &str = "openInFolio:userData:error:";
        const PROVIDER: &str = include_str!("macos_services.rs");
        let plist = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../packaging/macos/Info.plist.in"
        ))
        .expect("the bundle template is at packaging/macos/Info.plist.in");

        assert_eq!(
            AppDelegateOrigin::Services.selector(),
            SELECTOR,
            "the origin names the method AppKit sends"
        );
        assert!(
            PROVIDER.contains(&format!("#[unsafe(method({SELECTOR}))]")),
            "the provider class does not answer {SELECTOR}"
        );
        let message = plist_string(&plist, "NSMessage");
        assert_eq!(
            message, "openInFolio",
            "NSMessage is the first word of the selector and AppKit appends the rest"
        );
        assert!(
            SELECTOR.starts_with(&format!("{message}:")),
            "{message} is not the first word of {SELECTOR}"
        );
        assert_eq!(
            plist_string(&plist, "NSPortName"),
            plist_string(&plist, "CFBundleName"),
            "a Service's port is the bundle's own name"
        );
        assert!(
            plist.contains("<string>public.file-url</string>"),
            "the row is offered for a file selection or for nothing"
        );
        assert!(
            !plist.contains("<key>NSRequiredContext</key>"),
            "a required context would take the row off files or off folders"
        );
        assert!(
            PROVIDER.contains("NSUpdateDynamicServices()"),
            "a freshly built or freshly moved bundle's row would not appear until a logout"
        );
        assert!(
            PROVIDER.contains("AppDelegateOrigin::Services"),
            "the provider posts a Service as something else"
        );
    }

    /// The `<string>` a plist files under `key` — the reader
    /// `bt_app::version`'s own plist pins use, for its reason: the thing under
    /// test is the text of a generated file, and a parser would only ever agree
    /// with the writer.
    fn plist_string(plist: &str, key: &str) -> String {
        let at = plist
            .find(&format!("<key>{key}</key>"))
            .unwrap_or_else(|| panic!("the template files something under {key}"));
        let rest = &plist[at..];
        let opens = rest.find("<string>").expect("a string follows the key") + "<string>".len();
        let closes = rest.find("</string>").expect("the string is closed");
        rest[opens..closes].to_owned()
    }

    /// RED — **a name that is not text is still a name.**
    ///
    /// Unix only, because it is the platform's own fact: `0xFF` is not UTF-8 and
    /// is a perfectly ordinary byte in a file name on an SMB or exFAT mount,
    /// which `docs/DESIGN.md` §13.18 ① records the file column can be rooted on.
    /// A decoder that went through a `String` would answer a different file or
    /// none.
    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_utf8_survives_the_decoding() {
        use std::os::unix::ffi::OsStrExt;
        let path = path_from_file_url("file:///tmp/%FF%FE").expect("bytes are a name here");
        assert_eq!(path.as_os_str().as_bytes(), b"/tmp/\xff\xfe");
    }
}
