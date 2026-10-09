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
//! carries **seven declared fields and nothing else** — a folder, a profile id, whether the launch
//! asked for a window of its own, whether it asked for a tab, who started it, what an update's
//! rollback sent the launch to report (0.4.7 U-36: a word from a closed set, and for an unfinished
//! rollback the folder of its journal), and the environment `--with-environment` carries
//! (F-SWEEP-2-048: names and values the launcher's own shell already holds, laid over the
//! account's environment of the one tab the launch opens). There is no room in it for a command to
//! run, a document to open or a name to type, because a channel that carried any of those would be
//! a channel worth attacking: it is answered by a process that has a terminal in it. The
//! environment is not one of those: it is what a shell started from the launcher's own terminal
//! would have had anyway, and it reaches a shell and nothing else.
//!
//! # The report crosses because nothing else can carry it
//!
//! A start that a rollback sent (`--update-failed`, or the start a refused rescue build left with
//! *Update incomplete.*) learns what to report from its own pass (`crate::update_startup`), and
//! that pass has already retired a rolled-back journal under the transaction lock before this start
//! asks who holds the data directory — the order F-6 fixes. The word itself is only on the command
//! line, and the Folio that holds the data directory may be another copy with another installation
//! home. So the running Folio cannot read the report from the disk; the start's verdict crosses
//! instead, decided once on that side, as the landing is decided once on this one.
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
use crate::update_job::Failure;

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
///
/// **Still 2 since U-36, because the report is a key and never a value.** A v2 reader takes the keys
/// it knows and reads no other (it has since 0.3), so a key added beside them is ignored by every
/// earlier build and changes nothing it says; its absence is what every earlier sender writes, and
/// means what it always meant — nothing to report. A version bump instead would have made every
/// launch between a 0.4.6 and a later build open a second, non-writing window. The rule this keeps
/// is the converse: **a v2 grammar grows by new keys, never by new values of a known key** — a
/// value this build does not know drops the whole frame, as `from` always has.
const WIRE_VERSION: u64 = 2;

/// The key the report crosses as ([`Report`]), absent when there is nothing to report.
const REPORT_KEY: &str = "failed";

/// The key an unfinished rollback's journal folder crosses as, beside [`REPORT_KEY`] and only with
/// [`Report::Incomplete`].
const REPORT_FOLDER_KEY: &str = "failed_folder";

/// The key an unfinished update whose journal records no trial ever begun crosses as — `true`,
/// beside [`REPORT_KEY`] and only with [`Report::Incomplete`] (0.4.8 E4). A key, never a new value
/// of a known one ([`WIRE_VERSION`]'s rule): an earlier build ignores it and says *Update
/// incomplete.* as it always did.
const REPORT_UNTRIED_KEY: &str = "failed_untried";

/// The key the refusal of a journal write that another program's hold outlasted the window for
/// crosses as, beside [`REPORT_KEY`] with any report ([`Report::JournalHeld`], 0.4.8 E4); an
/// earlier build ignores it.
const REPORT_HELD_KEY: &str = "failed_journal_held";

/// The key `--with-environment`'s environment crosses as: an array of `[name, value]` pairs,
/// absent when the launch carried none (F-SWEEP-2-048). A key, never a new value of a known one
/// ([`WIRE_VERSION`]'s rule): an earlier build ignores it and opens the tab without it.
const ENVIRONMENT_KEY: &str = "env";

/// **The most bytes the refusal of a held journal may cross as** — an operating-system error
/// message, a sentence: the profile id's bound is too short for a translated one, and a folder's
/// is generous.
const MAX_REFUSAL_BYTES: usize = 512;

/// **What a start a rollback sent was told to report**, in the words it crosses the pipe in
/// (0.4.7 U-36).
///
/// The three of `crate::update_job::Failure`'s kinds that a start can carry to another Folio, and
/// no other. `TrialIncomplete` is deliberately not one of them: its card tells the reader that
/// *this* session is the update's trial, which is false of every process but the trial itself — and
/// a trial never hands itself over (`crate::update_trial::take_the_claim`). The kinds a driver
/// reports (`Unsupported`, `Stopped`) are this launch's own and never a start's.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Report {
    /// The previous version was put back (`Failure::RolledBack`).
    RolledBack,
    /// The update stopped before the new version ran, and the previous one was put back
    /// (`Failure::Interrupted`).
    Interrupted,
    /// Putting the previous version back did not finish; `folder` is where its journal is
    /// (`Failure::Incomplete`). A sender always names it — its own pass read the journal there —
    /// and the frame always carries it; it is `None` only once [`accept`] has taken away a
    /// folder that is not a local path, which leaves the report and drops the folder.
    /// `untried`: the journal records no trial ever begun ([`REPORT_UNTRIED_KEY`]).
    Incomplete {
        folder: Option<PathBuf>,
        untried: bool,
    },
    /// **Another program held the journal open past its holder's window**
    /// (`Failure::JournalHeld`, 0.4.8 E4): `error`, the system's refusal, over the report the
    /// journal itself makes ([`REPORT_HELD_KEY`]).
    JournalHeld { error: String, then: Box<Report> },
}

impl Report {
    /// The report a start carries for `failure`, or `None` for a failure no other Folio may be told.
    #[must_use]
    pub(crate) fn of(failure: &Failure) -> Option<Self> {
        match failure {
            Failure::RolledBack => Some(Self::RolledBack),
            Failure::Interrupted => Some(Self::Interrupted),
            Failure::JournalHeld { error, then } => Some(Self::JournalHeld {
                error: error.clone(),
                then: Box::new(Self::of(then)?),
            }),
            // A later Folio's unfinished update crosses as *Update
            // incomplete.* (0.4.8 E1): the key and its words are the ones every
            // build since 0.4.7 reads, and a new value of a known key would
            // drop the whole frame there ([`WIRE_VERSION`]'s rule). Whether this
            // session's writes are held is the sender's own and does not cross.
            Failure::Incomplete {
                folder, untried, ..
            } => Some(Self::Incomplete {
                folder: folder.clone(),
                untried: *untried,
            }),
            Failure::Newer { folder, .. } => Some(Self::Incomplete {
                folder: folder.clone(),
                untried: false,
            }),
            // The trial's own card (above), an update committed after its trial
            // ended (nothing failed), and a driver's own stops: none is a
            // start's to tell another Folio. A stand-in that stood down
            // beside the reserved trial reports nothing either: its card is
            // about this session, and the Folio it would cross to is that
            // very trial (0.4.8 E3).
            Failure::TrialIncomplete { .. }
            | Failure::BesideTheTrial { .. }
            | Failure::ChangesNotKept { .. }
            | Failure::Unsupported
            | Failure::Stopped(_) => None,
        }
    }

    /// The failure the update job is told, the same one the start itself would have shown.
    #[must_use]
    pub(crate) fn failure(&self) -> Failure {
        match self {
            Self::RolledBack => Failure::RolledBack,
            Self::Interrupted => Failure::Interrupted,
            Self::Incomplete { folder, untried } => Failure::Incomplete {
                folder: folder.clone(),
                held: false,
                untried: *untried,
            },
            Self::JournalHeld { error, then } => Failure::JournalHeld {
                error: error.clone(),
                then: Box::new(then.failure()),
            },
        }
    }

    /// **The report beneath a held journal's** — the one whose word crosses as [`REPORT_KEY`].
    fn beneath(&self) -> &Self {
        match self {
            Self::JournalHeld { then, .. } => then.beneath(),
            other => other,
        }
    }

    /// [`Self::beneath`], to change.
    fn beneath_mut(&mut self) -> &mut Self {
        match self {
            Self::JournalHeld { then, .. } => then.beneath_mut(),
            other => other,
        }
    }

    /// The refusal of a held journal over this report, if there is one.
    fn held(&self) -> Option<&str> {
        match self {
            Self::JournalHeld { error, .. } => Some(error),
            _ => None,
        }
    }

