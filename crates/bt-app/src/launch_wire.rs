//! **What a second launch says to the Folio that is already running** — the grammar on
//! `bt_platform::launch_pipe`'s wire, and both ends of it (`docs/DESIGN.md` §7.59).
//!
//! The split is [`crate::attention_wire`]'s and is kept for its reason: `bt_platform::launch_pipe`
//! owns the kernel object and knows nothing about grammar; this module owns the grammar and knows
//! nothing about the kernel, so the unsafe boundary does not have to be reopened to change a field
//! name.
//!
//! # No free payload crosses, here either
//!
//! The attention wire's founding rule, at the second door and in its strongest form: this channel
//! carries **five declared fields and nothing else** — a folder, a profile id, whether the launch
//! asked for a window of its own, whether it asked for a tab, and who started it. There is no room
//! in it for a command to run, a document to open or a name to type, because a channel that carried
//! any of those would be a channel worth attacking: it is answered by a process that has a terminal
//! in it.
//!
//! # The wire carries the origin; the decision is made on this side
//!
//! The three fields that are not the folder and the profile are all about **where the launch
//! lands**, and none of them is the answer. [`landing`] is the answer, it is one function, and it
//! runs in the process that is already up — the one that has `settings.json` open. The second
//! process never reads a setting: it holds no claim on the data directory, and a build that let it
//! read one anyway would be reading a document another process owns, at the one moment that
//! document is most likely to be being written.
//!
//! Both fields that are text are bounded and checked **at both ends**. That is not belt and braces:
//! the client checks because a frame past the bound must never reach a pipe at all, and the server
//! checks because it has no reason to trust that whatever wrote the frame is this build.
//!
//! # The folder goes through the door a printed path goes through
//!
//! [`bt_transcript::paths::is_local_absolute_path`] is the gate every path this window is asked to
//! believe in already passes — drive-rooted, nameable by this filesystem, no NUL — and this one
//! additionally has to be a **directory that exists**, because what it is for is a tab that opens
//! standing in it. Anything else is refused, and the launch that sent it opens nothing and says why
//! on its own console.
//!
//! That last sentence is a deliberate difference from a cold launch, which opens a window and puts
//! the same sentence on a card. A launch that vanished into somebody else's window and then quietly
//! did nothing would be worse than one that speaks: the person who typed it is standing at a
//! console, and the console is where they are looking.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};

use bt_platform::launch_pipe::LaunchPipe;

use crate::cli;

/// The wire's version, and it is [`crate::attention_wire`]'s reason exactly: a `folio.exe` started
/// from a shortcut may be a different build from the one holding the data directory — a user who
/// upgraded while a window was open is the ordinary way that happens. A frame from a version this
/// build does not know is dropped rather than half-understood, and the launch that sent it opens
/// its own window, which is what every launch did before this channel existed.
///
/// **2 since the ruling of 2026-09-11** (§7.59), and the bump is the ruling rather than the two new
/// keys. A v1 frame's `"new":false` meant *open a tab*, because that is what the build that wrote
/// it would have done with it; under this build the same bytes mean *ask the row*, and the row's
/// shipped answer is a window. A reader that accepted v1 and filled the missing keys in with
/// defaults would therefore be quietly changing what somebody else's build said — so v1 is dropped,
/// which is this constant's own rule and lands a mixed pair of builds on the behaviour every Folio
/// had before the channel existed: each launch opens its own window.
const WIRE_VERSION: u64 = 2;

/// **The most bytes a folder may cross as.**
///
/// The frame bound is `bt_platform::attention_pipe::MAX_MESSAGE_BYTES` — four kilobytes — and one
/// request is one path, one profile id and a boolean, so this leaves room for the rest of the
/// object several times over. It is four times the `MAX_PATH` that Explorer's verb,
/// `folio-here.cmd`, a pinned icon and a shortcut actually produce, and it is a bound rather than
/// the frame's own because the field that could be long is this one and the honest place to say
/// how long is beside the field.
const MAX_FOLDER_BYTES: usize = 2048;

/// **The most bytes a profile id may cross as** — the attention wire's own bound for a declared
/// text field, unchanged, because a profile id is a slug in this build's table and 128 bytes is
/// already far more than any of them.
const MAX_PROFILE_BYTES: usize = 128;

/// **One launch, in the words it crosses the pipe in.**
///
/// Five fields, and the shape is the ruling: a second `folio.exe` is saying what it was asked for
/// and who asked, and the process that is already up decides where that lands ([`landing`]). It is
/// deliberately not a `CliRequest` — that type carries a bare positional and a COM switch, neither
/// of which this channel has any business carrying, and a message that was "the command line" would
/// grow a field every time the command line did.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct LaunchRequest {
    /// Where the tab opens, in **Windows' own namespace** — the namespace a command line is written
    /// in, crossed into the profile's on the other side by the same door every other folder goes
    /// through.
    pub(crate) cwd: Option<PathBuf>,
    /// The profile id the caller typed, not an index. Whether this build has one by that name is
    /// the receiving window's question and is answered there with the card it already has for it.
    pub(crate) profile: Option<String>,
    /// `--new-window`: a window of its own, in the running process.
    pub(crate) new_window: bool,
    /// `--tab`: a tab in the window the reader was last in.
    pub(crate) tab: bool,
    /// Who started this launch — see [`cli::LaunchOrigin`]. Not a decision; [`landing`] is.
    pub(crate) origin: cli::LaunchOrigin,
}

/// **Where one launch lands.**
///
/// Two answers, because there are two places a request can go and the whole of §7.59's second
/// ruling is which of them it goes to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Landing {
    /// A window of its own, in the running process.
    Window,
    /// A tab in the window the reader was last in.
    Tab,
}

