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
//! carries **three declared fields and nothing else** — a folder, a profile id, and whether the
//! launch asked for a window of its own. There is no room in it for a command to run, a document to
//! open or a name to type, because a channel that carried any of those would be a channel worth
//! attacking: it is answered by a process that has a terminal in it.
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
use std::sync::{Mutex, OnceLock, PoisonError};

use bt_platform::launch_pipe::LaunchPipe;

use crate::cli;

/// The wire's version, and it is [`crate::attention_wire`]'s reason exactly: a `folio.exe` started
/// from a shortcut may be a different build from the one holding the data directory — a user who
/// upgraded while a window was open is the ordinary way that happens. A frame from a version this
/// build does not know is dropped rather than half-understood, and the launch that sent it opens
/// its own window, which is what every launch did before this channel existed.
const WIRE_VERSION: u64 = 1;

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
/// Three fields, and the shape is the ruling: a second `folio.exe` is asking the first for a tab
/// somewhere, or for a window. It is deliberately not a `CliRequest` — that type carries a bare
/// positional and a COM switch, neither of which this channel has any business carrying, and a
/// message that was "the command line" would grow a field every time the command line did.
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
    pub(crate) fn from_cli(
        request: &cli::CliRequest,
        kind: impl Fn(&Path) -> cli::PathKind,
    ) -> Option<Self> {
        let positional = match request.path.as_deref() {
            None => None,
            Some(path) if kind(path) == cli::PathKind::Directory => Some(path.to_path_buf()),
            Some(_) => return None,
        };
        Some(Self {
            cwd: request.cwd.clone().or(positional),
            profile: request.profile.clone(),
            new_window: request.new_window,
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
/// One variant, and it is a variant rather than a `bool` because the whole point of the reply
/// channel is that the second process can say *what* went wrong on the console it was typed at —
/// and because the day there are two reasons, a `bool` would have to be read alongside a second
/// call to tell them apart.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Refusal {
    /// The folder is not a local directory that exists.
    NoSuchFolder,
}

impl Refusal {
    /// The token this refusal crosses as. Short, from a closed set, and never free text — the far
    /// end turns it back into a sentence out of this build's own table.
    const fn token(self) -> &'static str {
        match self {
            Self::NoSuchFolder => "folder",
        }
    }

    fn from_token(token: &str) -> Option<Self> {
        match token {
            "folder" => Some(Self::NoSuchFolder),
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
/// The process id is in it because of what the client has to do with it: `SetForegroundWindow` is
/// refused unless the process holding the foreground has granted it, the process holding the
/// foreground is the one the user just started, and it can only name the grant's beneficiary if it
/// has been told who that is. See `bt_platform::launch_pipe`'s header for why the grant has to be
/// made before this end lets go.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Reply {
    Taken { by: u32 },
    Refused(Refusal),
}

impl Reply {
    #[must_use]
    fn encode(self) -> String {
        let mut value = serde_json::Map::new();
        value.insert("v".to_owned(), WIRE_VERSION.into());
        match self {
            Self::Taken { by } => {
                value.insert("ok".to_owned(), true.into());
                value.insert("pid".to_owned(), by.into());
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
            return Some(Self::Taken {
                by: u32::try_from(object.get("pid")?.as_u64()?).ok()?,
            });
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
static INBOX: Mutex<Vec<LaunchRequest>> = Mutex::new(Vec::new());

/// **How many launches may wait for the window thread before the oldest are dropped.**
///
/// Eight, and it is small on purpose: the only thing that produces one of these is a person
/// starting `folio.exe`, and eight of them queued means the window thread has not turned since the
/// eighth double-click. The **oldest** goes, because the newest is the one that is actually
/// happening — the attention wire's rule at a door where it matters less and costs nothing.
const INBOX_BOUND: usize = 8;

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
                    Some(
                        match accept(&request) {
                            Ok(()) => Reply::Taken {
                                by: std::process::id(),
                            },
                            Err(refusal) => Reply::Refused(refusal),
                        }
                        .encode(),
                    )
                },
                move |line| {
                    // **The same two questions, asked again**, and asking them twice rather than
                    // carrying an answer between the two closures is the point: nothing is parked
                    // here that was not answered `ok` up there, and the way to be sure of that is
                    // for both to reach the same predicate rather than for one to remember what the
                    // other decided.
                    let Some(request) = LaunchRequest::decode(line) else {
                        return;
                    };
                    if accept(&request).is_err() {
                        return;
                    }
                    park(request);
                    wake();
                },
            )
            .ok()
        })
        .as_ref()
}

fn park(request: LaunchRequest) {
    let mut inbox = INBOX.lock().unwrap_or_else(PoisonError::into_inner);
    if inbox.len() >= INBOX_BOUND {
        inbox.remove(0);
    }
    inbox.push(request);
}

/// Every launch that has arrived since the last time the window thread looked.
#[must_use]
pub(crate) fn take() -> Vec<LaunchRequest> {
    std::mem::take(&mut *INBOX.lock().unwrap_or_else(PoisonError::into_inner))
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
    let request = LaunchRequest::from_cli(argv, cli::machine_path_kind)?;
    if !request.is_sayable() {
        return None;
    }
    let endpoint = bt_platform::launch_pipe::endpoint_for(directory)?;
    let mut answer = None;
    bt_platform::launch_pipe::hand_over(&endpoint, &request.encode(), |line| {
        answer = Reply::decode(line);
        // **Inside the conversation, because the running Folio has not acted yet.** The pipe is
        // still open, and the process on the other end is waiting for this end to let go before it
        // opens anything — which is the only moment at which this grant is both possible (this
        // process still owns the foreground) and useful (nothing has tried to take it yet).
        if let Some(Reply::Taken { by }) = answer {
            bt_platform::hotkey::allow_foreground_for(by);
        }
    })
    .ok()?;
    match answer? {
        Reply::Taken { .. } => Some(0),
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
        )
        .expect("a command line with no document is one this wire can carry");
        assert_eq!(
            request,
            LaunchRequest {
                cwd: Some(PathBuf::from(r"D:\Developer")),
                profile: Some("winps".to_owned()),
                new_window: true,
            }
        );
        assert_eq!(
            LaunchRequest::decode(&request.encode()).as_ref(),
            Some(&request),
            "and it reads back as itself"
        );
        assert_eq!(
            LaunchRequest::from_cli(&argv(&[]), all_folders),
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
            LaunchRequest::from_cli(&argv(&[r"D:\Developer"]), all_folders)
                .expect("a folder crosses")
                .cwd,
            Some(PathBuf::from(r"D:\Developer"))
        );
        assert_eq!(
            LaunchRequest::from_cli(&argv(&["--cwd", r"D:\Developer", r"D:\Other"]), all_folders)
                .expect("a folder crosses")
                .cwd,
            Some(PathBuf::from(r"D:\Developer")),
            "the flag said where to open, so the positional is not the place"
        );
        assert_eq!(
            LaunchRequest::from_cli(&argv(&[r"D:\a\notes.md"]), |_| cli::PathKind::File),
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
            format!(r#"{{"v":1,"new":false,"cwd":"{long}"}}"#),
            format!(
                r#"{{"v":1,"new":false,"profile":"{}"}}"#,
                "p".repeat(MAX_PROFILE_BYTES + 1)
            ),
            r#"{"v":1,"new":false,"cwd":"C:\\a\rb"}"#.to_owned(),
            r#"{"v":1,"new":false,"cwd":""}"#.to_owned(),
            r#"{"v":2,"new":false}"#.to_owned(),
            r#"{"v":1}"#.to_owned(),
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
            LaunchRequest::decode(r#"{"v":1,"new":true,"cwd":"C:\\Users","profile":"pwsh"}"#)
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

    /// **RED — the reply says which process to hand the foreground to, or why nothing happened.**
    ///
    /// The process id is the load-bearing field: without it the second launch has nothing to name
    /// in `AllowSetForegroundWindow`, and the tab would open in a window that stays behind whatever
    /// the reader was looking at.
    #[test]
    fn the_reply_carries_the_process_id_or_the_reason() {
        for reply in [
            Reply::Taken { by: 4242 },
            Reply::Refused(Refusal::NoSuchFolder),
        ] {
            assert_eq!(Reply::decode(&reply.encode()), Some(reply));
        }
        for line in [
            r#"{"v":1,"ok":true}"#,
            r#"{"v":1,"ok":false,"why":"something this build never sends"}"#,
            r#"{"v":2,"ok":true,"pid":1}"#,
            "",
        ] {
            assert_eq!(Reply::decode(line), None, "{line} is not an answer");
        }
    }
}