    /// The token this report crosses as — short and from a closed set, [`origin_token`]'s rule.
    fn token(&self) -> &'static str {
        match self.beneath() {
            Self::RolledBack => "rolled-back",
            Self::Interrupted => "interrupted",
            Self::Incomplete { .. } | Self::JournalHeld { .. } => "incomplete",
        }
    }

    /// A token, its folder, whether no trial began and the hold's refusal, read back: the folder
    /// and `untried` come with `incomplete` and with nothing else, a hold with any word, and
    /// anything else is not a report this build knows.
    fn from_token(
        token: &str,
        folder: Option<String>,
        untried: bool,
        held: Option<String>,
    ) -> Option<Self> {
        let report = match (token, folder, untried) {
            ("rolled-back", None, false) => Self::RolledBack,
            ("interrupted", None, false) => Self::Interrupted,
            ("incomplete", Some(folder), untried) => Self::Incomplete {
                folder: Some(PathBuf::from(folder)),
                untried,
            },
            _ => return None,
        };
        Some(match held {
            Some(error) => Self::JournalHeld {
                error,
                then: Box::new(report),
            },
            None => report,
        })
    }
}

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
/// Six fields, and the shape is the ruling: a second `folio.exe` is saying what it was asked for,
/// who asked and what it was sent to report, and the process that is already up decides where that
/// lands ([`landing`]). It is
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
    /// **What an update's rollback sent this launch to report** (U-36) — the start's own verdict
    /// (`crate::update_startup::failed`), never the command line's word re-read. The running
    /// Folio's update job is told it where the launch lands ([`Self::told`]).
    pub(crate) report: Option<Report>,
    /// **The environment `--with-environment` carries into the tab** (owner ruling 2026-10-05) —
    /// the starting process's own, read by [`hand_over`]; `None` for every other launch.
    pub(crate) environment: Option<cli::CarriedEnvironment>,
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
            report: None,
            // The command line says whether; the process says what — [`hand_over`] reads it.
            environment: None,
        })
    }

    /// **The request one start hands over**: its command line ([`Self::from_cli`]) and what its
    /// update pass sent it to report (`failed`, `crate::update_startup::failed`) — the one function
    /// [`hand_over`] builds its request with.
    pub(crate) fn of_start(
        request: &cli::CliRequest,
        failed: Option<&Failure>,
        kind: impl Fn(&Path) -> cli::PathKind,
        here: Option<&Path>,
    ) -> Option<Self> {
        Some(Self {
            report: failed.and_then(Report::of),
            ..Self::from_cli(request, kind, here)?
        })
    }

    /// **Tell the running Folio's update job what this launch reports**, once it has landed in
    /// `landed` (the window it opened or opened a tab in; `None` when none could be). Answers
    /// whether the job raised the report's card; `None` when there is nothing to report.
    pub(crate) fn told<W: Copy + Eq>(
        &self,
        job: &mut crate::update_job::Job<W>,
        landed: Option<W>,
    ) -> Option<bool> {
        let report = self.report.as_ref()?;
        Some(job.told_by_a_launch(report.failure(), landed))
    }

    /// **What `diagnostics.log` says a report came to**, read off the card the turn has placed:
    /// `raised` is [`Self::told`]'s answer, `landed` the window the launch landed in and `shown`
    /// the window the update card's presenter shows it in after the turn's card settle
    /// (`update_card::Shown::card`, which is what a window draws from). The line names where the
    /// card is, not what the job was asked to do.
    #[must_use]
    pub(crate) fn told_line<W: Copy + Eq>(
        raised: bool,
        landed: Option<W>,
        shown: Option<W>,
    ) -> &'static str {
        match (raised, shown) {
            (false, _) => {
                "Folio: update job — a launch handed over reports an earlier update's failure; the running update keeps the card"
            }
            (true, Some(window)) if Some(window) == landed => {
                "Folio: update job — a launch handed over reports an earlier update's failure; its card is up in the window the launch landed in"
            }
            (true, Some(_)) => {
                "Folio: update job — a launch handed over reports an earlier update's failure; its card is up in another window"
            }
            (true, None) => {
                "Folio: update job — a launch handed over reports an earlier update's failure; no window shows its card"
            }
        }
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
        // **Only when there is something to report**: a launch with nothing to report writes
        // exactly the frame every earlier build writes (see [`WIRE_VERSION`]).
        if let Some(report) = &self.report {
            value.insert(REPORT_KEY.to_owned(), report.token().into());
            if let Report::Incomplete { folder, untried } = report.beneath() {
                if let Some(folder) = folder {
                    value.insert(
                        REPORT_FOLDER_KEY.to_owned(),
                        folder.to_string_lossy().into_owned().into(),
                    );
                }
                if *untried {
                    value.insert(REPORT_UNTRIED_KEY.to_owned(), true.into());
                }
            }
            if let Some(error) = report.held() {
                value.insert(REPORT_HELD_KEY.to_owned(), error.into());
            }
        }
        // **Only when one was carried**, for the report's reason. A name or value that is not
        // text is written lossily here and so never reads back as itself: [`Self::is_sayable`]
        // refuses that launch rather than change what it carries.
        if let Some(environment) = &self.environment {
            let pairs = environment
                .pairs()
                .iter()
                .map(|(name, value)| {
                    serde_json::Value::Array(vec![
                        name.to_string_lossy().into_owned().into(),
                        value.to_string_lossy().into_owned().into(),
                    ])
                })
                .collect();
            value.insert(ENVIRONMENT_KEY.to_owned(), serde_json::Value::Array(pairs));
        }
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
            // **Optional, and from the closed set when present** ([`WIRE_VERSION`]'s rule): absent
            // is what every earlier sender writes and reads as nothing to report; a word this
            // build has no report for, a folder with the wrong word or without one, is not a frame
            // this build understands. Here the folder is only bounded, as `cwd` is here; whether it
            // is a place on this machine is [`accept`]'s question, asked of both paths alike.
            report: match (
                object.get(REPORT_KEY),
                bounded(REPORT_FOLDER_KEY, MAX_FOLDER_BYTES)?,
                object.get(REPORT_UNTRIED_KEY),
                bounded(REPORT_HELD_KEY, MAX_REFUSAL_BYTES)?,
            ) {
                (None, None, None, None) => None,
                (None, ..) => return None,
                (Some(token), folder, untried, held) => Some(Report::from_token(
                    token.as_str()?,
                    folder,
                    match untried {
                        None => false,
                        Some(untried) => untried.as_bool()?,
                    },
                    held,
                )?),
            },
            // **Optional, and every pair a variable a process could hold** when present: a name
            // that is not empty and holds no `=` past its first character (Windows keeps its
            // per-drive folders as `=C:`), and no NUL in a name or a value. Bounded by the frame
            // (`bt_platform::launch_pipe::MAX_FRAME_BYTES`), which is the field that can be long.
            environment: match object.get(ENVIRONMENT_KEY) {
                None => None,
                Some(pairs) => Some(cli::CarriedEnvironment::from_pairs(
                    pairs
                        .as_array()?
                        .iter()
                        .map(|pair| {
                            let [name, value] = pair.as_array()?.as_slice() else {
                                return None;
                            };
                            let (name, value) = (name.as_str()?, value.as_str()?);
                            let well_formed = !name.is_empty()
                                && !name.chars().skip(1).any(|character| character == '=')
                                && !name.contains('\0')
                                && !value.contains('\0');
                            well_formed.then(|| (name.into(), value.into()))
                        })
                        .collect::<Option<Vec<_>>>()?,
                )),
            },
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

    /// **Why the environment this launch carries cannot cross**, or `None` when it can or none was
    /// carried: past the endpoint's frame bound, or holding a name or value the wire cannot write
    /// as itself. Never cut to fit — a pane given half an environment is a pane that is quietly
    /// wrong ([`offer_start`] refuses the launch instead). Names the count and the sizes, never a
    /// variable's value.
    fn environment_refusal(&self) -> Option<String> {
        let environment = self.environment.as_ref()?;
        let bytes = self.encode().len();
        let limit = bt_platform::launch_pipe::MAX_FRAME_BYTES;
        let why = if bytes > limit {
            format!("{bytes} bytes on the wire, more than the launch endpoint's {limit}")
        } else if !self.is_sayable() {
            "a variable whose name or value the wire cannot write as itself".to_owned()
        } else {
            return None;
        };
        Some(format!(
            "Folio: {} — the environment of this launch ({} variables) cannot be handed to the \
             running Folio: {why}; nothing was opened",
            cli::WITH_ENVIRONMENT_FLAG,
            environment.pairs().len()
        ))
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

/// **Whether the running Folio will take this request, and the request it takes.**
///
/// The one machine question on this wire, and the one place it is asked: **every path that arrives
/// over the wire goes through [`is_a_local_path`]** — the door a path printed into a pane goes
/// through: drive-rooted, nameable by this filesystem, no NUL; never a share on another machine or
/// a device. There are two such paths, `cwd` and an unfinished rollback's journal folder
/// ([`Report::Incomplete`]); `profile` is a slug and no path. What fails it is answered by what the
/// path is for:
///
/// * **`cwd`** is where a shell will stand, so it must also be a directory that is there, and a
///   launch that names anything else is refused ([`Refusal::NoSuchFolder`]).
/// * **The report's folder** is only named on a card and handed to the file manager by its Show
///   folder (U-36 round 2), so one that is not local is taken away and **the report stays**: the
///   reader is still told the update is incomplete, on a card that names no folder. A report is
///   never dropped for its folder — losing it is the defect U-36 closed — and a share is never put
///   on a card whose button would hand this account's credentials to another machine's SMB server.
///   Existence is not asked: the journal's folder is not a place anything stands in.
pub(crate) fn accept(mut request: LaunchRequest) -> Result<LaunchRequest, Refusal> {
    if let Some(cwd) = request.cwd.as_deref()
        && !(is_a_local_path(cwd) && cwd.is_dir())
    {
        return Err(Refusal::NoSuchFolder);
    }
    if let Some(Report::Incomplete { folder, .. }) =
        request.report.as_mut().map(Report::beneath_mut)
        && folder.as_deref().is_some_and(|path| !is_a_local_path(path))
    {
        *folder = None;
    }
    Ok(request)
}

/// **The one rule every path on this wire meets** ([`accept`]): a local absolute path, by the gate
/// a printed path meets ([`bt_transcript::paths::is_local_absolute_path`]).
fn is_a_local_path(path: &Path) -> bool {
    bt_transcript::paths::is_local_absolute_path(path)
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
                    decide(line, || {
                        crate::hang_watch::window_thread_can_serve(
                            bt_platform::launch_pipe::HANDOVER_BUDGET,
                        )
                    })
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

/// **One line, answered** — the listener's whole decision, `None` for a line that is not a request.
///
/// **Decided once, here, and carried** (review C-5). The admitted request goes to `commit` as a
/// value; there is no second decode and no second `accept`, so a folder deleted in the middle of
/// the conversation cannot turn a launch the client was told about into nothing at all.
/// `window_thread_can_serve` is [`admit`]'s one impure input, asked only of a request whose folder
/// was accepted.
fn decide(
    line: &str,
    window_thread_can_serve: impl FnOnce() -> bool,
) -> Option<bt_platform::launch_pipe::Decision<Admission>> {
    let request = LaunchRequest::decode(line)?;
    Some(match accept(request) {
        Err(refusal) => bt_platform::launch_pipe::Decision {
            reply: Reply::Refused(refusal).encode(),
            admitted: None,
        },
        Ok(request) => match admit(request, window_thread_can_serve()) {
            Some(admission) => bt_platform::launch_pipe::Decision {
                reply: Reply::Taken.encode(),
                admitted: Some(admission),
            },
            None => bt_platform::launch_pipe::Decision {
                reply: Reply::Refused(Refusal::NotServing).encode(),
                admitted: None,
            },
        },
    })
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
///
/// **But the reason is not dropped** (0.4.8 D3): when a conversation was had, or tried, and did not
/// end in this process leaving, `gave_up` is handed the one line that says why — the OS error,
/// the running Folio's answer, or an answer this build cannot read ([`gave_up_line`]). The front
/// door has no log, so the caller keeps the line for `diagnostics.log`, beside the line that says
/// the window it opens saves nothing. A command line this wire has no field for is not a
/// hand-over that gave up, and says nothing.
///
/// **Only after the update pass** (`crate::update_startup`, 0.4.6 U-12): the
/// [`crate::update_startup::Admitted`] it asks for is made by that pass alone,
/// so this launch holds its installation's admission before it can be handed
/// anywhere.
///
/// **An owner-thread door** (`doors::LaunchHandOver`, §5.3 row 18): the window thread's one wait
/// before the loop exists, admitted only in `Starting`, minted in `main`.
pub(crate) fn hand_over(
    token: bt_platform::admission::WaitToken<'_, bt_platform::admission::doors::LaunchHandOver>,
    admitted: &crate::update_startup::Admitted,
    directory: &Path,
    argv: &cli::CliRequest,
    say: impl Fn(&str),
    gave_up: impl FnOnce(String),
) -> Option<i32> {
    let _ = token;
    // **What the pass sent this start to report crosses with it** (U-36): the
    // [`crate::update_startup::Admitted`] is the pass's own word for it.
    offer_start(
        directory,
        argv,
        admitted.failed().as_ref(),
        // **`--with-environment` carries this process's environment** — the launcher's, which
        // this start inherited (owner ruling 2026-10-05).
        argv.with_environment
            .then(cli::CarriedEnvironment::of_this_process),
        std::env::current_dir().ok().as_deref(),
        say,
        gave_up,
    )
}

/// [`hand_over`] past its door and its pass: this start's request — `argv`, what the pass sent it
/// to report (`failed`), the environment it carries (`environment`), its folder resolved against
/// `here` — offered to the Folio that holds `directory`, with [`hand_over`]'s answer and its
/// give-up line.
///
/// **An environment that cannot cross refuses the launch** (F-SWEEP-2-048): the line that says so
/// goes to this start's console and to `diagnostics.log` in `directory`, and the start leaves with
/// `2`, as a launch whose folder is not there does. It is not cut to fit, and it does not open a
/// window of its own, which could save nothing while the running Folio holds the data directory.
fn offer_start(
    directory: &Path,
    argv: &cli::CliRequest,
    failed: Option<&Failure>,
    environment: Option<cli::CarriedEnvironment>,
    here: Option<&Path>,
    say: impl Fn(&str),
    gave_up: impl FnOnce(String),
) -> Option<i32> {
    let request = LaunchRequest {
        environment,
        ..LaunchRequest::of_start(argv, failed, cli::machine_path_kind, here)?
    };
    if let Some(line) = request.environment_refusal() {
        say(&line);
        crate::diagnostics::append_note(&crate::diagnostics::log_path(directory), &line);
        return Some(2);
    }
    if !request.is_sayable() {
        return None;
    }
    let answer = match converse(directory, &request) {
        Ok(answer) => answer,
        Err(why) => {
            gave_up(gave_up_line(directory, &why));
            return None;
        }
    };
    let left = after_reply(request, answer, say);
    if left.is_none() {
        gave_up(gave_up_line(
            directory,
            &format!("it answered {}", answer.encode()),
        ));
    }
    left
}

/// **The one line a start whose hand-over gave up leaves in `diagnostics.log`** (0.4.8 D3):
/// which data directory's Folio it asked, and `why` — the OS error, the answer, or the reason no
/// conversation could be had.
#[must_use]
pub(crate) fn gave_up_line(directory: &Path, why: &str) -> String {
    format!(
        "Folio: launch hand-over — the Folio that holds {} did not take this launch ({why}); this \
         start opens a window of its own",
        directory.display()
    )
}

/// **The command line a person's start handed a rescue build, as the request it crosses in**
/// (0.4.8 E3): `handed` (`--then-launch`'s words) parsed as that start parsed them, its folder
/// resolved against `here` — the start's working directory, which the rescue build it started
/// inherits — and nothing to report, since the road decided no failure for it. `None` for a line
/// this wire cannot carry, as [`LaunchRequest::from_cli`] says (a document), or one that does not
/// parse.
#[must_use]
pub(crate) fn carried(handed: &[std::ffi::OsString], here: Option<&Path>) -> Option<LaunchRequest> {
    let line = cli::parse(handed.iter().cloned()).ok()?;
    LaunchRequest::from_cli(&line, cli::machine_path_kind, here).filter(LaunchRequest::is_sayable)
}

/// **A person's start a recovery carried, handed to the Folio that holds `directory`** (0.4.8
/// E3, `update_apply::carry_the_start`): one conversation on the launch endpoint, the one
/// [`hand_over`] has — from a road process's worker, which has no console to say a refusal on, so
/// the answer itself is returned. `None` when nobody answered.
pub(crate) fn hand_over_carried(directory: &Path, request: &LaunchRequest) -> Option<Reply> {
    converse(directory, request).ok()
}

/// **One conversation with the Folio that holds `directory`**: `request` sent, and its answer —
/// or why there is none: no endpoint can be named, the wire's own error (nobody listening, the
/// budget spent, a peer that is not a Folio), or an answer this build cannot read.
fn converse(directory: &Path, request: &LaunchRequest) -> Result<Reply, String> {
    let endpoint = bt_platform::launch_pipe::endpoint_for(directory)
        .ok_or_else(|| "no launch endpoint can be named for it here".to_owned())?;
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
        //
        // **The grant's answer is not read**, by the door's own rule: a refused grant is never
        // reported to a reader (nothing a person can do about a foreground lock), and the far end
        // activates itself whether or not it was granted — at worst its window opens behind.
        if answer == Some(Reply::Taken) {
            let _ = bt_platform::hotkey::allow_foreground_for(server);
        }
    })
    .map_err(|error| format!("{:?}: {error}", error.kind()))?;
    answer.ok_or_else(|| "it answered in words this build cannot read".to_owned())
}

/// **What the start does with the running Folio's answer** — `Some(code)` to leave, `None` to carry
/// on and open its own window ([`hand_over`]'s two words).
fn after_reply(request: LaunchRequest, answer: Reply, say: impl Fn(&str)) -> Option<i32> {
    match answer {
        Reply::Taken => Some(0),
        // **The running Folio said it could not serve this, so this process does** (review C-2).
        // `None` is the same word every other "carry on and open a window" path answers with, and
        // that is deliberate: a new way of being refused must not arrive with a new way of doing
        // nothing.
        Reply::Refused(Refusal::NotServing) => None,
        // **A launch with a report is never refused into nothing** (U-36). Nobody typed it at a
        // console — a lock holder started it to tell the reader what became of an update — so the
        // sentence below would reach no one and the report would be lost with it. It opens its own
        // window instead, as every launch the running Folio cannot take does, and that window
        // says both: the report on the update card, the gone folder on a cold launch's card.
        Reply::Refused(Refusal::NoSuchFolder) if request.report.is_some() => None,
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
    use crate::test_support::{host_path, host_spelling};

    fn argv(list: &[&str]) -> cli::CliRequest {
        cli::parse(list.iter().map(std::ffi::OsString::from))
            .expect("this command line was meant to parse")
    }

    /// Every path is a folder, so `from_cli`'s fold can be exercised without a disk.
    fn all_folders(_: &Path) -> cli::PathKind {
        cli::PathKind::Directory
    }

    /// A relative folder, joined with this host's separator.
    fn relative(names: &[&str]) -> String {
        names
            .iter()
            .collect::<PathBuf>()
            .to_string_lossy()
            .into_owned()
    }

    /// The working directory a test launch was typed in. Named, so that the one test about
    /// resolving a relative folder is the only place it means anything.
    fn typed_in() -> PathBuf {
        host_path(r"D:\Developer\Ledger")
    }

    /// `from_cli` with this module's two fixtures, since every call but one wants both.
    fn from(list: &[&str]) -> Option<LaunchRequest> {
        LaunchRequest::from_cli(&argv(list), all_folders, Some(typed_in().as_path()))
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
                host_spelling(r"D:\Developer").as_str(),
                "--profile",
                "winps",
                "--new-window",
            ]),
            all_folders,
            Some(typed_in().as_path()),
        )
        .expect("a command line with no document is one this wire can carry");
        assert_eq!(
            request,
            LaunchRequest {
                cwd: Some(host_path(r"D:\Developer")),
                profile: Some("winps".to_owned()),
                new_window: true,
                tab: false,
                origin: cli::LaunchOrigin::Plain,
                report: None,
                environment: None,
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
            from(&[host_spelling(r"D:\Developer").as_str()])
                .expect("a folder crosses")
                .cwd,
            Some(host_path(r"D:\Developer"))
        );
        assert_eq!(
            from(&[
                "--cwd",
                host_spelling(r"D:\Developer").as_str(),
                host_spelling(r"D:\Other").as_str(),
            ])
            .expect("a folder crosses")
            .cwd,
            Some(host_path(r"D:\Developer")),
            "the flag said where to open, so the positional is not the place"
        );
        assert_eq!(
            LaunchRequest::from_cli(
                &argv(&[host_spelling(r"D:\a\notes.md").as_str()]),
                |_| cli::PathKind::File,
                Some(typed_in().as_path())
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
        let file = bt_testpath::temp_path("bt-app-launch-wire").with_extension("txt");
        std::fs::write(&file, b"x").expect("write a fixture into the scratch directory");
        let asking = |cwd: Option<PathBuf>| {
            accept(LaunchRequest {
                cwd,
                ..LaunchRequest::default()
            })
            .map(drop)
        };
        assert_eq!(asking(Some(here.clone())), Ok(()));
        assert_eq!(asking(None), Ok(()), "a launch that named no folder is one");
        assert_eq!(asking(Some(file.clone())), Err(Refusal::NoSuchFolder));
        assert_eq!(
            asking(Some(bt_testpath::temp_path("no-such-folder-at-all"))),
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
        for spelling in [
            relative(&["."]),
            relative(&["crates", ".."]),
            relative(&[".", "crates", ".."]),
        ] {
            assert_eq!(
                from(&["--cwd", spelling.as_str()])
                    .expect("a folder crosses")
                    .cwd,
                Some(typed_in()),
                "{spelling} is the folder the launch was typed in"
            );
        }
        assert_eq!(
            from(&[relative(&["..", "bt-wt"]).as_str()])
                .expect("a folder crosses")
                .cwd,
            Some(host_path(r"D:\Developer\bt-wt")),
            "a positional goes through the same door as the flag"
        );
        assert_eq!(
            from(&["--cwd", host_spelling(r"D:\Other").as_str()])
                .expect("a folder crosses")
                .cwd,
            Some(host_path(r"D:\Other")),
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

    /// RED (0.4.6 U-21) — **no launch is admitted after the photograph of an update's quit.**
    ///
    /// An update's Restart is a new trigger of the quit, and it is the first one that keeps the
    /// windows up and the loop turning after the photograph: it waits across turns for its
    /// session's receipt. `launch_wire::admit` is one of the two readers that assumed a person
    /// quits (the restore card is the other), and it is held here to the quit's own answer at
    /// every step of that walk — admitting while the card is still asking, and refusing with the
    /// existing `NotServing` from the answer on. The two places the window thread says it — the
    /// turn's head and the photograph's own arm, which runs in the turn the quit began — are held
    /// by `the_photograph_stops_the_launches_and_the_restore_card` in `main.rs`.
    ///
    /// MUTATION: make `Quit::admits_launches` answer `true` for the write
    /// (`Phase::Writing { .. }`) — a launch is then promised a window while the document lands.
    #[test]
    fn no_launch_is_admitted_after_the_photograph() {
        use crate::quit::{Quit, QuitAnswer, QuitStep, Reason, WriteVerdict};
        let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = take();
        let asks = |quit: &Quit| {
            set_admitting(quit.admits_launches());
            let admitted = admit(LaunchRequest::default(), true);
            set_admitting(true);
            admitted.is_some()
        };
        let txn = crate::update_txn::TxnId::new([0x21; 16]);
        let mut quit = Quit::begin_for(vec!["notes.md".to_owned()], Reason::UpdateRestart { txn });
        assert!(
            asks(&quit),
            "a quit still asking can be cancelled, and admits"
        );
        let now = std::time::Instant::now();
        let mut seen = Vec::new();
        let mut step = quit.answer(QuitAnswer::Discard);
        loop {
            seen.push(step);
            assert!(!asks(&quit), "a launch was admitted at {step:?}: {seen:?}");
            step = match step {
                QuitStep::Discard => quit.discarded(),
                QuitStep::Photograph => quit.photographed(),
                QuitStep::Write if quit.awaited_generation().is_none() => quit.requested(4, now),
                QuitStep::Write => quit.written(WriteVerdict::Landed),
                QuitStep::Retire => quit.retired(now),
                QuitStep::WaitForPages => quit.pages(true, now),
                QuitStep::Ask | QuitStep::Save | QuitStep::Exit | QuitStep::Abandon => break,
            };
        }
        assert!(
            seen.contains(&QuitStep::Photograph) && seen.contains(&QuitStep::Exit),
            "the whole road: {seen:?}"
        );
        assert!(
            admit(LaunchRequest::default(), true).is_some(),
            "and an ordinary run admits again"
        );
        let _ = take();
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

    /// The names of a carried environment — what these tests assert and print, never a value.
    fn names_of(environment: Option<&cli::CarriedEnvironment>) -> Vec<String> {
        environment.map_or_else(Vec::new, |environment| {
            environment
                .pairs()
                .iter()
                .map(|(name, _)| name.to_string_lossy().into_owned())
                .collect()
        })
    }

    /// A launcher's environment of the test's own making: a unique name, a CJK value, Windows'
    /// per-drive folder variable, and an empty value.
    fn a_launchers_environment() -> cli::CarriedEnvironment {
        cli::CarriedEnvironment::from_pairs(vec![
            (
                "FSWEEP2_CARRIED_环境".into(),
                "D:\\工具\\venv\\Scripts".into(),
            ),
            ("=C:".into(), "C:\\Users".into()),
            ("FSWEEP2_EMPTY".into(), "".into()),
        ])
    }

    /// **RED (F-SWEEP-2-048) — `--with-environment` crosses the wire as itself, and a pair that is
    /// not a variable is not a frame.**
    ///
    /// MUTATION: drop the environment on the wire (`encode` writing no `env` key) and the decoded
    /// request carries none.
    #[test]
    fn a_launch_wire_frame_carries_the_environment_and_refuses_one_that_is_not_one() {
        let request = LaunchRequest {
            cwd: Some(host_path(r"D:\项目")),
            environment: Some(a_launchers_environment()),
            ..LaunchRequest::default()
        };
        let back = LaunchRequest::decode(&request.encode()).expect("the frame reads back");
        assert_eq!(
            names_of(back.environment.as_ref()),
            names_of(request.environment.as_ref())
        );
        assert!(
            back == request,
            "the values crossed changed (not printed: an environment's values stay out of test \
             output)"
        );
        // Absent is none, as every earlier sender writes it.
        let plain = LaunchRequest::default();
        assert_eq!(
            LaunchRequest::decode(&plain.encode()).and_then(|it| it.environment),
            None
        );
        // Each of these is not a variable a process could hold.
        let frame = |pairs: &str| {
            format!(r#"{{"v":2,"new":false,"tab":false,"from":"plain","env":{pairs}}}"#)
        };
        for pairs in [
            r#"[["", "x"]]"#,
            r#"[["A=B", "x"]]"#,
            r#"[["NAME", "a\u0000b"]]"#,
            r#"[["NA\u0000ME", "x"]]"#,
            r#"[["NAME"]]"#,
            r#"[["NAME", "x", "y"]]"#,
            r#"[["NAME", 1]]"#,
            r#"{"NAME": "x"}"#,
        ] {
            assert_eq!(LaunchRequest::decode(&frame(pairs)), None, "{pairs}");
        }
        assert!(LaunchRequest::decode(&frame(r#"[["=D:", "D:\\"]]"#)).is_some());
    }

    /// **RED (F-SWEEP-2-048) — a launch carrying its environment, handed to a running Folio over
    /// the real endpoint, lands with it.**
    ///
    /// The whole road of [`hand_over`] past its door: [`offer_start`] builds the request from the
    /// command line and the carried environment, `bt_platform::launch_pipe` carries the frame, and
    /// the running end decodes it. The running end here takes the decoded request as it is, so the
    /// shared inbox other tests drain is not touched.
    ///
    /// MUTATION: drop it on the wire (`encode` writing no `env` key, or `offer_start` dropping its
    /// `environment`) and the landed request carries no environment.
    #[test]
    fn a_launch_carrying_its_environment_lands_with_it_in_the_running_folio() {
        let directory = bt_testpath::temp_path("bt-app-launch-wire-环境");
        std::fs::create_dir_all(&directory).expect("a data directory");
        let (sender, landed) = std::sync::mpsc::channel();
        let Ok(endpoint) = LaunchPipe::start(
            &directory,
            |line| {
                LaunchRequest::decode(line).map(|request| bt_platform::launch_pipe::Decision {
                    reply: Reply::Taken.encode(),
                    admitted: Some(request),
                })
            },
            move |request: LaunchRequest| {
                let _ = sender.send(request);
            },
        ) else {
            // A platform with no launch endpoint carries nothing to anybody.
            return;
        };
        let carried = a_launchers_environment();
        let left = offer_start(
            &directory,
            &argv(&["--with-environment", "--tab"]),
            None,
            Some(carried.clone()),
            None,
            |_| panic!("a launch that was taken says nothing"),
            |why| panic!("the hand-over gave up: {why}"),
        );
        drop(endpoint);
        assert_eq!(left, Some(0), "the running Folio took the launch");
        let request = landed
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the launch landed");
        assert_eq!(
            names_of(request.environment.as_ref()),
            names_of(Some(&carried))
        );
        assert!(
            request.environment.as_ref() == Some(&carried),
            "the values landed are not the ones carried (not printed)"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// **RED (F-SWEEP-2-048) — an environment too large for the wire refuses the launch with a
    /// line, and is never cut to fit.**
    ///
    /// No running Folio is needed: the refusal is decided before a byte is written. The line goes
    /// to the start's console and to `diagnostics.log` in the data directory, and names the count
    /// and the sizes, never a value.
    ///
    /// MUTATION: send it anyway (drop the `environment_refusal` arm in `offer_start`) and the start
    /// gives up and opens a window of its own instead (`None`), with no line.
    #[test]
    fn a_launch_wire_environment_too_large_for_the_frame_is_refused_with_a_line() {
        let directory = bt_testpath::temp_path("bt-app-launch-wire-过大");
        std::fs::create_dir_all(&directory).expect("a data directory");
        let large = cli::CarriedEnvironment::from_pairs(vec![(
            "FSWEEP2_LARGE".into(),
            "路"
                .repeat(bt_platform::launch_pipe::MAX_FRAME_BYTES / 3 + 1)
                .into(),
        )]);
        let said = std::cell::RefCell::new(Vec::new());
        let left = offer_start(
            &directory,
            &argv(&["--with-environment"]),
            None,
            Some(large),
            None,
            |line| said.borrow_mut().push(line.to_owned()),
            |why| panic!("an oversized environment is refused, not given up on: {why}"),
        );
        assert_eq!(left, Some(2), "the launch is refused and the start leaves");
        let said = said.into_inner();
        assert_eq!(said.len(), 1, "one line on the console");
        assert!(
            said[0].contains(cli::WITH_ENVIRONMENT_FLAG)
                && said[0].contains("1 variables")
                && said[0].contains(&bt_platform::launch_pipe::MAX_FRAME_BYTES.to_string()),
            "{}",
            said[0]
        );
        assert!(!said[0].contains('路'), "the line names no value");
        let log = std::fs::read_to_string(crate::diagnostics::log_path(&directory))
            .expect("the line is in the data directory's diagnostics.log");
        assert!(log.contains(&said[0]));
        let _ = std::fs::remove_dir_all(&directory);
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

    // ── U-36: what a start a rollback sent reports, across the hand-over ─────

    /// The folder of an unfinished rollback's journal, in a home whose path is not ASCII.
    fn journal_folder() -> PathBuf {
        PathBuf::from(r"D:\工具\Folio 终端\.folio-update")
    }

    /// The command line a lock holder starts the installed build with after a rollback: the
    /// journal's word first (`update_apply::failed_words`), then the handed start's own words.
    fn sent_by_a_rollback() -> cli::CliRequest {
        argv(&[
            "--update-failed",
            r"D:\工具\Folio 终端\.folio-update\journal.json",
            "--cwd",
            r"D:\Developer\笔记",
        ])
    }

    /// The reports a start can carry, an unfinished rollback's naming `folder`.
    fn reports(folder: PathBuf) -> [Failure; 5] {
        [
            Failure::RolledBack,
            Failure::Interrupted,
            Failure::Incomplete {
                folder: Some(folder.clone()),
                held: false,
                untried: false,
            },
            // 0.4.8 E4: the update stopped before the new version started, and
            // a hold of the journal over the restored one.
            Failure::Incomplete {
                folder: Some(folder),
                held: false,
                untried: true,
            },
            Failure::JournalHeld {
                error: "拒绝访问。 (os error 5)".to_owned(),
                then: Box::new(Failure::RolledBack),
            },
        ]
    }

    /// **RED (U-36) — a start a rollback sent, which finds a Folio running, has its report raised
    /// as the card in the window its launch landed in, and every window's Version row moves.**
    ///
    /// The product's own seams end to end, except the kernel object between them (the platform's
    /// pipe carries a line byte for byte, `bt_platform::launch_pipe`'s own tests): the request is
    /// built by [`LaunchRequest::of_start`] — what [`hand_over`] sends — from the command line a
    /// lock holder writes and the pass's verdict; the listener's [`decide`] answers it; [`park`]
    /// and [`take`] carry it to the window thread; and [`LaunchRequest::told`] tells the running
    /// Folio's job, whose card and Version row are read through `update_card::shown`, the one
    /// comparison the loop repaints from. Before U-36 the request had no report and the job no
    /// card: the failure was dropped on the way.
    ///
    /// MUTATIONS: build the hand-over's request with [`LaunchRequest::from_cli`] (the report is
    /// never set); drop the report from `encode` or `decode`; drop the `told` call's job update
    /// (`Job::told_by_a_launch` returns before `told`) — each leaves the job without a card.
    #[test]
    fn a_report_crosses_the_hand_over_and_its_card_rises_where_the_launch_landed() {
        let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = take();
        // The listener's `accept` asks the disk, so the handed folder is one that exists.
        let here = bt_testpath::temp_path("bt-app-u36-笔记");
        std::fs::create_dir_all(&here).expect("a scratch folder");
        let start = argv(&[
            "--update-failed",
            r"D:\工具\Folio 终端\.folio-update\journal.json",
            "--cwd",
            &here.to_string_lossy(),
        ]);
        // A local path on the platform the test runs on, so [`accept`] keeps it.
        for failure in reports(here.join(".folio-update")) {
            let request = LaunchRequest::of_start(
                &start,
                Some(&failure),
                all_folders,
                Some(typed_in().as_path()),
            )
            .expect("a start a rollback sent is a launch this wire carries");
            assert!(request.is_sayable(), "{failure:?} can be said");
            let decision = decide(&request.encode(), || true).expect("the line is a request");
            assert_eq!(Reply::decode(&decision.reply), Some(Reply::Taken));
            park(decision.admitted.expect("the running Folio admitted it"));
            let arrived = take();
            assert_eq!(arrived.len(), 1, "one launch reached the window thread");
            let arrived = &arrived[0];
            assert_eq!(
                arrived.cwd,
                Some(here.clone()),
                "the launch is still the launch it was"
            );

            let mut job = crate::update_job::Job::<u32>::with_offers(true);
            let before = crate::update_card::shown(
                &job,
                false,
                crate::update::CheckView::default(),
                None,
                0,
            );
            const LANDED: u32 = 7;
            assert_eq!(arrived.told(&mut job, Some(LANDED)), Some(true));
            assert_eq!(
                job.state(),
                &crate::update_job::State::Failed(None, failure.clone()),
                "the card is the one the start would have shown cold"
            );
            assert_eq!(
                job.card_window(),
                Some(LANDED),
                "in the window the launch landed in"
            );
            let after = crate::update_card::shown(
                &job,
                false,
                crate::update::CheckView::default(),
                None,
                0,
            );
            assert_eq!(after.card.as_ref().map(|(window, _)| *window), Some(LANDED));
            assert!(
                crate::update_card::version_changed(&before, &after),
                "every window's About → Version row is repainted, because any may show it"
            );
        }
        // A launch with nothing to report tells the job nothing.
        let plain = LaunchRequest::of_start(&argv(&[]), None, all_folders, None)
            .expect("an empty command line crosses");
        let mut job = crate::update_job::Job::<u32>::with_offers(true);
        assert_eq!(plain.told(&mut job, Some(1)), None);
        assert_eq!(job.card_window(), None);
        let _ = std::fs::remove_dir(&here);
    }

    /// **RED (047-U36-CARD) — a report that lands in a window of its own is up in that window
    /// after the turn's card settle, whether the start was this installation's or another's.**
    ///
    /// The clean-VM row N15: a failure start of one installation handed itself to a running Folio
    /// of another, the launch opened a window (the default landing), the job was told the new
    /// window — and the card was drawn nowhere a reader could see, though the log said it was
    /// raised. The settle that follows the telling (`FolioApp::settle_update_card`) asks
    /// [`crate::update_job::Job::hand_over`] whether the card's window is still open, against the
    /// window directory; that directory had been walked at the turn's head, before the window
    /// door opened the launch's window, so the card was taken for one whose window had closed and
    /// seated in the older window, behind the new one. The window door now names the window in
    /// the directory as it opens it (pinned at `launch_landing_tests`); this holds what the
    /// settle does with the directory that door leaves and with the one the turn's head walked,
    /// for both installations, and what the log line says of each.
    ///
    /// The receiver's installation never enters the job: the report's folder is a name on the
    /// card, and a folder in another copy's home is shown as one in its own.
    ///
    /// MUTATIONS: in [`LaunchRequest::told_line`], drop the `Some(window) == landed` guard (the
    /// line says "the window the launch landed in" of the card seated in the older window — the
    /// rehearsal's false line); in `Job::hand_over`, keep a card whose window is not in the
    /// directory where it is (the turn-head case leaves it in the landed window, so the
    /// assertion that such a directory moves it goes red — the reason the door must name it).
    #[test]
    fn a_report_landing_in_a_window_of_its_own_is_up_there_after_the_settle() {
        let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = take();
        let root = bt_testpath::temp_path("bt-app-047-u36-card");
        let receiver = root.join("Folio 终端");
        let other = root.join("其他 copy");
        for home in [&receiver, &other] {
            std::fs::create_dir_all(home.join(".folio-update")).expect("an installation's home");
        }
        // The receiver's own windows: one the reader was in, and the summoned terminal.
        const OLDER: u32 = 1;
        const QUAKE: u32 = 2;
        const LANDED: u32 = 9;
        for (starter, label) in [
            (&receiver, "same installation"),
            (&other, "another installation"),
        ] {
            for failure in reports(starter.join(".folio-update")) {
                let start = argv(&["--update-failed", "journal.json"]);
                let request = LaunchRequest::of_start(&start, Some(&failure), all_folders, None)
                    .expect("a start a rollback sent crosses");
                let decision = decide(&request.encode(), || true).expect("the line is a request");
                park(decision.admitted.expect("the running Folio admitted it"));
                let arrived = take();
                assert_eq!(arrived.len(), 1, "{label}: {failure:?}");
                // The default landing for a plain start is a window of its own.
                assert_eq!(
                    landing(&arrived[0], bt_persist::LaunchOpensV1::default()),
                    Landing::Window
                );

                // The turn, as the window thread runs it: the landing records the new window as
                // the one the reader is in, the job is told it, and the card settles against the
                // window directory — once as the window door now leaves it, naming the new
                // window, and once as the turn's head walked it, before the door opened it
                // (the rehearsal's turn).
                let visited = [QUAKE, OLDER, LANDED];
                for (open, seated, says) in [
                    (
                        &[OLDER, QUAKE, LANDED][..],
                        LANDED,
                        "its card is up in the window the launch landed in",
                    ),
                    (
                        &[OLDER, QUAKE][..],
                        OLDER,
                        "its card is up in another window",
                    ),
                ] {
                    let mut job = crate::update_job::Job::<u32>::with_offers(true);
                    assert_eq!(
                        arrived[0].told(&mut job, Some(LANDED)),
                        Some(true),
                        "{label}: {failure:?}"
                    );
                    job.hand_over(&crate::update_job::Presenters {
                        visited: &visited,
                        open,
                        quake: Some(QUAKE),
                    });
                    let shown = crate::update_card::shown(
                        &job,
                        false,
                        crate::update::CheckView::default(),
                        None,
                        0,
                    );
                    let (window, paint) = shown.card.as_ref().expect("the card is up");
                    assert_eq!(*window, seated, "{label}: {failure:?} {open:?}");
                    // What a window draws from (`Runtime::update_card_is_up`).
                    assert_eq!(job.card_window(), Some(seated), "{label}: {failure:?}");
                    if let Failure::Incomplete { folder, .. } = &failure {
                        assert_eq!(
                            &paint.folder, folder,
                            "{label}: the folder is named as sent"
                        );
                    }
                    assert!(
                        LaunchRequest::told_line(true, Some(LANDED), Some(*window)).ends_with(says),
                        "{label}: {failure:?} {open:?}"
                    );
                }
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **RED (047-U36-CARD) — the log line says where the card is**, from the window the settle
    /// left it in, never from what the job was asked: a card seated elsewhere, or nowhere, is
    /// not "up in the window the launch landed in".
    ///
    /// MUTATION: drop the `Some(window) == landed` guard of [`LaunchRequest::told_line`]; the
    /// card seated in another window is reported as up where the launch landed.
    #[test]
    fn the_log_line_names_where_the_card_is() {
        let line = |raised, landed, shown| LaunchRequest::told_line::<u32>(raised, landed, shown);
        assert!(
            line(true, Some(9), Some(9))
                .ends_with("its card is up in the window the launch landed in")
        );
        assert!(line(true, Some(9), Some(1)).ends_with("its card is up in another window"));
        assert!(line(true, None, Some(1)).ends_with("its card is up in another window"));
        assert!(line(true, Some(9), None).ends_with("no window shows its card"));
        for shown in [None, Some(1), Some(9)] {
            assert!(
                line(false, Some(9), shown).ends_with("the running update keeps the card"),
                "{shown:?}"
            );
        }
    }

    /// **RED (U-36) — the start's hand-over builds its request with its pass's report.**
    ///
    /// The one line no in-process test can run — `hand_over` holds an owner-thread door's token
    /// and the pass's witness — read from the source: it hands what the pass's witness says to
    /// [`offer_start`], whose request goes through [`LaunchRequest::of_start`] and is answered
    /// through [`after_reply`] (0.4.8 D3: [`offer_start`] itself runs in the two-copy tests).
    ///
    /// MUTATION: build the request with `LaunchRequest::from_cli` in `offer_start`, or pass
    /// `None` for the report in `hand_over`.
    #[test]
    fn the_hand_over_sends_what_the_pass_sent_it_to_report() {
        let body_of = |name: &str| {
            bt_source::Index::of_package("bt-app")
                .body_of(&bt_source::ItemQuery::function(name).in_module("crate::launch_wire"))
                .unwrap_or_else(|failure| panic!("{failure}"))
        };
        let door = body_of("hand_over");
        assert!(
            door.contains("offer_start(") && door.contains("admitted.failed().as_ref()"),
            "the hand-over no longer offers the pass's report:\n{door}"
        );
        let body = body_of("offer_start");
        assert!(
            body.contains("LaunchRequest::of_start(argv, failed,"),
            "the hand-over's request no longer carries the pass's report:\n{body}"
        );
        assert!(
            body.contains("after_reply(request, answer, say)"),
            "the answer is read somewhere other than the tested function:\n{body}"
        );
    }

    /// **RED (U-36 round 2) — every path the wire carries meets one gate, and a report whose
    /// folder fails it keeps the report and loses the folder.**
    ///
    /// The journal folder of an `incomplete` report is named on the card and handed to the file
    /// manager by its Show folder, so a share there would be a click that offers this account's
    /// credentials to another machine. [`accept`] holds it to the gate `cwd` meets
    /// ([`is_a_local_path`]). What fails is taken away and the report stays: the reader is told
    /// the update is incomplete, on a card that names no folder and offers the releases page. A
    /// local folder is kept, with Show folder.
    ///
    /// MUTATIONS: drop the report-folder half of `accept` (the share reaches the card and its
    /// Show folder); drop the report instead of its folder (the reader is told nothing).
    #[test]
    fn a_report_folder_that_is_not_local_is_taken_away_and_the_report_kept() {
        let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = take();
        let local = bt_testpath::temp_path("工具").join(".folio-update");
        for (sent, kept) in [
            (
                PathBuf::from(r"\\server\share\Folio 终端\.folio-update"),
                None,
            ),
            (PathBuf::from(r"\\.\pipe\folio"), None),
            (PathBuf::from(r"工具\.folio-update"), None),
            (local.clone(), Some(local.clone())),
        ] {
            let request = LaunchRequest {
                report: Some(Report::Incomplete {
                    folder: Some(sent.clone()),
                    untried: false,
                }),
                ..LaunchRequest::default()
            };
            let decision = decide(&request.encode(), || true).expect("the line is a request");
            assert_eq!(
                Reply::decode(&decision.reply),
                Some(Reply::Taken),
                "a report is never refused for its folder: {sent:?}"
            );
            park(decision.admitted.expect("admitted"));
            let arrived = take();
            assert_eq!(arrived.len(), 1);
            assert_eq!(
                arrived[0].report,
                Some(Report::Incomplete {
                    folder: kept.clone(),
                    untried: false,
                }),
                "{sent:?}"
            );
            let mut job = crate::update_job::Job::<u32>::with_offers(true);
            assert_eq!(arrived[0].told(&mut job, Some(2)), Some(true));
            let (_, paint) = crate::update_card::shown(
                &job,
                false,
                crate::update::CheckView::default(),
                None,
                0,
            )
            .card
            .expect("the card is up");
            assert_eq!(paint.folder, kept, "{sent:?}");
            assert_eq!(
                paint.verbs.first(),
                Some(if kept.is_some() {
                    &crate::update_card::CardVerb::ShowFolder
                } else {
                    &crate::update_card::CardVerb::Releases
                }),
                "Show folder only with a folder to show: {sent:?}"
            );
        }
    }

    /// **RED (E1) — a newer Folio's unfinished update crosses the hand-over as *Update
    /// incomplete.*, in the words every build since 0.4.7 reads.**
    ///
    /// A start that continued past a journal it cannot read whole, and finds a Folio already
    /// running, hands its report over like any other. The running Folio may be 0.4.7, whose
    /// decoder drops the whole frame at a value of `failed` it does not know
    /// ([`WIRE_VERSION`]'s rule): so the report crosses as `incomplete` with its folder — the
    /// frame byte for byte the one an unfinished rollback sends — whatever later build the
    /// journal names, and whether or not the sender's writes are held.
    ///
    /// MUTATION: give `Failure::Newer` a report of its own in [`Report::of`] (a new token,
    /// `newer`): a 0.4.7 Folio drops the frame.
    #[test]
    fn a_newer_folios_unfinished_update_crosses_as_update_incomplete() {
        let folder = journal_folder();
        let incomplete = LaunchRequest::of_start(
            &sent_by_a_rollback(),
            Some(&Failure::Incomplete {
                folder: Some(folder.clone()),
                held: false,
                untried: false,
            }),
            all_folders,
            Some(typed_in().as_path()),
        )
        .expect("the launch crosses")
        .encode();
        for version in [None, Some("99.0.0".to_owned())] {
            for held in [false, true] {
                let failure = Failure::Newer {
                    folder: Some(folder.clone()),
                    version: version.clone(),
                    held,
                };
                assert_eq!(
                    Report::of(&failure),
                    Some(Report::Incomplete {
                        folder: Some(folder.clone()),
                        untried: false,
                    })
                );
                let frame = LaunchRequest::of_start(
                    &sent_by_a_rollback(),
                    Some(&failure),
                    all_folders,
                    Some(typed_in().as_path()),
                )
                .expect("the launch crosses")
                .encode();
                assert_eq!(frame, incomplete, "{failure:?}: the same frame as 0.4.7's");
                let words: serde_json::Value = serde_json::from_str(&frame).unwrap();
                assert_eq!(words[REPORT_KEY], "incomplete");
                assert_eq!(
                    words[REPORT_FOLDER_KEY].as_str().map(PathBuf::from),
                    Some(folder.clone())
                );
                assert_eq!(
                    LaunchRequest::decode(&frame).and_then(|request| request.report),
                    Some(Report::Incomplete {
                        folder: Some(folder.clone()),
                        untried: false,
                    })
                );
            }
        }
    }

    /// **RED (U-36) — a trial's report and a driver's failure never cross.**
    ///
    /// `TrialIncomplete`'s card says *this session is the update's trial*, which is false of
    /// every process but the trial; a trial never hands itself over, and if one ever did its
    /// report would be a lie on the other side. `Unsupported` and `Stopped` are a running job's
    /// own and never a start's.
    ///
    /// MUTATION: map `Failure::TrialIncomplete` to [`Report::Incomplete`] in [`Report::of`].
    #[test]
    fn a_trials_report_and_a_drivers_failure_never_cross() {
        for failure in [
            Failure::TrialIncomplete {
                folder: journal_folder(),
            },
            Failure::Unsupported,
            Failure::Stopped(crate::update_job::Stop::Download),
        ] {
            let request = LaunchRequest::of_start(
                &sent_by_a_rollback(),
                Some(&failure),
                all_folders,
                Some(typed_in().as_path()),
            )
            .expect("the launch still crosses");
            assert_eq!(request.report, None, "{failure:?} is not carried");
            assert!(
                !request.encode().contains(REPORT_KEY),
                "and nothing of it is written"
            );
        }
    }

    /// **RED (U-36) — the report is a closed set, its folder is bounded like a folder, and it
    /// comes with `incomplete` and nothing else.**
    ///
    /// The decode rule of the other two text fields and of `from`, at the new keys: a word this
    /// build does not know, a folder without its word or a word without its folder, an empty or
    /// overlong folder or one carrying a control byte, is not a frame this build understands.
    ///
    /// MUTATIONS: read an unknown token as no report (the first line is taken); drop the
    /// `(None, Some(_))` arm (the orphan folder is taken); bound the folder by nothing (the long
    /// one is taken).
    #[test]
    fn a_report_is_a_closed_word_and_its_folder_is_bounded() {
        let long = "C:\\".to_owned() + &"a".repeat(MAX_FOLDER_BYTES);
        let base = r#""v":2,"new":false,"tab":false,"from":"plain""#;
        for line in [
            format!(r#"{{{base},"failed":"updated"}}"#),
            format!(r#"{{{base},"failed":true}}"#),
            format!(r#"{{{base},"failed":"incomplete"}}"#),
            format!(r#"{{{base},"failed":"rolled-back","failed_folder":"C:\\x"}}"#),
            format!(r#"{{{base},"failed_folder":"C:\\x"}}"#),
            format!(r#"{{{base},"failed":"incomplete","failed_folder":""}}"#),
            format!(r#"{{{base},"failed":"incomplete","failed_folder":"C:\\a\rb"}}"#),
            format!(r#"{{{base},"failed":"incomplete","failed_folder":"{long}"}}"#),
        ] {
            assert_eq!(
                LaunchRequest::decode(&line),
                None,
                "{line} is not a request this build understands"
            );
        }
        assert_eq!(
            LaunchRequest::decode(&format!(
                r#"{{{base},"failed":"incomplete","failed_folder":"D:\\工具\\Folio 终端\\.folio-update"}}"#
            ))
            .and_then(|request| request.report),
            Some(Report::Incomplete {
                folder: Some(journal_folder()),
                untried: false,
            }),
            "and a well-formed one is"
        );
    }

    /// **RED (0.4.8 E4) — a report's cause crosses in keys of its own: whether no trial was
    /// begun, and the refusal of a journal another program held; the receiver tells its job the
    /// very failure the start had, so its card has the same heading.** The keys come only where
    /// they mean something — `failed_untried` with `incomplete` alone, as a boolean; the refusal
    /// with any word, bounded and free of control bytes — and the word itself is the one every
    /// build since 0.4.7 reads (an earlier receiver says *Update incomplete.* or *Previous
    /// version restored.* as it always did; `a_0_4_6_receiver_takes_a_reported_launch_as_the_same_launch`
    /// holds the version's frozen reader to these frames too).
    ///
    /// MUTATIONS: leave `REPORT_UNTRIED_KEY` out of `encode` — the untried report comes back
    /// tried; leave `REPORT_HELD_KEY` out — the held report comes back without its hold.
    #[test]
    fn a_reports_cause_crosses_in_keys_of_its_own() {
        let refusal = "拒绝访问。 (os error 5)".to_owned();
        let untried = Failure::Incomplete {
            folder: Some(journal_folder()),
            held: false,
            untried: true,
        };
        for failure in [
            untried.clone(),
            Failure::JournalHeld {
                error: refusal.clone(),
                then: Box::new(untried.clone()),
            },
            Failure::JournalHeld {
                error: refusal.clone(),
                then: Box::new(Failure::Interrupted),
            },
        ] {
            let request =
                LaunchRequest::of_start(&sent_by_a_rollback(), Some(&failure), all_folders, None)
                    .expect("it crosses");
            let frame = request.encode();
            let words: serde_json::Value = serde_json::from_str(&frame).unwrap();
            assert_ne!(
                words[REPORT_KEY], "journal-held",
                "never a new word: {frame}"
            );
            let arrived = LaunchRequest::decode(&frame).expect("this build reads it");
            assert_eq!(
                arrived.report.map(|report| report.failure()),
                Some(failure.clone()),
                "{frame}"
            );
        }
        let base = r#""v":2,"new":false,"tab":false,"from":"plain""#;
        for line in [
            format!(r#"{{{base},"failed":"rolled-back","failed_untried":true}}"#),
            format!(
                r#"{{{base},"failed":"incomplete","failed_folder":"C:\\x","failed_untried":"yes"}}"#
            ),
            format!(r#"{{{base},"failed_untried":true}}"#),
            format!(r#"{{{base},"failed_journal_held":"拒绝访问。"}}"#),
            format!(r#"{{{base},"failed":"interrupted","failed_journal_held":""}}"#),
            format!(r#"{{{base},"failed":"interrupted","failed_journal_held":"a\nb"}}"#),
        ] {
            assert_eq!(
                LaunchRequest::decode(&line),
                None,
                "{line} is not a request this build understands"
            );
        }
    }

    /// **RED (U-36) — a launch with a report is never refused into nothing.**
    ///
    /// Nobody is at a console for a start a lock holder made, so a refused folder would have ended
    /// the start with a sentence nobody reads and the report with it; it opens its own window
    /// instead, which shows both. A launch with nothing to report keeps its sentence and its code.
    ///
    /// MUTATION: drop the guarded `NoSuchFolder` arm of `after_reply` — the reported launch leaves
    /// with code 2.
    #[test]
    fn a_launch_with_a_report_is_never_refused_into_nothing() {
        let said = std::cell::RefCell::new(Vec::new());
        let say = |line: &str| said.borrow_mut().push(line.to_owned());
        let reported = LaunchRequest::of_start(
            &sent_by_a_rollback(),
            Some(&Failure::RolledBack),
            all_folders,
            Some(typed_in().as_path()),
        )
        .expect("it crosses");
        assert_eq!(
            after_reply(reported.clone(), Reply::Refused(Refusal::NoSuchFolder), say),
            None,
            "the start opens its own window, and its card"
        );
        assert!(said.borrow().is_empty(), "and prints nothing nobody reads");
        assert_eq!(after_reply(reported, Reply::Taken, say), Some(0));
        let plain = from(&["--cwd", r"D:\gone"]).expect("it crosses");
        assert_eq!(
            after_reply(plain, Reply::Refused(Refusal::NoSuchFolder), say),
            Some(2)
        );
        assert_eq!(said.borrow().len(), 1, "a person's launch still says why");
    }

    /// **0.4.6's reader of this wire**: the same body as `LaunchRequest::decode` and
    /// `origin_from_token` at `v0.4.6-preview` (that file did not change between the tag and
    /// U-36), with the constants it reads inlined, its comments left out and its result named
    /// `Request` — the frozen fixture the compatibility
    /// tests below hold every later frame to. A copy, and deliberately: it is not this build's
    /// reader but a build already on people's machines, and it must not move when this one does.
    mod as_0_4_6 {
        use std::path::PathBuf;

        use crate::cli;

        const WIRE_VERSION: u64 = 2;
        const MAX_FOLDER_BYTES: usize = 2048;
        const MAX_PROFILE_BYTES: usize = 128;

        #[derive(Debug, PartialEq, Eq)]
        pub(super) struct Request {
            pub(super) cwd: Option<PathBuf>,
            pub(super) profile: Option<String>,
            pub(super) new_window: bool,
            pub(super) tab: bool,
            pub(super) origin: cli::LaunchOrigin,
        }

        fn origin_from_token(token: &str) -> Option<cli::LaunchOrigin> {
            match token {
                "plain" => Some(cli::LaunchOrigin::Plain),
                "explorer" => Some(cli::LaunchOrigin::Explorer),
                "here" => Some(cli::LaunchOrigin::Here),
                _ => None,
            }
        }

        pub(super) fn decode(line: &str) -> Option<Request> {
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
                (!text.is_empty() && text.len() <= bound && !text.chars().any(char::is_control))
                    .then(|| Some(text.to_owned()))
            };
            Some(Request {
                cwd: bounded("cwd", MAX_FOLDER_BYTES)?.map(PathBuf::from),
                profile: bounded("profile", MAX_PROFILE_BYTES)?,
                new_window: object.get("new")?.as_bool()?,
                tab: object.get("tab")?.as_bool()?,
                origin: origin_from_token(object.get("from")?.as_str()?)?,
            })
        }
    }

    /// **Frames a 0.4.6 `folio.exe` writes**, as JSON — one per shape its `encode` has: no folder
    /// or profile, both, and each origin. What 0.4.6 sends is fixed; these are its words.
    const FRAMES_0_4_6: [&str; 4] = [
        r#"{"v":2,"new":false,"tab":false,"from":"plain"}"#,
        r#"{"v":2,"cwd":"D:\\Developer\\笔记","profile":"winps","new":true,"tab":false,"from":"plain"}"#,
        r#"{"v":2,"cwd":"C:\\Users","new":false,"tab":true,"from":"explorer"}"#,
        r#"{"v":2,"new":false,"tab":false,"from":"here"}"#,
    ];

    /// **RED (U-36) — old sender → new receiver: a 0.4.6 frame is read as the launch it was, with
    /// nothing to report; and a launch of this build with nothing to report is the 0.4.6 frame.**
    ///
    /// The half of compatibility this build can hold by itself. A 0.4.6 start reports nothing (it
    /// has no key to report with), so its launch lands exactly as before and the running Folio's
    /// job is told nothing; and a launch of this build that has nothing to report writes no new
    /// key, so it is, as JSON, the frame 0.4.6 writes for the same launch.
    ///
    /// MUTATIONS: make the report key required, or write it as `null` when there is none — the
    /// first assertion or the second goes red; bump [`WIRE_VERSION`] — every frame here is dropped.
    #[test]
    fn a_0_4_6_frame_is_the_same_launch_with_nothing_to_report() {
        for frame in FRAMES_0_4_6 {
            let request =
                LaunchRequest::decode(frame).expect("a 0.4.6 launch is one this build takes");
            assert_eq!(request.report, None, "{frame} reports nothing");
            let ours: serde_json::Value =
                serde_json::from_str(&request.encode()).expect("this build writes JSON");
            let theirs: serde_json::Value =
                serde_json::from_str(frame).expect("the fixture is JSON");
            assert_eq!(
                ours, theirs,
                "with nothing to report this build writes 0.4.6's frame"
            );
        }
    }

    /// **RED (U-36) — new sender → old receiver: a 0.4.6 Folio takes a launch with a report as the
    /// same launch, and only the report is lost — as it was before U-36.**
    ///
    /// The other half, against the frozen reader above: the report rides in keys 0.4.6 does not
    /// read, inside the version it does, so a 0.4.6 that is running when a later build's rollback
    /// sends its start opens the window or tab it always would; it shows no card, because it has
    /// no way to (RULES §36 names this).
    ///
    /// MUTATIONS: send the report under `from` or another key 0.4.6 reads, or bump
    /// [`WIRE_VERSION`] for it — the frozen reader drops the frame, and the start opens a second,
    /// non-writing window.
    #[test]
    fn a_0_4_6_receiver_takes_a_reported_launch_as_the_same_launch() {
        for failure in reports(journal_folder()) {
            let request = LaunchRequest::of_start(
                &sent_by_a_rollback(),
                Some(&failure),
                all_folders,
                Some(typed_in().as_path()),
            )
            .expect("it crosses");
            assert!(request.report.is_some(), "{failure:?} is carried");
            let old = as_0_4_6::decode(&request.encode())
                .unwrap_or_else(|| panic!("0.4.6 dropped {}", request.encode()));
            assert_eq!(
                old,
                as_0_4_6::Request {
                    cwd: request.cwd.clone(),
                    profile: request.profile.clone(),
                    new_window: request.new_window,
                    tab: request.tab,
                    origin: request.origin,
                },
                "0.4.6 reads the same launch"
            );
        }
        for frame in FRAMES_0_4_6 {
            assert!(
                as_0_4_6::decode(frame).is_some(),
                "the frozen reader reads its own build's frames: {frame}"
            );
        }
    }

    /// **RED (0.4.8 E3, #12) — a person's start a recovery deferred reaches the Folio that opens
    /// the window: its folder, over the real launch endpoint.**
    ///
    /// A recovery handed `--then-launch --cwd <folder>` that started nothing, because another
    /// process opens the window, carries that start ([`carried`], `update_apply::carry_the_start`):
    /// once a Folio holds the data directory, the request crosses the endpoint that Folio listens
    /// on, folder and origin intact, and no report rides with it. The far end here is a listener
    /// of the test's own on a private directory whose claim the test holds.
    ///
    /// MUTATION: in [`hand_over_carried`], converse with `&LaunchRequest::default()` (the folder
    /// dropped on the way to the window).
    #[test]
    fn a_carried_start_reaches_the_folio_that_holds_the_data_directory() {
        let directory = bt_testpath::temp_path("launch-wire-carried-数据");
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let folder = bt_testpath::temp_path("工作 文件夹 carried");
        std::fs::create_dir_all(&folder).unwrap();
        if bt_platform::launch_pipe::endpoint_for(&directory).is_none() {
            return;
        }
        let _holder = crate::persist::try_claim(&directory).expect("the window's Folio holds it");
        let (sender, landed) = std::sync::mpsc::channel();
        let Ok(_endpoint) = bt_platform::launch_pipe::LaunchPipe::start(
            &directory,
            |line: &str| {
                let request = LaunchRequest::decode(line)?;
                Some(bt_platform::launch_pipe::Decision {
                    reply: Reply::Taken.encode(),
                    admitted: Some(request),
                })
            },
            move |request: LaunchRequest| {
                let _ = sender.send(request);
            },
        ) else {
            return;
        };
        let handed: Vec<std::ffi::OsString> = ["--from-explorer", "--cwd"]
            .into_iter()
            .map(std::ffi::OsString::from)
            .chain([folder.clone().into_os_string()])
            .collect();
        let request = carried(&handed, None).expect("a folder crosses");
        assert_eq!(request.cwd.as_deref(), Some(folder.as_path()));
        assert_eq!(request.origin, cli::LaunchOrigin::Explorer);
        assert_eq!(request.report, None, "a carried start reports nothing");
        let sent = request.clone();
        let carried = bt_platform::spawn_at_priority(
            "bt-launch-wire-carry-test",
            bt_platform::ThreadPriority::BelowNormal,
            move |worker| {
                crate::update_apply::carry_the_start(
                    worker,
                    &directory,
                    &crate::update_apply::Ahead::DataHolder,
                    &sent,
                    std::time::Duration::from_secs(1),
                )
            },
        )
        .unwrap()
        .join()
        .unwrap();
        assert_eq!(carried, crate::update_apply::Carried::Taken);
        assert_eq!(
            // The listener commits once this end has confirmed `Taken`: its one
            // message is the completion signal.
            landed
                .recv()
                .expect("the window's Folio was handed the start"),
            request
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    // -----------------------------------------------------------------------
    // Two installed copies of Folio sharing one data directory (0.4.8 D3)
    // -----------------------------------------------------------------------

    /// The test and the half of it a copy of this binary runs ([`two_copies`]): `<test> serve`
    /// or `<test> start`.
    const COPY_CHILD: &str = "BT_LAUNCH_WIRE_COPY_CHILD";

    /// The private folder of that run: the data directory, the folder handed over, and the
    /// second start's command line.
    const COPY_ROOT: &str = "BT_LAUNCH_WIRE_COPY_ROOT";

    /// The data directory both copies share, inside a run's folder.
    fn shared_data(root: &Path) -> PathBuf {
        root.join("数据 data")
    }

    /// The second start's command line, one word a line, inside a run's folder.
    fn start_words(root: &Path) -> PathBuf {
        root.join("start.argv")
    }

    /// Present when the second start was sent by a rollback (it reports
    /// [`Failure::RolledBack`], the pass's verdict for `--update-failed`).
    fn start_report(root: &Path) -> PathBuf {
        root.join("start.report")
    }

    /// **This process's half of a [`two_copies`] run of `selector`**, when it is one: `true`
    /// once the half is done.
    ///
    /// * `serve` — the first copy: it holds the data directory's claim, listens on its launch
    ///   endpoint with this module's own [`decide`] and [`park`] (a window thread that can
    ///   serve), says `D3 SERVING`, and when its standard input closes stops listening — which
    ///   finishes a conversation in flight — and says `D3 LANDED <line>` for every launch it
    ///   was handed.
    /// * `start` — the second copy: a start whose claim is refused, offering its command line
    ///   exactly as `main`'s hand-over does ([`offer_start`]); it says `D3 LEFT <code>` when it
    ///   leaves, or `D3 OPENS <line>` with the give-up line when it opens a window of its own.
    fn copy_half(selector: &str) -> bool {
        use std::io::Write;
        let Ok(role) = std::env::var(COPY_CHILD) else {
            return false;
        };
        let Some(role) = role.strip_prefix(selector).map(str::trim) else {
            return false;
        };
        let root = PathBuf::from(std::env::var_os(COPY_ROOT).expect("the run's folder"));
        let data = shared_data(&root);
        let mut out = std::io::stdout();
        match role {
            "serve" => {
                let _claim = crate::persist::try_claim(&data).expect("the first copy holds it");
                let endpoint = LaunchPipe::start(&data, |line| decide(line, || true), park)
                    .expect("the first copy listens on the data directory's endpoint");
                writeln!(out, "D3 SERVING").unwrap();
                out.flush().unwrap();
                let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut Vec::new());
                drop(endpoint);
                for request in take() {
                    writeln!(out, "D3 LANDED {}", request.encode()).unwrap();
                }
            }
            "start" => {
                assert!(
                    crate::persist::try_claim(&data).is_err(),
                    "the second copy finds the data directory held"
                );
                let words = std::fs::read_to_string(start_words(&root)).unwrap();
                let words: Vec<&str> = words.lines().collect();
                let failed = start_report(&root).exists().then_some(Failure::RolledBack);
                let mut why = None;
                let left = offer_start(
                    &data,
                    &argv(&words),
                    failed.as_ref(),
                    None,
                    None,
                    |said| println!("D3 SAID {said}"),
                    |line| why = Some(line),
                );
                match left {
                    Some(code) => writeln!(out, "D3 LEFT {code}").unwrap(),
                    None => writeln!(out, "D3 OPENS {}", why.unwrap_or_default()).unwrap(),
                }
            }
            other => panic!("no half of the run is called {other}"),
        }
        out.flush().unwrap();
        true
    }

    /// **A half's own words in one line of its output**, from its `D3 ` on: the harness writes
    /// `test <name> ... ` before the test's first line, on the same line.
    fn marked(line: &str) -> Option<&str> {
        line.find("D3 ").map(|at| &line[at..])
    }

    /// What one [`two_copies`] run came to: the second start's own words (`D3 LEFT 0`, or
    /// `D3 OPENS <why>`) and every launch the first copy was handed.
    struct TwoCopies {
        second: String,
        landed: Vec<LaunchRequest>,
    }

    /// **Two copies of this program, at two paths, sharing one data directory**: `first` holds
    /// it and listens, `second` starts with `words` (a rollback's start when `report`), and the
    /// run's folder is removed after. Each is a real process; nothing opens a window. `None`
    /// where this platform's wire cannot name the data directory's endpoint.
    fn two_copies(
        selector: &str,
        first: &Path,
        second: &Path,
        words: &[String],
        report: bool,
    ) -> Option<TwoCopies> {
        use std::io::BufRead;
        use std::process::Stdio;
        let root = bt_testpath::temp_path("launch-wire-two-copies");
        std::fs::create_dir_all(shared_data(&root)).unwrap();
        if bt_platform::launch_pipe::endpoint_for(&shared_data(&root)).is_none() {
            let _ = std::fs::remove_dir_all(&root);
            return None;
        }
        std::fs::write(start_words(&root), words.join("\n")).unwrap();
        if report {
            std::fs::write(start_report(&root), b"rolled back").unwrap();
        }
        let half = |program: &Path, role: &str| {
            let mut command = bt_platform::quiet_command(program);
            command
                .args(["--exact", selector, "--nocapture", "--test-threads=1"])
                .env(COPY_CHILD, format!("{selector} {role}"))
                .env(COPY_ROOT, &root)
                .env("APPDATA", root.join("roaming"))
                .env("LOCALAPPDATA", root.join("local"))
                .env("HOME", root.join("home"))
                .env("XDG_DATA_HOME", root.join("xdg"))
                .stderr(Stdio::inherit());
            command
        };
        let mut serving = half(first, "serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("start the first copy");
        let mut said = std::io::BufReader::new(serving.stdout.take().unwrap());
        let mut line = String::new();
        while !line.contains("D3 SERVING") {
            line.clear();
            assert!(
                said.read_line(&mut line).unwrap() > 0,
                "the first copy ended before it listened"
            );
        }
        let second = half(second, "start")
            .output()
            .expect("start the second copy");
        assert!(second.status.success(), "the second copy's half failed");
        drop(serving.stdin.take());
        let mut rest = String::new();
        std::io::Read::read_to_string(&mut said, &mut rest).unwrap();
        assert!(
            serving.wait().unwrap().success(),
            "the first copy's half failed"
        );
        let _ = std::fs::remove_dir_all(&root);
        let said_by_second = String::from_utf8_lossy(&second.stdout).into_owned();
        let second = said_by_second
            .lines()
            .filter_map(marked)
            .find(|line| line.starts_with("D3 LEFT") || line.starts_with("D3 OPENS"))
            .unwrap_or_else(|| {
                panic!(
                    "the second copy says how its start ended:
{said_by_second}"
                )
            })
            .to_owned();
        let landed = rest
            .lines()
            .filter_map(marked)
            .filter_map(|line| line.strip_prefix("D3 LANDED "))
            .map(|line| LaunchRequest::decode(line).expect("a landed launch is a request"))
            .collect();
        Some(TwoCopies { second, landed })
    }

    /// **A copy of this binary at `place` under `copies`, named as this one is** — another
    /// installation of the same program: another file, the same name. Beside this binary's own
    /// folder, so the copy stands on the volume the build does.
    fn another_copy(copies: &Path, place: &str) -> PathBuf {
        let this = std::env::current_exe().unwrap();
        let folder = copies
            .join(place)
            .join("Folio.app")
            .join("Contents")
            .join("MacOS");
        std::fs::create_dir_all(&folder).unwrap();
        let copy = folder.join(this.file_name().unwrap());
        std::fs::copy(&this, &copy).expect("copy this program to another installation");
        copy
    }

    /// **The folder the copies of one test stand in**, beside this binary's own: emptied when it
    /// is made and removed when it is dropped — after the run, and on a panic too — so a build
    /// folder (a CI runner's among them) keeps no copy of the test binary.
    ///
    /// One name per test and not a unique one: on Windows a copy that has just been run can
    /// still be held for a moment after its process has been waited for, so the removal at the
    /// end of a run may not land; the next run of the same test clears what it left, and a
    /// build folder holds at most one copy per test.
    struct Copies(PathBuf);

    impl std::ops::Deref for Copies {
        type Target = Path;

        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Copies {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// [`Copies`] for one test, named by `tag`.
    fn copies_folder(tag: &str) -> Copies {
        let this = std::env::current_exe().unwrap();
        let folder = this.parent().unwrap().join(format!("launch-wire-{tag}"));
        let _ = std::fs::remove_dir_all(&folder);
        Copies(folder)
    }

    /// The folder a launch hands over, named in two scripts, made for one test.
    fn handed_folder() -> PathBuf {
        let folder = bt_testpath::temp_path("工作 文件夹 copy");
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    /// **RED (0.4.8 D3) — a start of a second installed copy, which finds the first copy holding
    /// the data directory, hands its launch over and leaves: its folder lands in the first.**
    ///
    /// The macOS rehearsal's twin (2026-10-07): two copies at two paths, one data directory,
    /// two real processes. The second copy's start is refused the claim, offers its command line
    /// as `main`'s hand-over does, the first copy's own [`decide`] and [`park`] take it, and the
    /// second leaves with 0 — no window of its own. On Windows the same pair is
    /// `…\folio-0.4.6\folio.exe` beside `…\folio-0.4.7\folio.exe`.
    ///
    /// MUTATION (refuse foreign copies): `bt_platform`'s Unix `vet_executable` compares device
    /// and inode again (this bundle, not a Folio); on Windows `vetted_server` compares the whole
    /// image path. The second copy opens its own window: `D3 OPENS … PermissionDenied …`.
    #[test]
    fn a_second_installed_copy_hands_its_launch_to_the_copy_that_holds_the_data_directory() {
        const SELECTOR: &str = "launch_wire::tests::a_second_installed_copy_hands_its_launch_to_the_copy_that_holds_the_data_directory";
        if copy_half(SELECTOR) {
            return;
        }
        let copies = copies_folder("second-copy");
        let second = another_copy(&copies, "other copy");
        let folder = handed_folder();
        let words = vec!["--cwd".to_owned(), folder.to_string_lossy().into_owned()];
        let run = two_copies(
            SELECTOR,
            &std::env::current_exe().unwrap(),
            &second,
            &words,
            false,
        );
        drop(copies);
        let _ = std::fs::remove_dir_all(&folder);
        let Some(run) = run else {
            return;
        };
        assert_eq!(
            run.second, "D3 LEFT 0",
            "the second copy handed its launch over"
        );
        let expected = LaunchRequest::from_cli(
            &argv(&["--cwd", &folder.to_string_lossy()]),
            cli::machine_path_kind,
            None,
        )
        .unwrap();
        assert_eq!(
            run.landed,
            vec![expected],
            "its folder landed in the first copy"
        );
    }

    /// **RED (0.4.8 D3) — the restored version's start after a failed update, from a second
    /// copy, hands its launch and its report to the copy that kept running.**
    ///
    /// The rehearsal's other half: the start a rollback sent (`--update-failed <journal>`, the
    /// pass's verdict `RolledBack`) is the same road with a report in the request (U-36), so it
    /// crosses the same way — folder and report — and leaves with 0, and the running copy's
    /// update card is told where the launch lands (`LaunchRequest::told`, U-36's own test).
    ///
    /// MUTATION (refuse foreign copies), as above: `D3 OPENS … PermissionDenied …`, nothing
    /// landed.
    #[test]
    fn a_second_copys_restored_start_after_a_failed_update_hands_over_with_its_report() {
        const SELECTOR: &str = "launch_wire::tests::a_second_copys_restored_start_after_a_failed_update_hands_over_with_its_report";
        if copy_half(SELECTOR) {
            return;
        }
        let copies = copies_folder("restored-copy");
        let second = another_copy(&copies, "restored copy");
        let folder = handed_folder();
        let journal = copies.join(".folio-update").join("journal.json");
        let words = vec![
            cli::UPDATE_FAILED_FLAG.to_owned(),
            journal.to_string_lossy().into_owned(),
            "--cwd".to_owned(),
            folder.to_string_lossy().into_owned(),
        ];
        let run = two_copies(
            SELECTOR,
            &std::env::current_exe().unwrap(),
            &second,
            &words,
            true,
        );
        drop(copies);
        let _ = std::fs::remove_dir_all(&folder);
        let Some(run) = run else {
            return;
        };
        assert_eq!(
            run.second, "D3 LEFT 0",
            "the restored start handed its launch over"
        );
        let line: Vec<&str> = words.iter().map(String::as_str).collect();
        let expected = LaunchRequest::of_start(
            &argv(&line),
            Some(&Failure::RolledBack),
            cli::machine_path_kind,
            None,
        )
        .unwrap();
        assert_eq!(expected.report, Some(Report::RolledBack));
        assert_eq!(
            run.landed,
            vec![expected],
            "its folder and its report landed in the copy that kept running"
        );
    }

    /// **RED (0.4.8 D3) — a launch endpoint held by a program that is not a Folio is a true
    /// refusal: nothing is written to it, the start opens its own window, and the log line says
    /// why.**
    ///
    /// The other side of the narrowed identity: a Folio is this program's name, and a program
    /// of another name holding the door — here a copy of this binary under another name,
    /// listening as a Folio would — is not handed the folder. The start carries on to its own
    /// window (whose card is the second-instance notice, unchanged) and its give-up line names
    /// the OS's refusal. A copy and not a hard link: macOS names a hard-linked image by
    /// whichever of its links it cached, so the program's name would not be the one it ran as.
    ///
    /// MUTATION (accept any peer): `bt_platform`'s Unix `vet_executable` answers `Ok` whatever
    /// the peer, or Windows' `vetted_server` its pid whatever the image — the start leaves with
    /// 0 and the folder lands in the impostor.
    #[test]
    fn a_door_held_by_a_program_that_is_not_a_folio_is_refused_and_the_log_says_why() {
        const SELECTOR: &str = "launch_wire::tests::a_door_held_by_a_program_that_is_not_a_folio_is_refused_and_the_log_says_why";
        if copy_half(SELECTOR) {
            return;
        }
        let this = std::env::current_exe().unwrap();
        let copies = copies_folder("not-a-folio");
        let folder_of_it = copies.join("another program");
        std::fs::create_dir_all(&folder_of_it).unwrap();
        let impostor =
            folder_of_it.join(format!("another-program{}", std::env::consts::EXE_SUFFIX));
        std::fs::copy(&this, &impostor)
            .expect("stand a copy of this program there under another name");
        let folder = handed_folder();
        let words = vec!["--cwd".to_owned(), folder.to_string_lossy().into_owned()];
        let run = two_copies(SELECTOR, &impostor, &this, &words, false);
        drop(copies);
        let _ = std::fs::remove_dir_all(&folder);
        let Some(run) = run else {
            return;
        };
        assert!(
            run.second
                .starts_with("D3 OPENS Folio: launch hand-over — the Folio that holds ")
                && run
                    .second
                    .contains("did not take this launch (PermissionDenied: "),
            "the start opens its own window and its log line names the refusal: {}",
            run.second
        );
        assert_eq!(
            run.landed,
            Vec::new(),
            "nothing was handed to a program that is not a Folio"
        );
    }
}