/// **Where a launch lands, decided in the process that is already running** (§7.59, user ruling
/// 2026-09-11).
///
/// The whole rule, as a table over what the launch said and what the reader has asked for:
///
/// | who asked | flag | row says | lands as |
/// |---|---|---|---|
/// | anyone | `--new-window` | either | window |
/// | anyone | `--tab` | either | tab |
/// | Explorer's entry, `folio-here.cmd` | none | either | tab |
/// | a person starting Folio | none | `NewWindow` | window |
/// | a person starting Folio | none | `TabInLastWindow` | tab |
///
/// **A flag beats the row, and the row is never asked about a launch that means "a shell in this
/// folder".** Explorer's entry and `folio-here.cmd` are not somebody asking for Folio — they are
/// somebody asking for a terminal standing in a place they are already looking at, and a window of
/// its own for that is the answer to a question they did not ask. That is not an exception to the
/// row; it is the reason the wire carries who asked.
///
/// **`--new-window` beats `--tab`**, and that precedence is here rather than in the parser for the
/// reason this function exists at all: a frame can carry both whatever the command line refuses, so
/// the rule has to be total here anyway — and one rule stated once cannot come to disagree with
/// itself. A window is the answer that is always possible; a tab needs a window to put it in.
pub(crate) fn landing(request: &LaunchRequest, setting: bt_persist::LaunchOpensV1) -> Landing {
    if request.new_window {
        return Landing::Window;
    }
    if request.tab {
        return Landing::Tab;
    }
    match request.origin {
        cli::LaunchOrigin::Explorer | cli::LaunchOrigin::Here => Landing::Tab,
        cli::LaunchOrigin::Plain => match setting {
            bt_persist::LaunchOpensV1::NewWindow => Landing::Window,
            bt_persist::LaunchOpensV1::TabInLastWindow => Landing::Tab,
        },
    }
}

/// The token an origin crosses as. Short, from a closed set, and never free text — [`Refusal::
/// token`]'s rule at the one other field of this message that is not a path, a slug or a boolean.
const fn origin_token(origin: cli::LaunchOrigin) -> &'static str {
    match origin {
        cli::LaunchOrigin::Plain => "plain",
        cli::LaunchOrigin::Explorer => "explorer",
        cli::LaunchOrigin::Here => "here",
    }
}

/// One token, read back — or `None`, which is the answer to a word this build has no origin for.
fn origin_from_token(token: &str) -> Option<cli::LaunchOrigin> {
    match token {
        "plain" => Some(cli::LaunchOrigin::Plain),
        "explorer" => Some(cli::LaunchOrigin::Explorer),
        "here" => Some(cli::LaunchOrigin::Here),
        _ => None,
    }
}

impl LaunchRequest {
    /// **The request a command line becomes** — built from argv and from nothing else.
    ///
    /// `None` for a launch this channel cannot carry, which is exactly one case: a bare positional
    /// that is not a folder. `folio notes.md` asks for a document in a preview, and the wire has
    /// three declared fields with no room for a fourth — see this module's header for why that is a
    /// rule rather than an omission. Such a launch opens its own window, as it did before this
    /// channel existed.
    ///
    /// A positional that **is** a folder is folded into `cwd` here rather than sent as a field of
    /// its own, and the rule for doing so is [`cli::resolve`]'s and not a second one: the flag wins
    /// when both are given, and the two forms mean the same place.
    ///
    /// **`here` is this process's own working directory, and it is what makes `folio .` mean the
    /// same thing warm as it does cold** (review C-6, 2026-09-11). A folder that was **named** is
    /// resolved against it before it goes on the wire; a folder that was **not** named stays
    /// `None`. That distinction is the whole of it, and it is not a crack in `cli.rs`'s rule —
    /// that rule is *never inherit the process directory when nothing was asked for*, and this is
    /// *resolve the thing that was asked for*. Without it `folio .` opened a shell in this folder
    /// on a cold machine and printed «There is no .» on a warm one, naming a path in a spelling the
    /// person never typed.
    pub(crate) fn from_cli(
        request: &cli::CliRequest,
        kind: impl Fn(&Path) -> cli::PathKind,
        here: Option<&Path>,
    ) -> Option<Self> {
        let positional = match request.path.as_deref() {
            None => None,
            Some(path) if kind(path) == cli::PathKind::Directory => Some(path.to_path_buf()),
            Some(_) => return None,
        };
        let named = request.cwd.clone().or(positional);
        Some(Self {
            cwd: named.map(|folder| cli::absolute_from(here, &folder)),
            profile: request.profile.clone(),
            new_window: request.new_window,
            tab: request.tab,
            origin: request.origin,
        })
    }

    /// The line this request crosses as.
    #[must_use]
    fn encode(&self) -> String {
        let mut value = serde_json::Map::new();
        value.insert("v".to_owned(), WIRE_VERSION.into());
        if let Some(cwd) = &self.cwd {
            value.insert("cwd".to_owned(), cwd.to_string_lossy().into_owned().into());
        }
        if let Some(profile) = &self.profile {
            value.insert("profile".to_owned(), profile.clone().into());
        }
        value.insert("new".to_owned(), self.new_window.into());
        value.insert("tab".to_owned(), self.tab.into());
        value.insert("from".to_owned(), origin_token(self.origin).into());
        serde_json::Value::Object(value).to_string()
    }

    /// One line, read back — or `None`, which is the answer to every kind of nonsense.
    ///
    /// Deliberately total and deliberately unforgiving, which is [`crate::attention_wire::Message::
    /// decode`]'s rule word for word: a line that is not exactly one JSON object with a version this
    /// build knows and fields inside their bounds is not a launch that arrived slightly wrong, it is
    /// a line from something that is not `folio.exe`.
    #[must_use]
    fn decode(line: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
        let object = value.as_object()?;
        if object.get("v")?.as_u64()? != WIRE_VERSION {
            return None;
        }
        let bounded = |key: &str, bound: usize| -> Option<Option<String>> {
            let Some(value) = object.get(key) else {
                return Some(None);
            };
            let text = value.as_str()?;
            // Non-empty, inside its bound, and nothing in it that moves text about rather than
            // being text. The third clause is the attention wire's and is owed here for a reason of
            // this channel's own: what a refused folder becomes is a sentence on somebody's
            // console, and a control byte in it would be a line of that console written by whoever
            // sent the frame.
            (!text.is_empty() && text.len() <= bound && !text.chars().any(char::is_control))
                .then(|| Some(text.to_owned()))
        };
        Some(Self {
            cwd: bounded("cwd", MAX_FOLDER_BYTES)?.map(PathBuf::from),
            profile: bounded("profile", MAX_PROFILE_BYTES)?,
            new_window: object.get("new")?.as_bool()?,
            tab: object.get("tab")?.as_bool()?,
            // **Required, and a word from the closed set or nothing.** An absent key is not read as
            // `Plain`: this build writes the key on every frame it sends, so a frame without it was
            // written by something that is not this build — and the field decides whether a
            // reader's own row is consulted at all.
            origin: origin_from_token(object.get("from")?.as_str()?)?,
        })
    }

    /// Whether this request could cross at all — the bounds, asked before a byte reaches a pipe.
    ///
    /// The other end of [`Self::decode`]'s own rule: a frame past the bound must never be written,
    /// because the endpoint would drop it and the person would be left with neither a window nor a
    /// word. A launch that fails this opens its own window.
    fn is_sayable(&self) -> bool {
        Self::decode(&self.encode()).as_ref() == Some(self)
    }
}

/// **Why a running Folio would not take a launch.**
///
/// Two variants, and they are answered in two completely different ways — which is the reason this
/// was a variant rather than a `bool` from the first day it had only one member.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Refusal {
    /// The folder is not a local directory that exists. Nothing opens and the launch says so on its
    /// own console.
    NoSuchFolder,
    /// **This Folio cannot serve a launch right now** (review C-2, 2026-09-11): its window thread
    /// is not coming round, it is in the middle of quitting, or it already holds as many waiting
    /// launches as it will hold.
    ///
    /// The client's answer to this one is to **open its own window**, which is the behaviour every
    /// Folio had before this channel existed. It exists because the alternative is the silence that
    /// was there before: a running Folio whose UI had stopped answered `Taken` from its listener
    /// thread in microseconds, so starting Folio again — the one recovery anybody reaches for —
    /// exited with code 0 and put nothing on the screen.
    NotServing,
}

impl Refusal {
    /// The token this refusal crosses as. Short, from a closed set, and never free text — the far
    /// end turns it back into a sentence out of this build's own table.
    const fn token(self) -> &'static str {
        match self {
            Self::NoSuchFolder => "folder",
            Self::NotServing => "busy",
        }
    }

    fn from_token(token: &str) -> Option<Self> {
        match token {
            "folder" => Some(Self::NoSuchFolder),
            "busy" => Some(Self::NotServing),
            _ => None,
        }
    }
}

/// **Whether the running Folio will take this request**, asked identically at both ends.
///
/// The one machine question on this wire, and the door it goes through is the door a path printed
/// into a pane goes through: drive-rooted, nameable by this filesystem, no NUL — and, because what
/// this path is for is a shell standing in it, a directory that is actually there.
pub(crate) fn accept(request: &LaunchRequest) -> Result<(), Refusal> {
    let Some(cwd) = request.cwd.as_deref() else {
        return Ok(());
    };
    if bt_transcript::paths::is_local_absolute_path(cwd) && cwd.is_dir() {
        Ok(())
    } else {
        Err(Refusal::NoSuchFolder)
    }
}

/// The reply a server writes, and the client reads back.
///
/// **It no longer carries a process id, and that is the point** (review C-7, 2026-09-11). It used
/// to: the client needed one to name in `AllowSetForegroundWindow`, and the obvious place to put it
/// was the reply. But `AllowSetForegroundWindow((DWORD)-1)` is `ASFW_ANY` — *every process on this
/// machine may take the foreground* — so a field a peer fills in was a peer choosing what a freshly
/// started `folio.exe` did with the foreground rights it actually holds. The client asks the kernel
/// who is on the other end of the pipe instead (`bt_platform::launch_pipe::hand_over`), which is a
/// fact nobody on the wire can write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Reply {
    Taken,
    Refused(Refusal),
}

impl Reply {
    #[must_use]
    fn encode(self) -> String {
        let mut value = serde_json::Map::new();
        value.insert("v".to_owned(), WIRE_VERSION.into());
        match self {
            Self::Taken => {
                value.insert("ok".to_owned(), true.into());
            }
            Self::Refused(refusal) => {
                value.insert("ok".to_owned(), false.into());
                value.insert("why".to_owned(), refusal.token().into());
            }
        }
        serde_json::Value::Object(value).to_string()
    }

    #[must_use]
    fn decode(line: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
        let object = value.as_object()?;
        if object.get("v")?.as_u64()? != WIRE_VERSION {
            return None;
        }
        if object.get("ok")?.as_bool()? {
            return Some(Self::Taken);
        }
        Some(Self::Refused(Refusal::from_token(
            object.get("why")?.as_str()?,
        )?))
    }
}

// ---------------------------------------------------------------------------
// The process that is already running
// ---------------------------------------------------------------------------

static ENDPOINT: OnceLock<Option<LaunchPipe>> = OnceLock::new();

/// **Everything waiting for the window thread, and everything promised to somebody.**
///
/// One lock over both numbers, because the bound is about their sum and two atomics could be read
/// a microsecond apart and add up to a ninth launch.
static INBOX: Mutex<Inbox> = Mutex::new(Inbox {
    landed: Vec::new(),
    reserved: 0,
});

/// The parked launches and the count of admissions that have been promised a place among them.
struct Inbox {
    landed: Vec<LaunchRequest>,
    reserved: usize,
}

/// **How many launches this Folio will promise to open before it starts saying no.**
///
/// Eight, and it is small on purpose: the only thing that produces one of these is a person
/// starting `folio.exe`, and eight of them queued means the window thread has not turned since the
/// eighth double-click.
///
/// **Full means refused, and it used to mean the oldest was thrown away** (review C-2, 2026-09-11).
/// Eviction is the attention wire's rule and it is right there, where the thing being dropped is a
/// notification nobody is waiting on. Here it was wrong in the worst way available: the evicted
/// launch had already been told `Taken`, so its process exited with code 0 having promised a person
/// a window that no longer existed anywhere. A refusal costs that person a window of their own,
/// which is what they would have had before this channel existed.
const INBOX_BOUND: usize = 8;

/// **One launch, from the moment it is admitted to the moment the window thread has it.**
///
/// The object review C-2/C-4/C-5 says was missing. It exists only if this Folio said it could serve
/// the launch, it holds the place in the inbox that promise was made on, and it carries the request
/// as it was decided — nothing downstream re-reads the line or re-asks the filesystem.
///
/// **Dropping it releases the place.** That is the whole reason the reservation lives in a value
/// with a destructor rather than in a counter somebody has to remember to decrement: every way a
/// conversation can end after the reply is written — a client that never confirmed, a write that
/// did not land, a frame that was not [`bt_platform::launch_pipe::CONFIRM`] — ends by dropping this,
/// and none of them has to know that a reservation was ever taken.
pub(crate) struct Admission {
    request: LaunchRequest,
    /// Whether the place has already been spent, which [`park`] sets and `Drop` reads. Two states
    /// of one fact and not two fields, because "reserved" and "landed" are the same place.
    spent: bool,
}

impl Admission {
    /// Take a place in the inbox for this request, or `None` when there is none to take.
    fn reserve(request: LaunchRequest) -> Option<Self> {
        let mut inbox = INBOX.lock().unwrap_or_else(PoisonError::into_inner);
        if inbox.landed.len() + inbox.reserved >= INBOX_BOUND {
            return None;
        }
        inbox.reserved += 1;
        Some(Self {
            request,
            spent: false,
        })
    }
}

impl Drop for Admission {
    fn drop(&mut self) {
        if self.spent {
            return;
        }
        let mut inbox = INBOX.lock().unwrap_or_else(PoisonError::into_inner);
        inbox.reserved = inbox.reserved.saturating_sub(1);
    }
}

/// **Whether this Folio is still taking launches.**
///
/// Set false the moment quitting passes the point where a window could still be opened, and set
/// true again if the quit is abandoned — see `crate::main`'s `about_to_wait_inner`, which mirrors
/// the window thread's own state into it once per turn. A flag rather than a question because the
/// thing that asks is the listener thread, which owns nothing of the window thread's.
static ADMITTING: AtomicBool = AtomicBool::new(true);

/// Say whether this process is still in a position to open something for somebody.
pub(crate) fn set_admitting(admitting: bool) {
    ADMITTING.store(admitting, Ordering::Relaxed);
}

/// **Whether a launch arriving now could actually be served** (review C-2, 2026-09-11).
///
/// Three facts, and a launch is admitted only if all three hold. None of them is new — what was
/// missing is that nobody asked before answering `Taken`:
///
/// * this process is not retiring ([`ADMITTING`]), because past that point every window is hidden
///   and the request would be parked in a process that is leaving;
/// * the window thread can be expected to come round inside the launch's own budget
///   ([`crate::hang_watch::window_thread_can_serve`]), because a request parked for a wedged loop
///   is a person told "done" and shown nothing;
/// * there is a place in the inbox — which the [`Admission`] itself answers, by taking one.
///
/// `window_thread_can_serve` is handed in rather than asked for here, which is [`cli::resolve`]'s
/// own shape and is owed for its reason: it is the one impure input, it is a fact about a thread
/// that does not exist in a test process, and the rule this function *is* has to be readable
/// without one.
fn admit(request: LaunchRequest, window_thread_can_serve: bool) -> Option<Admission> {
    if !ADMITTING.load(Ordering::Relaxed) || !window_thread_can_serve {
        return None;
    }
    Admission::reserve(request)
}

/// Open this process's launch endpoint, once, and start parking what arrives.
///
/// `wake` is called on the listener thread and must do nothing but nudge the loop.
///
/// A failure here is not fatal and is not reported to the user: no endpoint means a second launch
/// finds no door and opens its own window, which is where every machine was before this slice. The
/// one thing it must never do is fall back to a weaker endpoint — see `bt_platform::launch_pipe`.
pub(crate) fn open(
    directory: &Path,
    wake: impl Fn() + Send + Sync + 'static,
) -> Option<&'static LaunchPipe> {
    ENDPOINT
        .get_or_init(|| {
            LaunchPipe::start(
                directory,
                |line| {
                    let request = LaunchRequest::decode(line)?;
                    // **Decided once, here, and carried** (review C-5). The admitted request goes
                    // to `commit` as a value; there is no second decode and no second `accept`, so
                    // a folder deleted in the middle of the conversation cannot turn a launch the
                    // client was told about into nothing at all.
                    let decision = match accept(&request) {
                        Err(refusal) => bt_platform::launch_pipe::Decision {
                            reply: Reply::Refused(refusal).encode(),
                            admitted: None,
                        },
                        Ok(()) => match admit(
                            request,
                            crate::hang_watch::window_thread_can_serve(
                                bt_platform::launch_pipe::HANDOVER_BUDGET,
                            ),
                        ) {
                            Some(admission) => bt_platform::launch_pipe::Decision {
                                reply: Reply::Taken.encode(),
                                admitted: Some(admission),
                            },
                            None => bt_platform::launch_pipe::Decision {
                                reply: Reply::Refused(Refusal::NotServing).encode(),
                                admitted: None,
                            },
                        },
                    };
                    Some(decision)
                },
                move |admission| {
                    park(admission);
                    wake();
                },
            )
            .ok()
        })
        .as_ref()
}

/// Spend an admission's place on the request it was taken for.
fn park(mut admission: Admission) {
    let mut inbox = INBOX.lock().unwrap_or_else(PoisonError::into_inner);
    admission.spent = true;
    inbox.reserved = inbox.reserved.saturating_sub(1);
    inbox.landed.push(admission.request.clone());
}

/// Every launch that has arrived since the last time the window thread looked.
#[must_use]
pub(crate) fn take() -> Vec<LaunchRequest> {
    std::mem::take(&mut INBOX.lock().unwrap_or_else(PoisonError::into_inner).landed)
}

// ---------------------------------------------------------------------------
// The process that has just been started
// ---------------------------------------------------------------------------

/// **What a second launch does instead of opening a window.**
///
/// `Some(code)` is this process's whole life: the request has been handed over, or refused with a
/// sentence already on the console, and there is nothing left for it to do. `None` is every reason
/// to carry on and open a window — no endpoint, nobody listening, an answer that never came, an
/// answer this build could not read, or a command line this wire has no field for.
///
/// **Every one of those reasons is the same answer on purpose.** A launch that could not reach the
/// running Folio must never leave the person with nothing; the fallback is the behaviour every
/// Folio had before this channel existed, and it is one branch rather than five so that a new way
/// of failing cannot arrive with a new way of doing nothing.
pub(crate) fn hand_over(
    directory: &Path,
    argv: &cli::CliRequest,
    say: impl Fn(&str),
) -> Option<i32> {
    let request = LaunchRequest::from_cli(
        argv,
        cli::machine_path_kind,
        std::env::current_dir().ok().as_deref(),
    )?;
    if !request.is_sayable() {
        return None;
    }
    let endpoint = bt_platform::launch_pipe::endpoint_for(directory)?;
    let mut answer = None;
    bt_platform::launch_pipe::hand_over(&endpoint, &request.encode(), |server, line| {
        answer = Reply::decode(line);
        // **Inside the conversation, because the running Folio has not acted yet.** The pipe is
        // still open, and the process on the other end acts only once this end has confirmed —
        // which is the only moment at which this grant is both possible (this process still owns
        // the foreground) and useful (nothing has tried to take it yet).
        //
        // **`server` and never a number out of the reply** (review C-7): it is the pid the kernel
        // named for the far end of this pipe, which is the one spelling of "who am I talking to"
        // that the thing being talked to cannot write.
        if answer == Some(Reply::Taken) {
            bt_platform::hotkey::allow_foreground_for(server);
        }
    })
    .ok()?;
    match answer? {
        Reply::Taken => Some(0),
        // **The running Folio said it could not serve this, so this process does** (review C-2).
        // `None` is the same word every other "carry on and open a window" path answers with, and
        // that is deliberate: a new way of being refused must not arrive with a new way of doing
        // nothing.
        Reply::Refused(Refusal::NotServing) => None,
        Reply::Refused(Refusal::NoSuchFolder) => {
            // **[`cli::CliRefusal::NoSuchPath`] and not `NoSuchFolder`**, and the
            // difference is the second half of each sentence rather than the
            // first. `NoSuchFolder` ends «This pane opened where a new one
            // would», which is true of a cold launch and a lie here: nothing
            // opened at all. `NoSuchPath` says «There is no <path>» and stops,
            // which is the whole of what happened — so this path adds no string
            // to the table and tells no half-truth to get away with it.
            let folder = request.cwd.unwrap_or_default();
            say(&cli::CliRefusal::NoSuchPath(folder).notice());
            Some(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(list: &[&str]) -> cli::CliRequest {
        cli::parse(list.iter().map(std::ffi::OsString::from))
            .expect("this command line was meant to parse")
    }

    /// Every path is a folder, so `from_cli`'s fold can be exercised without a disk.
    fn all_folders(_: &Path) -> cli::PathKind {
        cli::PathKind::Directory
    }

    /// The working directory a test launch was typed in. Named, so that the one test about
    /// resolving a relative folder is the only place it means anything.
    const HERE: &str = r"D:\Developer\Ledger";

    /// `from_cli` with this module's two fixtures, since every call but one wants both.
    fn from(list: &[&str]) -> Option<LaunchRequest> {
        LaunchRequest::from_cli(&argv(list), all_folders, Some(Path::new(HERE)))
    }

    /// **RED (§7.59) — the message is built from argv exactly, and it carries three fields.**
    ///
    /// The whole of the wire's declaration, held as a property of one function: what a second
    /// launch says is the folder, the profile id and the window switch it was given, and nothing
    /// else it was given. Before this slice there was no message at all.
    ///
    /// MUTATION: read the profile out of anywhere but `argv`, or add a fourth field, and the
    /// round trip below stops being an equality.
    #[test]
    fn the_message_is_the_command_line_and_the_three_fields_it_declares() {
        let request = LaunchRequest::from_cli(
            &argv(&[
                "--cwd",
                r"D:\Developer",
                "--profile",
                "winps",
                "--new-window",
            ]),
            all_folders,
            Some(Path::new(HERE)),
        )
        .expect("a command line with no document is one this wire can carry");
        assert_eq!(
            request,
            LaunchRequest {
                cwd: Some(PathBuf::from(r"D:\Developer")),
                profile: Some("winps".to_owned()),
                new_window: true,
                tab: false,
                origin: cli::LaunchOrigin::Plain,
            }
        );
        assert_eq!(
            LaunchRequest::decode(&request.encode()).as_ref(),
            Some(&request),
            "and it reads back as itself"
        );
        assert_eq!(
            from(&[]),
            Some(LaunchRequest::default()),
            "an empty command line is a request that names nothing and still crosses"
        );
    }

    /// **RED — a bare folder is the same request `--cwd` makes, and a document is not one this
    /// wire can carry.**
    ///
    /// The fold is [`cli::resolve`]'s rule and not a second one — the flag wins when both are
    /// given — and the refusal is the header's: three declared fields, and a fourth for a document
    /// is a decision this channel did not take. A launch naming a document opens its own window,
    /// which is what it did before.
    #[test]
    fn a_positional_folder_is_a_cwd_and_a_positional_document_is_not_carried() {
        assert_eq!(
            from(&[r"D:\Developer"]).expect("a folder crosses").cwd,
            Some(PathBuf::from(r"D:\Developer"))
        );
        assert_eq!(
            from(&["--cwd", r"D:\Developer", r"D:\Other"])
                .expect("a folder crosses")
                .cwd,
            Some(PathBuf::from(r"D:\Developer")),
            "the flag said where to open, so the positional is not the place"
        );
        assert_eq!(
            LaunchRequest::from_cli(
                &argv(&[r"D:\a\notes.md"]),
                |_| cli::PathKind::File,
                Some(Path::new(HERE))
            ),
            None,
            "a document has no field on this wire"
        );
    }

    /// **RED — the folder goes through the door a printed path goes through, and it must be
    /// there.**
    ///
    /// Four refusals and one acceptance, and each refusal is a different way of not being a place a
    /// shell could stand in: a relative name, a name that is nowhere, a file, and a name that is
    /// not local at all. The acceptance is a directory that exists, which is the only thing this
    /// field is for.
    ///
    /// MUTATION: drop the `is_local_absolute_path` half and the share below is accepted; drop the
    /// `is_dir` half and the file is.
    #[test]
    fn only_a_local_directory_that_exists_is_taken() {
        let here = std::env::temp_dir();
        let file = here.join(format!("bt-app-launch-wire-{}.txt", std::process::id()));
        std::fs::write(&file, b"x").expect("write a fixture into the scratch directory");
        let asking = |cwd: Option<PathBuf>| {
            accept(&LaunchRequest {
                cwd,
                ..LaunchRequest::default()
            })
        };
        assert_eq!(asking(Some(here.clone())), Ok(()));
        assert_eq!(asking(None), Ok(()), "a launch that named no folder is one");
        assert_eq!(asking(Some(file.clone())), Err(Refusal::NoSuchFolder));
        assert_eq!(
            asking(Some(here.join("no-such-folder-at-all"))),
            Err(Refusal::NoSuchFolder)
        );
        assert_eq!(
            asking(Some(PathBuf::from("relative"))),
            Err(Refusal::NoSuchFolder),
            "a command line is written in absolute paths; a relative one names \
             the folder folio.exe happens to live in"
        );
        assert_eq!(
            asking(Some(PathBuf::from(r"\\server\share"))),
            Err(Refusal::NoSuchFolder)
        );
        let _ = std::fs::remove_file(&file);
    }

    /// **RED — a line past a field's bound, or with a control byte in it, is not a request.**
    ///
    /// The bounds are checked on the way in as well as on the way out, which is the attention
    /// wire's rule: this end has no cause to trust that the far end applied them. The control-byte
    /// clause is owed by this channel in particular — a refused folder becomes a sentence on
    /// somebody's console, and a `\r` inside it would be a line of that console written by whoever
    /// sent the frame.
    #[test]
    fn a_field_past_its_bound_or_carrying_a_control_byte_is_not_a_request() {
        let long = "C:\\".to_owned() + &"a".repeat(MAX_FOLDER_BYTES);
        let refused = [
            format!(r#"{{"v":2,"new":false,"tab":false,"from":"plain","cwd":"{long}"}}"#),
            format!(
                r#"{{"v":2,"new":false,"tab":false,"from":"plain","profile":"{}"}}"#,
                "p".repeat(MAX_PROFILE_BYTES + 1)
            ),
            r#"{"v":2,"new":false,"tab":false,"from":"plain","cwd":"C:\\a\rb"}"#.to_owned(),
            r#"{"v":2,"new":false,"tab":false,"from":"plain","cwd":""}"#.to_owned(),
            r#"{"v":3,"new":false,"tab":false,"from":"plain"}"#.to_owned(),
            // **The frame a 0.2.5 `folio.exe` writes**, dropped rather than read with the two
            // missing keys filled in from defaults — see [`WIRE_VERSION`]. Its `"new":false`
            // meant *open a tab*, and this build would read the same bytes as *ask the row*.
            r#"{"v":1,"new":false}"#.to_owned(),
            r#"{"v":1,"new":true,"cwd":"C:\\Users"}"#.to_owned(),
            // A v2 frame missing each of the three keys that say where it lands.
            r#"{"v":2,"tab":false,"from":"plain"}"#.to_owned(),
            r#"{"v":2,"new":false,"from":"plain"}"#.to_owned(),
            r#"{"v":2,"new":false,"tab":false}"#.to_owned(),
            // And one naming an origin this build has none of — the one field here whose value
            // decides whether the reader's own row is consulted at all.
            r#"{"v":2,"new":false,"tab":false,"from":"somewhere else"}"#.to_owned(),
            r#"{"v":2,"new":false,"tab":false,"from":true}"#.to_owned(),
            r#"{"v":2}"#.to_owned(),
            "not json at all".to_owned(),
            String::new(),
        ];
        for line in refused {
            assert_eq!(
                LaunchRequest::decode(&line),
                None,
                "{line} is not a request this build understands"
            );
        }
        assert!(
            LaunchRequest::decode(
                r#"{"v":2,"new":true,"tab":false,"from":"explorer","cwd":"C:\\Users","profile":"pwsh"}"#
            )
            .is_some(),
            "and a well-formed one still is"
        );
        assert!(
            !LaunchRequest {
                cwd: Some(PathBuf::from(long)),
                ..LaunchRequest::default()
            }
            .is_sayable(),
            "a request past the bound never reaches a pipe"
        );
    }

    /// **RED (§7.59, user ruling 2026-09-11) — the whole decision, every cell of it.**
    ///
    /// Three origins × three ways of saying how to land × two settings, written out rather than
    /// generated, because what is being pinned is a *ruling* and a table that computed its own
    /// expectations would be the implementation asserting itself.
    ///
    /// MUTATIONS: read the setting before the flags and `--tab` stops beating `NewWindow`; read the
    /// setting for an Explorer launch and a right-click grows a whole window; drop the origin arm
    /// and `folio-here.cmd` — which is what VS Code's external-terminal setting runs — opens a
    /// window per terminal.
    #[test]
    fn where_a_launch_lands_is_the_origin_the_flag_and_the_row() {
        use bt_persist::LaunchOpensV1::{NewWindow, TabInLastWindow};
        use cli::LaunchOrigin::{Explorer, Here, Plain};

        let ask = |origin, new_window, tab, setting| {
            landing(
                &LaunchRequest {
                    origin,
                    new_window,
                    tab,
                    ..LaunchRequest::default()
                },
                setting,
            )
        };
        for origin in [Plain, Explorer, Here] {
            for setting in [NewWindow, TabInLastWindow] {
                assert_eq!(
                    ask(origin, true, false, setting),
                    Landing::Window,
                    "--new-window is the reader saying so out loud: {origin:?} {setting:?}"
                );
                assert_eq!(
                    ask(origin, false, true, setting),
                    Landing::Tab,
                    "--tab is the reader saying so out loud: {origin:?} {setting:?}"
                );
                assert_eq!(
                    ask(origin, true, true, setting),
                    Landing::Window,
                    "a frame that says both is answered once and always the same way, because a \
                     window is the answer that is always possible: {origin:?} {setting:?}"
                );
            }
            let asked_for = match origin {
                Plain => [Landing::Window, Landing::Tab],
                // "A shell in this folder" is not "another Folio", so the row is never put to it.
                Explorer | Here => [Landing::Tab, Landing::Tab],
            };
            assert_eq!(
                ask(origin, false, false, NewWindow),
                asked_for[0],
                "{origin:?}"
            );
            assert_eq!(
                ask(origin, false, false, TabInLastWindow),
                asked_for[1],
                "{origin:?}"
            );
        }
    }

    /// **RED (review C-6, 2026-09-11) — `folio .` means the same folder warm as it does cold.**
    ///
    /// The regression this closes: a relative folder passed every gate on a cold machine, because
    /// `machine_path_kind` resolves it against the process's own working directory — and was
    /// refused on a warm one with «There is no .», naming a path in a spelling nobody typed.
    ///
    /// **`None` stays `None`**, which is the half that keeps `cli.rs`'s own rule intact: never
    /// inherit the process directory when nothing was asked for.
    ///
    /// MUTATIONS: resolve an omitted folder too and `folio` with no arguments starts opening tabs
    /// in whatever folder the shell was standing in; resolve through `canonicalize` and the answer
    /// is a verbatim path that `is_local_absolute_path` refuses.
    #[test]
    fn a_relative_folder_is_resolved_before_it_goes_on_the_wire() {
        for spelling in [".", r"crates\..", r".\crates\.."] {
            assert_eq!(
                from(&["--cwd", spelling]).expect("a folder crosses").cwd,
                Some(PathBuf::from(HERE)),
                "{spelling} is the folder the launch was typed in"
            );
        }
        assert_eq!(
            from(&[r"..\bt-wt"]).expect("a folder crosses").cwd,
            Some(PathBuf::from(r"D:\Developer\bt-wt")),
            "a positional goes through the same door as the flag"
        );
        assert_eq!(
            from(&["--cwd", r"D:\Other"]).expect("a folder crosses").cwd,
            Some(PathBuf::from(r"D:\Other")),
            "a folder that was already absolute is left exactly as it was written"
        );
        assert_eq!(
            from(&[]).expect("an empty command line crosses").cwd,
            None,
            "nothing was named, so nothing is resolved — an inherited working directory here \
             would open every shortcut in whatever folder folio.exe lives in"
        );
        assert_eq!(
            LaunchRequest::from_cli(&argv(&["--cwd", "."]), all_folders, None)
                .expect("a folder crosses")
                .cwd,
            Some(PathBuf::from(".")),
            "a launch with no working directory of its own resolves nothing and is judged by the \
             same door it would have met anyway"
        );
    }

    /// **RED (review C-2, 2026-09-11) — a promise is only made if there is a place to keep it.**
    ///
    /// `Taken` used to be a syntax verdict: the listener thread answered it on nothing but the
    /// grammar and the folder, so a Folio whose window thread had stopped answered *yes* in
    /// microseconds and the person who started Folio again got exit code 0 and no window. And with
    /// eight launches already waiting, the ninth evicted the oldest — a launch that had already
    /// been told yes.
    ///
    /// What is held here is the inbox half, which is the half that stands up without a window
    /// thread: a place is reserved when a launch is admitted, released if the conversation ends
    /// without a confirmation, and never taken from a launch that has one.
    ///
    /// MUTATIONS: evict instead of refusing and the ninth admission succeeds; forget the
    /// reservation and nine admissions fit in eight places; drop `Admission`'s `Drop` and a
    /// conversation that ends after the reply permanently costs the inbox a place.
    /// **The two tests below own the process's inbox while they run.**
    ///
    /// [`INBOX`] and [`ADMITTING`] are one per process because the thing they describe is one per
    /// process, and `cargo test` runs this module's tests on several threads at once. Anything that
    /// counts places or flips the flag has to hold this first, or it is measuring another test.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    #[test]
    fn a_launch_is_admitted_only_against_a_place_that_is_kept_for_it() {
        let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = take();
        let held: Vec<Admission> = (0..INBOX_BOUND)
            .map(|_| {
                Admission::reserve(LaunchRequest::default())
                    .expect("an empty inbox has places in it")
            })
            .collect();
        assert!(
            Admission::reserve(LaunchRequest::default()).is_none(),
            "the place a ninth launch would take belongs to one of the eight already promised one"
        );
        drop(held);
        let admission = Admission::reserve(LaunchRequest::default())
            .expect("a conversation that ended without a confirmation left its place behind");
        park(admission);
        assert_eq!(
            take().len(),
            1,
            "a committed launch reaches the window thread"
        );
        assert_eq!(take().len(), 0, "and it reaches it once");
    }

    /// **RED (review C-2) — a Folio that is leaving refuses launches rather than swallowing them.**
    ///
    /// The retirement hole: the endpoint is a `OnceLock` and goes on answering after Quit has
    /// begun, while the arm of the loop that drains the inbox has already stopped running — so a
    /// launch admitted then disappeared permanently and its process exited 0.
    ///
    /// The other half is the window thread's: a loop that is not coming round inside the launch's
    /// own budget cannot be promised anything either, which is the case that made starting Folio
    /// again — the one recovery anybody reaches for — exit 0 and show nothing.
    ///
    /// MUTATIONS: admit regardless of the flag and the first refusal becomes an admission into a
    /// process that is closing its windows; ignore the liveness answer and the second becomes the
    /// silent loss the freeze report was about.
    #[test]
    fn a_folio_that_has_stopped_admitting_or_stopped_turning_takes_no_launch() {
        let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = take();
        set_admitting(false);
        let while_leaving = admit(LaunchRequest::default(), true);
        set_admitting(true);
        assert!(
            while_leaving.is_none(),
            "a process past the point where it can open a window still promised somebody one"
        );
        assert!(
            admit(LaunchRequest::default(), false).is_none(),
            "a window thread that is not coming round was answered `Taken` anyway, which is a \
             person told `done` and shown nothing"
        );
        assert!(
            admit(LaunchRequest::default(), true).is_some(),
            "and an ordinary run admits"
        );
        let _ = take();
    }

    /// **RED — the reply says yes or says which of the two kinds of no, and it names no process.**
    ///
    /// The process id used to be the load-bearing field. It is gone (review C-7, 2026-09-11): a
    /// pid out of a frame is a number the peer chose, `AllowSetForegroundWindow(u32::MAX)` is
    /// `ASFW_ANY`, and the client now asks the kernel who is on the other end of its own pipe
    /// instead. What is left is exactly the answer: taken, or not taken and why.
    ///
    /// MUTATION: put `pid` back in `encode` and the assertion below that no reply names a process
    /// goes red — which is the shape of the finding, not merely its symptom.
    #[test]
    fn the_reply_says_yes_or_which_no_and_never_names_a_process() {
        for reply in [
            Reply::Taken,
            Reply::Refused(Refusal::NoSuchFolder),
            Reply::Refused(Refusal::NotServing),
        ] {
            assert_eq!(Reply::decode(&reply.encode()), Some(reply));
            assert!(
                !reply.encode().contains("pid"),
                "a reply names a process id, which is a number the peer chose being handed to \
                 AllowSetForegroundWindow: {}",
                reply.encode()
            );
        }
        assert_eq!(
            Reply::decode(r#"{"v":2,"ok":true,"pid":4294967295}"#),
            Some(Reply::Taken),
            "an extra key is not a pid this build will act on — there is nowhere left to put it"
        );
        for line in [
            r#"{"v":2}"#,
            r#"{"v":2,"ok":false,"why":"something this build never sends"}"#,
            // The version this build no longer speaks, and one it never did.
            r#"{"v":1,"ok":true,"pid":1}"#,
            r#"{"v":3,"ok":true,"pid":1}"#,
            "",
        ] {
            assert_eq!(Reply::decode(line), None, "{line} is not an answer");
        }
    }
}
