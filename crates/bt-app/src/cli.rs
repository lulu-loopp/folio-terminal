//! `folio.exe`'s front door — what the outside world can say to this program
//! before there is a window to say it to.
//!
//! # Why this exists, and why it is first
//!
//! `docs/spikes/spike-win-landing.md` §8 calls this slice 0 and says why: three
//! of the block's five items are dishonest without it. The Explorer verb the
//! spike measured registers the literal command
//! `"…\folio.exe" --cwd "%V"`, and the spike's own probe log records the fact
//! that makes the flag load-bearing rather than decorative:
//!
//! ```text
//! ARGV argv=["…\shell-probe.exe", "argv", "--cwd", "D:\\Developer\\folio-terminal\\crates"]
//!      cwd="…\probes\target\debug"   elevated=false
//! ```
//!
//! `%V` arrives as an *argument* and the launched process's working directory
//! is the exe's own folder. So Folio must take the place from `--cwd` and must
//! never read `current_dir()` — a build that inherited the process directory
//! would open every right-click in whatever folder `folio.exe` happens to live
//! in, and would look right on a developer's machine, where that is the repo.
//!
//! # Why it is hand-rolled
//!
//! `Cargo.toml`'s dependency policy is `docs/DESIGN.md` §8, and the workspace
//! has no argument parser today. The grammar here is two flags that take a
//! value, one that does not, a help spelling and one positional; a parser crate
//! would be a dependency, a build-time cost and a second set of conventions for
//! the sake of about a hundred lines. What it would buy — `--flag=value`, `--`,
//! a usage block — is written out below, tested, and small enough to read in one
//! sitting.
//!
//! # The two halves
//!
//! [`parse`] is **pure and total over `OsString`**: it turns a command line into
//! a [`CliRequest`] or into the one [`CliFault`] that ends the launch at the
//! door. It asks the machine nothing — not whether a folder exists, not what
//! profiles this build has — because a syntax error and a folder that was
//! deleted yesterday are not the same kind of event and must not be reported
//! the same way. A syntax error stops the program before a window exists; a
//! folder that is gone opens the window anyway and says so on it.
//!
//! [`resolve`] is the second half: the same request, put to this machine and to
//! this build's profile table, coming back as the [`CliPlan`] the launch acts
//! on plus the list of things it could not honour. Its one impure input — what
//! the filesystem says about a path — is handed in, so the whole of the "what
//! happens when the folder is gone" rule can be pinned by a test that touches
//! no disk.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::i18n;
use crate::profiles;

/// What a command line asked for, in the caller's own words.
///
/// Nothing here has been checked against anything. `profile` is the string the
/// caller typed and not an index, `cwd` is a path that may not exist, and the
/// two are still separate from the positional even though a positional folder
/// ends up meaning the same thing as `--cwd` — because the *reports* differ, and
/// a request that had already collapsed them could not say which of the two
/// forms the user actually used.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CliRequest {
    /// `--cwd <folder>` — where the first pane opens.
    pub cwd: Option<PathBuf>,
    /// `--profile <id>` — the [`profiles::Profile::id`] the first pane starts as.
    pub profile: Option<String>,
    /// The bare positional: a folder is a place, a file is a document.
    pub path: Option<PathBuf>,
    /// `--new-window` — **open a window, not a tab in the one you were last
    /// using** (`docs/DESIGN.md` §7.59).
    ///
    /// The opt-out of single instance, and it opts out of the *behaviour* and
    /// not of the *channel*: a second launch carrying this still hands its
    /// request to the Folio that is already running, which opens a second window
    /// of its own. A second `folio.exe` process is started only when nobody
    /// holds the data directory's claim, because that is the condition R4-5's
    /// lock exists to answer and a flag on a command line cannot change it.
    ///
    /// Beside `embedding` and not among the three above it for that field's
    /// reason: it says *how* this launch should land, not what to open. So it
    /// is deliberately not one of [`Self::names_a_place`]'s three — `folio
    /// --new-window` with nothing else asked for a window, not for a pane in a
    /// particular place, and a launch that read it as a place would give the
    /// reader a fresh tab instead of the session they left.
    pub new_window: bool,
    /// `--tab` — **a tab in the window you were last in, not a window of its
    /// own** (`docs/DESIGN.md` §7.59, user ruling 2026-09-11).
    ///
    /// [`Self::new_window`]'s opposite and its twin in every other respect: it
    /// says *how* this launch should land and not what to open, it is answered
    /// by the Folio that is already running, and on a machine with no Folio
    /// running it does nothing at all — a cold launch has no window to put a tab
    /// in, and one process is what it gets either way.
    ///
    /// **Both flags are opt-outs of the same row**, which since the ruling is
    /// `Settings ▸ General ▸ Opening Folio again`. Neither reads the row: the
    /// running Folio does, and only where neither flag was given.
    pub tab: bool,
    /// **Why this launch happened**, as the thing that started it said so.
    ///
    /// Not a decision and deliberately not one — see [`LaunchOrigin`]. The
    /// command line says who is asking; what that means for where the launch
    /// lands is the running Folio's to decide, out of the row and this.
    pub origin: LaunchOrigin,
    /// `-Embedding`, accepted and inert.
    ///
    /// Reserved by `spike-win-landing.md` §8 as part of this slice, and reserved
    /// rather than implemented because the thing that sends it does not exist
    /// yet: COM hands this flag to an out-of-process server it is cold-starting,
    /// which is how slice 3's notification activator will be launched when Folio
    /// is not already running. The reservation is the whole of its value today —
    /// an unrecognised flag ends the launch with a usage block, and the first
    /// notification ever clicked on a cold machine would have been answered with
    /// one.
    pub embedding: bool,
}

/// **Who started this launch** — `docs/DESIGN.md` §7.59.
///
/// **The wire carries this and never the decision it implies.** A launch from Explorer's menu means
/// "a shell in this folder" and a launch from the taskbar means "Folio"; those are different
/// sentences, and which window each of them lands in is one rule, held in one place, in the process
/// that has the settings open. A launcher that spelled the *landing* would be a second copy of that
/// rule, out in the registry and in a `.cmd` file, going stale the day the rule changes — which is
/// exactly what happened to the registered command line the first time round.
///
/// [`Self::Plain`] is what a command line says by saying nothing, so a shortcut, a pinned icon, a
/// double-click and `folio.exe` typed into another shell all arrive as themselves without anybody
/// having to remember to mark them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LaunchOrigin {
    /// A person started Folio: the taskbar, the Start menu, a shortcut, a double-click, or
    /// `folio.exe` with or without `--cwd`.
    #[default]
    Plain,
    /// Explorer's `Open in Folio` / `Open Folio here` — [`EXPLORER_ORIGIN_FLAG`].
    Explorer,
    /// `folio-here.cmd`, which is also what VS Code's external-terminal setting runs —
    /// [`HERE_ORIGIN_FLAG`].
    Here,
}

impl CliRequest {
    /// Whether this command line asked for a place of its own.
    ///
    /// The question the launch actually needs answered — see
    /// `docs/DESIGN.md` §7.2. `-Embedding` is deliberately not one of the three:
    /// it says how this process was started, not what to open.
    #[must_use]
    pub fn names_a_place(&self) -> bool {
        self.cwd.is_some() || self.profile.is_some() || self.path.is_some()
    }
}

/// Why a launch stops at the front door.
///
/// `--help` is one of these, and it is not an error. It is here because the two
/// share their entire tail: a block of text on a console that may not exist, and
/// an exit code. Splitting them would duplicate that tail so that one variant
/// could be spelled `Ok`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CliFault {
    /// `--help`, `-h` or `/?`.
    HelpAsked,
    /// `--version`.
    ///
    /// Beside `--help` and not beside the errors, for the reason the enum's own
    /// note gives: what these two share is their whole tail — a block of text on
    /// a console that may not exist, and an exit code that says nothing went
    /// wrong. What they do not share is the text, which is why
    /// [`refusal_text`] answers this one before it builds a usage block.
    ///
    /// It exists because a preview release is a thing people file bugs against.
    /// Every other way of asking a Windows binary which build it is — the
    /// Properties page, the file's own date — is a way of asking about the
    /// *file*; this is the only one that asks the program, and it is the one a
    /// person pastes into an issue.
    VersionAsked,
    /// A flag that takes a value, given without one.
    MissingValue(&'static str),
    /// A flag this build does not know.
    UnknownFlag(String),
    /// A flag that takes a value, given twice.
    ///
    /// Refused rather than resolved by a last-one-wins rule, which is
    /// `CONVENTIONS.md` §一 applied to a command line: a caller who wrote
    /// `--cwd A --cwd B` has said two things, and a program that silently picks
    /// one of them is guessing on the user's behalf about the one input they
    /// were most explicit about.
    Repeated(&'static str),
    /// A second bare path. One command line names one place.
    ExtraPath(String),
}

impl CliFault {
    /// What the process exits with. `0` for the text somebody asked to read.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::HelpAsked | Self::VersionAsked => 0,
            _ => 2,
        }
    }

    /// The line above the usage block, in the language the window would use.
    ///
    /// `None` for `--help`: the usage block *is* the answer, and a sentence
    /// introducing it would be the program explaining what the user just asked
    /// for.
    #[must_use]
    pub fn notice(&self) -> Option<String> {
        match self {
            // Neither of these is a mistake being reported. `--help` is answered
            // by the usage block alone; `--version` is answered without one.
            Self::HelpAsked | Self::VersionAsked => None,
            Self::MissingValue(flag) => Some(i18n::CliText::MissingValue(flag).text()),
            Self::UnknownFlag(flag) => Some(i18n::CliText::UnknownFlag(flag).text()),
            Self::Repeated(flag) => Some(i18n::CliText::RepeatedFlag(flag).text()),
            Self::ExtraPath(path) => Some(i18n::CliText::ExtraPath(path).text()),
        }
    }
}

/// The whole of what this program answers `--help` with, plus whatever refused
/// the launch.
///
/// Assembled here rather than at the two places that can display it (a console
/// and a message box), because the *text* is one thing and where it comes out is
/// a property of how the process was started.
#[must_use]
pub fn refusal_text(fault: &CliFault) -> String {
    // **Before the usage block is built**, because `--version` does not want
    // one: a person asking which build this is has asked a one-line question,
    // and answering it with a screen of grammar would bury the answer. It is
    // also the one line here that is not translated — see `crate::version`.
    if matches!(fault, CliFault::VersionAsked) {
        return crate::version::banner();
    }
    // The table and not the shipped five: `--profile` resolves through
    // `profiles::index_of_id`, so the ids `--help` prints have to be the ids
    // that door will answer to. Before the table is installed it *is* the
    // shipped five, which is the only state `--help` is ever printed in.
    let ids = (0..profiles::count())
        .map(profiles::id)
        .collect::<Vec<_>>()
        .join(", ");
    let usage = i18n::CliText::Usage { profile_ids: &ids }.text();
    match fault.notice() {
        Some(notice) => format!("{notice}\n\n{usage}"),
        None => usage,
    }
}

/// `--cwd`, spelled once. Every mention of the flag below reads it from here,
/// including the ones inside faults, so a rename cannot leave a message naming
/// the old spelling.
const CWD_FLAG: &str = "--cwd";
/// `--profile`, spelled once — see [`CWD_FLAG`].
const PROFILE_FLAG: &str = "--profile";
/// `--new-window`, spelled once — see [`CWD_FLAG`]. Public because
/// `crate::launch_wire` names it in the one sentence it prints about a launch
/// that could not be handed over, and two spellings of a flag is how a message
/// comes to name one that does not exist.
pub const NEW_WINDOW_FLAG: &str = "--new-window";
/// `--tab`, spelled once — see [`CWD_FLAG`]. Public for [`NEW_WINDOW_FLAG`]'s
/// reason: it is named in the usage block and in `crate::launch_wire`.
pub const TAB_FLAG: &str = "--tab";

/// **The marker Explorer's two entries pass**, spelled once.
///
/// Written by [`bt_platform::context_menu_shape`] into the classic verb's
/// `command` value and by `crate::explorer_menu::serve` onto the child the
/// first-page verb spawns. Deliberately **not** in the usage block: that block
/// lists what a person types, and this is a word one program says to another —
/// `-Embedding` and [`EXPLORER_COMMAND_FLAG`] are already there on that footing.
/// Typing it is harmless and honest; it says of a launch exactly what it says.
pub const EXPLORER_ORIGIN_FLAG: &str = "--from-explorer";

/// **The marker `packaging/folio-here.cmd` passes** — [`EXPLORER_ORIGIN_FLAG`]'s
/// twin, on the one line of that file and therefore on whatever runs it, which
/// today is a shell and VS Code's `terminal.external.windowsExec`.
pub const HERE_ORIGIN_FLAG: &str = "--from-here";

/// Turn a command line into a request, or into the fault that ends the launch.
///
/// The argument is everything **after** the program's own name;
/// `std::env::args_os().skip(1)` at the one real call site, and a literal list
/// in every test.
///
/// # The grammar
///
/// ```text
/// folio [--cwd <folder>] [--profile <id>] [--new-window | --tab] [--] [<path>]
/// folio --help | --version
/// ```
///
/// Plus the two words one program says to another rather than a person typing:
/// `--from-explorer` and `--from-here`, which say where a launch came from (see
/// [`LaunchOrigin`]), and `-Embedding`, which COM appends.
///
/// * `--flag value` and `--flag=value` both work. The second form is not
///   decoration: it is the only way to give a value that begins with `-`, which
///   the first form refuses on purpose (see below).
/// * `--` ends flag parsing. Everything after it is the positional, `-`-leading
///   or not.
/// * A value that begins with `-` is **not** taken from the next argument.
///   `folio --cwd --profile pwsh` is a caller who forgot the folder, not a
///   caller who wants a folder called `--profile`; taking it literally would
///   open a window in a place that cannot exist and report nothing at all.
/// * An argument that is not valid UTF-16-to-UTF-8 is a positional. Flags are
///   ASCII by construction, so a name this build cannot read as text is a path
///   and never a mistyped switch.
pub fn parse<I>(args: I) -> Result<CliRequest, CliFault>
where
    I: IntoIterator<Item = OsString>,
{
    let mut request = CliRequest::default();
    let mut args = args.into_iter();
    let mut positional_only = false;
    while let Some(arg) = args.next() {
        // `to_str` is the flag test and the flag test is `to_str`: every
        // spelling below is ASCII, so an argument this build cannot read as text
        // cannot be one of them, and calling it a path is the only reading that
        // does not lose it.
        let text = arg.to_str().filter(|_| !positional_only);
        match text {
            Some("--") => positional_only = true,
            Some("--help" | "-h" | "/?") => return Err(CliFault::HelpAsked),
            // `-v` is deliberately **not** a spelling of this. It is the one
            // short flag people expect to mean "verbose", and a build that
            // silently printed a version instead of turning on a trace would be
            // answering a different question than the one asked.
            Some("--version") => return Err(CliFault::VersionAsked),
            // Case-insensitive, and both sigils, because this one is not typed
            // by a person: it is whatever the COM activation path on the machine
            // in front of us hands over, and `-Embedding` / `/Embedding` are
            // both in the wild.
            Some(flag)
                if flag.eq_ignore_ascii_case("-Embedding")
                    || flag.eq_ignore_ascii_case("/Embedding") =>
            {
                request.embedding = true;
            }
            // **Exact, and not through [`is_flag`]**: this one takes no value,
            // so `--new-window=1` is a caller who believes it does and is
            // answered as the unknown flag it is rather than silently accepted
            // with its value dropped on the floor.
            Some(flag) if flag == NEW_WINDOW_FLAG => {
                if request.new_window {
                    return Err(CliFault::Repeated(NEW_WINDOW_FLAG));
                }
                request.new_window = true;
            }
            // [`NEW_WINDOW_FLAG`]'s arm word for word, including the reason it
            // is exact rather than through [`is_flag`]: `--tab=1` is a caller
            // who believes this takes a value.
            Some(flag) if flag == TAB_FLAG => {
                if request.tab {
                    return Err(CliFault::Repeated(TAB_FLAG));
                }
                request.tab = true;
            }
            // **The two origin markers, and one rule for a line that carries
            // both.** They are written by two different programs and a launch
            // has one origin, so `--from-explorer --from-here` is a line no
            // launcher this build ships can produce. It is still answered rather
            // than refused, and answered the way the landing rule answers its
            // own overlap: the first word wins, because both of these say the
            // same thing about where the launch lands and the difference between
            // them is only *who* is asking.
            Some(flag) if flag == EXPLORER_ORIGIN_FLAG => {
                if request.origin == LaunchOrigin::Explorer {
                    return Err(CliFault::Repeated(EXPLORER_ORIGIN_FLAG));
                }
                if request.origin == LaunchOrigin::Plain {
                    request.origin = LaunchOrigin::Explorer;
                }
            }
            Some(flag) if flag == HERE_ORIGIN_FLAG => {
                if request.origin == LaunchOrigin::Here {
                    return Err(CliFault::Repeated(HERE_ORIGIN_FLAG));
                }
                if request.origin == LaunchOrigin::Plain {
                    request.origin = LaunchOrigin::Here;
                }
            }
            Some(flag) if is_flag(flag, CWD_FLAG) => {
                if request.cwd.is_some() {
                    return Err(CliFault::Repeated(CWD_FLAG));
                }
                request.cwd = Some(PathBuf::from(value_for(CWD_FLAG, flag, &arg, &mut args)?));
            }
            Some(flag) if is_flag(flag, PROFILE_FLAG) => {
                if request.profile.is_some() {
                    return Err(CliFault::Repeated(PROFILE_FLAG));
                }
                let value = value_for(PROFILE_FLAG, flag, &arg, &mut args)?;
                // A profile id is an ASCII slug in this build's own table, so a
                // value that is not text cannot name one — and the report has to
                // name what was given, which is what `to_string_lossy` is for.
                request.profile = Some(value.to_string_lossy().into_owned());
            }
            Some(flag) if flag.starts_with('-') => {
                return Err(CliFault::UnknownFlag(flag.to_owned()));
            }
            _ => {
                if request.path.is_some() {
                    return Err(CliFault::ExtraPath(arg.to_string_lossy().into_owned()));
                }
                request.path = Some(PathBuf::from(arg));
            }
        }
    }
    Ok(request)
}

/// Whether `text` is `name`, in either of the two spellings a value can take.
///
/// One predicate for the match guard and the splitter both, so that the arm that
/// *accepts* `--cwd=x` and the code that *reads* the `x` out of it cannot come
/// to disagree about where the sign is.
fn is_flag(text: &str, name: &str) -> bool {
    text == name || (text.starts_with(name) && text.as_bytes().get(name.len()) == Some(&b'='))
}

/// The value of a flag written either way, or the fault of one written with none.
///
/// `text` is the argument decoded and `arg` is the argument itself: the `=` form
/// is split off the **encoded** argument rather than off the decoded copy,
/// because the half after the sign is a path and a path is not required to be
/// text. `--cwd=` is ASCII, so the sign sits at the same offset in both, and
/// what comes back is exactly what the operating system said.
///
/// Called only where [`is_flag`] has already said yes, which is what makes
/// "longer than the name" the test for the joined form.
///
/// # The split is portable, and it was not (M1-1, for M1-10)
///
/// This function used to reach for `std::os::windows::ffi` — one of two ungated
/// Windows uses in this file, which `scripts/check-portable-core.ps1` never sees
/// because it scans the thirteen portable crates and `bt-app` is not one of
/// them (`docs/plans/port/macos-plan-2026-09-12.md` §4.3). The plan's
/// recommendation was the second of its two options: give this function a
/// portable implementation rather than admit `cli.rs` to the eleven-file gate
/// list, **because what it actually does is split at a known ASCII offset** and
/// both platforms can express that.
///
/// The split itself is `bt_platform::argument_after_ascii`, and it is there
/// rather than here for the reason the workspace's `unsafe_code = "deny"`
/// states: taking an `OsStr` apart in the operating system's own encoding and
/// putting one back is one `unsafe` call in the standard library, and this
/// workspace keeps every one of those behind that one door.
///
/// The *test* beside this function has no portable twin at all and is M1-10's
/// second decision: a lone surrogate is not a thing a Unix `OsString` can hold.
fn value_for(
    name: &'static str,
    text: &str,
    arg: &OsString,
    args: &mut impl Iterator<Item = OsString>,
) -> Result<OsString, CliFault> {
    if text.len() > name.len() {
        // `is_flag` has already found the `=` at `name.len()`, so the value is
        // everything after it.
        let value = bt_platform::argument_after_ascii(arg, name.len() + 1);
        if value.is_empty() {
            return Err(CliFault::MissingValue(name));
        }
        return Ok(value);
    }
    let next = args.next().ok_or(CliFault::MissingValue(name))?;
    if next
        .to_str()
        .is_some_and(|text| text.starts_with('-') && text.len() > 1)
    {
        return Err(CliFault::MissingValue(name));
    }
    if next.is_empty() {
        return Err(CliFault::MissingValue(name));
    }
    Ok(next)
}

/// What the machine says about a path a caller named.
///
/// Three answers and not `Option<bool>`, because "there is nothing there" and
/// "there is something there and it is not a folder" are reported to the user in
/// different words, and a boolean would have to be read alongside a second call
/// to tell them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    Directory,
    File,
    /// Nothing at that name, or something that is neither — a device, a broken
    /// link, a name this process may not look at.
    Absent,
}

/// **A folder somebody named, written the way a command line writes places** — absolute, and with
/// `.` and `..` spent rather than carried.
///
/// The one place in this program that turns a relative path a person typed into an absolute one,
/// and it is a pure function of two paths so that the rule can be read and tested without a disk.
/// `here` is the working directory of the process that was *started*, handed in rather than read,
/// because [`parse`] and everything reachable from it is pure over its inputs — and because a
/// caller that had nothing to resolve against must be able to say so.
///
/// **Lexical and never `canonicalize`.** `std::fs::canonicalize` answers a `\\?\` verbatim path,
/// which is exactly the spelling `bt_transcript::paths::is_local_absolute_path` refuses and which
/// no shell would ever print; what a person means by `..\sibling` is what `cmd` and PowerShell
/// would show them, which is the lexical answer. A path with nothing to resolve — already
/// absolute, or a `here` this process could not read — comes back unchanged, which leaves it to be
/// judged by the same door it would have met anyway.
#[must_use]
pub fn absolute_from(here: Option<&Path>, folder: &Path) -> PathBuf {
    use std::path::Component;

    if folder.is_absolute() {
        return folder.to_path_buf();
    }
    let Some(here) = here else {
        return folder.to_path_buf();
    };
    let mut out = PathBuf::new();
    for component in here.join(folder).components() {
        match component {
            // A prefix or a root restarts the answer: `C:\a` joined onto `D:\b` is `C:\a`, which is
            // what `join` already decided and what this loop must not undo.
            Component::Prefix(_) | Component::RootDir => {
                if matches!(component, Component::Prefix(_)) {
                    out = PathBuf::new();
                }
                out.push(component.as_os_str());
            }
            Component::CurDir => {}
            // **Only a real name is walked back over.** `..` above a root is nothing — every shell
            // agrees — and popping a prefix or a root would turn an absolute path into a relative
            // one halfway through.
            Component::ParentDir => {
                if out
                    .components()
                    .next_back()
                    .is_some_and(|last| matches!(last, Component::Normal(_)))
                {
                    out.pop();
                }
            }
            Component::Normal(name) => out.push(name),
        }
    }
    out
}

/// [`PathKind`] as this machine answers it. The one impure input [`resolve`]
/// takes, named here so that every test can hand in a table instead.
#[must_use]
pub fn machine_path_kind(path: &Path) -> PathKind {
    if path.is_dir() {
        PathKind::Directory
    } else if path.is_file() {
        PathKind::File
    } else {
        PathKind::Absent
    }
}

/// What a launch does with a command line, once the machine has been asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliPlan {
    /// Whether the caller asked for a place of its own — the switch
    /// `docs/DESIGN.md` §7.2's composition rule turns on.
    ///
    /// True even when everything in the request was refused: a caller who named
    /// a folder that has been deleted still asked for a fresh pane, and giving
    /// them the last session's tabs and nothing else would answer a question
    /// they did not ask.
    pub wants_pane: bool,
    /// Which profile that pane starts as — the caller's, or this machine's
    /// default when they named none or named one this build has not got.
    pub profile: usize,
    /// Where it opens, **already in that profile's namespace**, or `None` for
    /// "wherever a fresh shell of it would".
    pub cwd: Option<PathBuf>,
    /// A document to open a preview on, once there is a window.
    pub preview: Option<PathBuf>,
    /// Everything the caller asked for that this launch could not do. One card
    /// each, on the window, after it opens.
    pub refusals: Vec<CliRefusal>,
}

/// The verb that makes this program something a hook can call.
///
/// `folio attention <family>:<event> [--json <payload>]` — see [`attention`] for the grammar and
/// `crate::attention_wire` for what it does with it.
pub const ATTENTION_VERB: &str = "attention";

/// One call of `folio attention`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttentionCall {
    /// `<family>:<event>`, exactly as the hook's own configuration spelled it.
    pub event: String,
    /// The hook's payload, if the caller passed one. **It does not leave this process** — see
    /// `crate::attention_wire`'s header — and the only things ever taken out of it are the two
    /// fields a row of the mapping table declared.
    pub payload: Option<AttentionPayload>,
}

/// **Where one call's payload is**, which is a question two upstreams answer differently.
///
/// codex appends its payload to the `notify` array it spawns, so it arrives as an argument. Claude
/// Code writes its own onto the hook process's standard input and closes the handle. Neither is
/// wrong and neither can be talked out of it, so the command line says which, and the grammar stays
/// a grammar: nothing is read off a handle while a command line is being parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttentionPayload {
    /// `--json <text>`: the payload itself, on the command line.
    Inline(String),
    /// `--json -`: the payload is on this process's standard input.
    Stdin,
}

/// What `--json` is given when the payload is on standard input.
///
/// The oldest spelling there is for it, and the reason to use it rather than a flag of our own: a
/// person reading a hook entry in their own settings file has seen this one before.
pub const STDIN_PAYLOAD: &str = "-";

/// Why a call of the verb stops before it says anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttentionFault {
    /// `folio attention` with no event, or with `--help`.
    NothingAsked,
    /// `--json` with nothing after it.
    MissingValue(&'static str),
    /// A flag the verb does not know.
    UnknownFlag(String),
    /// A second event. One call says one thing.
    ExtraEvent(String),
}

/// **The one subcommand, recognised before the flag grammar is entered at all.**
///
/// `None` when the first argument is not the verb, which is every ordinary launch — so a window
/// opening pays one string comparison for the existence of this door.
///
/// Separate from [`parse`] rather than folded into it, and the reason is what the two are for. The
/// flag grammar answers *what should this window open*; a hook is not opening a window, it is
/// ringing a doorbell and leaving. Folding the two would mean every future flag had to be thought
/// about twice — once for a window and once for a doorbell — and the second thought is the one that
/// would be forgotten.
///
/// The event is one positional and is **not** validated here: whether a name is one this build has
/// a mapping for is a question for the tables, and a syntax that had to be kept in step with a data
/// file would be a syntax that goes stale the day a family is added.
pub fn attention<I>(args: I) -> Option<Result<AttentionCall, AttentionFault>>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    if args.next()?.to_str()? != ATTENTION_VERB {
        return None;
    }
    Some(attention_arguments(args))
}

/// The switch `packaging/msix/AppxManifest.xml` puts on the `ExeServer`'s
/// command line, spelled once.
pub const EXPLORER_COMMAND_FLAG: &str = "--explorer-command";

/// Whether this launch is Explorer asking for a COM class rather than a person
/// asking for a window (§7.4a).
///
/// **The first argument and only the first.** The manifest writes this switch
/// and COM appends its own `-Embedding` after it, so first is where it always
/// is; anything after it belongs to COM and is deliberately not parsed —
/// refusing a word COM invented would be this process refusing its own caller.
/// Reading it anywhere on the line would let `folio --cwd D:\x
/// --explorer-command` become a server, which is a window somebody asked for
/// that never appears.
///
/// A third door beside [`attention`] and [`parse`], for [`attention`]'s reason
/// taken one step further: this launch is not opening a window and is not
/// ringing a doorbell either — it is answering questions until another program
/// stops asking, and it must reach that loop before anything in this process
/// builds a swap chain.
pub fn explorer_command<I>(args: I) -> bool
where
    I: IntoIterator<Item = OsString>,
{
    args.into_iter()
        .next()
        .is_some_and(|first| first.to_str() == Some(EXPLORER_COMMAND_FLAG))
}

/// `--json`, spelled once.
const JSON_FLAG: &str = "--json";

fn attention_arguments(
    mut args: impl Iterator<Item = OsString>,
) -> Result<AttentionCall, AttentionFault> {
    let mut event = None;
    let mut payload = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--help" | "-h" | "/?") => return Err(AttentionFault::NothingAsked),
            Some(flag) if is_flag(flag, JSON_FLAG) => {
                if payload.is_some() {
                    return Err(AttentionFault::UnknownFlag(JSON_FLAG.to_owned()));
                }
                let value = value_for(JSON_FLAG, flag, &arg, &mut args)
                    .map_err(|_| AttentionFault::MissingValue(JSON_FLAG))?;
                let value = value.to_string_lossy().into_owned();
                payload = Some(if value == STDIN_PAYLOAD {
                    AttentionPayload::Stdin
                } else {
                    AttentionPayload::Inline(value)
                });
            }
            Some(flag) if flag.starts_with("--") => {
                return Err(AttentionFault::UnknownFlag(flag.to_owned()));
            }
            _ => {
                let named = arg.to_string_lossy().into_owned();
                if event.is_some() {
                    return Err(AttentionFault::ExtraEvent(named));
                }
                event = Some(named);
            }
        }
    }
    match event {
        Some(event) => Ok(AttentionCall { event, payload }),
        None => Err(AttentionFault::NothingAsked),
    }
}

/// One thing a command line asked for and did not get.
///
/// **Said out loud, never silently substituted** — the rule `LeafSeed`'s
/// `unknown_profile_id` field exists for, applied at the other door. A launch
/// that fell back to the default profile and the default folder without a word
/// looks exactly like a launch nobody passed any arguments to, and the caller
/// most likely to hit it is a shortcut or a registry verb whose text they cannot
/// see.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CliRefusal {
    /// `--cwd` named something that is not a folder — gone, or a file.
    NoSuchFolder(PathBuf),
    /// `--profile` named an id this build has not got.
    NoSuchProfile(String),
    /// The bare positional named nothing at all.
    NoSuchPath(PathBuf),
    /// A real folder that the chosen profile cannot name.
    ///
    /// `--cwd \\server\share --profile wsl`: the folder exists, and there is no
    /// path in the Linux namespace that means it. The alternative was to open at
    /// the WSL home without comment, which is the silent substitution above.
    UnreachableFolder { folder: PathBuf, profile: usize },
    /// A positional folder given alongside `--cwd`. The flag wins, and the one
    /// that lost is named.
    ///
    /// **It wins whether or not it named a folder that exists.** The tempting
    /// second rule — fall to the positional when `--cwd` turned out to be gone —
    /// would make what a command line opens depend on the state of a folder
    /// somewhere else, so the same two arguments would land in two different
    /// places on two machines. One rule: the flag is the place, and both of the
    /// caller's own words are read back to them.
    PlaceAlreadyNamed(PathBuf),
}

impl CliRefusal {
    /// The card's body, in the language the window is drawing in.
    #[must_use]
    pub fn notice(&self) -> String {
        match self {
            Self::NoSuchFolder(folder) => {
                i18n::CliText::NoSuchFolder(&folder.to_string_lossy()).text()
            }
            Self::NoSuchProfile(id) => i18n::CliText::NoSuchProfile(id).text(),
            Self::NoSuchPath(path) => i18n::CliText::NoSuchPath(&path.to_string_lossy()).text(),
            Self::UnreachableFolder { folder, profile } => i18n::CliText::UnreachableFolder {
                profile_title: profiles::title(*profile),
                folder: &folder.to_string_lossy(),
            }
            .text(),
            Self::PlaceAlreadyNamed(folder) => {
                i18n::CliText::PlaceAlreadyNamed(&folder.to_string_lossy()).text()
            }
        }
    }
}

/// Put a request to this build and this machine.
///
/// `default_profile` is the resolved `settings.json` default — the same number
/// `create_tab_state` starts a seatless terminal as, handed in for the same
/// reason it is handed in there: a `usize`'s own `Default` is `0`, which is the
/// right profile only for as long as the default is a constant.
///
/// The order matters and is the order a reader would guess: the profile is
/// settled first because the folder's namespace depends on it, and the
/// positional is settled last because whether it is a place at all depends on
/// what `--cwd` already said.
pub fn resolve(
    request: &CliRequest,
    default_profile: usize,
    kind: impl Fn(&Path) -> PathKind,
) -> CliPlan {
    let mut refusals = Vec::new();
    let profile = match request.profile.as_deref() {
        Some(id) if profiles::has_id(id) => profiles::index_of_id(id),
        Some(id) => {
            refusals.push(CliRefusal::NoSuchProfile(id.to_owned()));
            default_profile
        }
        None => default_profile,
    };
    let mut preview = None;
    // The two forms of "here", in the order that decides which one is heard.
    let mut folder = match request.cwd.as_deref() {
        Some(cwd) if kind(cwd) == PathKind::Directory => Some(cwd.to_path_buf()),
        Some(cwd) => {
            refusals.push(CliRefusal::NoSuchFolder(cwd.to_path_buf()));
            None
        }
        None => None,
    };
    if let Some(path) = request.path.as_deref() {
        match kind(path) {
            PathKind::Directory => {
                if request.cwd.is_some() {
                    refusals.push(CliRefusal::PlaceAlreadyNamed(path.to_path_buf()));
                } else {
                    folder = Some(path.to_path_buf());
                }
            }
            PathKind::File => preview = Some(path.to_path_buf()),
            PathKind::Absent => refusals.push(CliRefusal::NoSuchPath(path.to_path_buf())),
        }
    }
    // **A command line is written in Windows paths**, whichever shell it names.
    // `%V` is a Windows path, a `cmd` line is a Windows path, and the profile
    // that pane starts as may not speak them — so the crossing is asked here,
    // through the same function a split's folder chooser goes through, and the
    // pairs that cannot cross are reported rather than dropped.
    let cwd = folder.and_then(|folder| {
        let crossed = profiles::translate_cwd(
            profiles::PathNamespace::Windows,
            profiles::paths(profile),
            &folder,
        );
        if crossed.is_none() {
            refusals.push(CliRefusal::UnreachableFolder { folder, profile });
        }
        crossed
    });
    CliPlan {
        wants_pane: request.names_a_place(),
        profile,
        cwd,
        preview,
        refusals,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    fn parsed(list: &[&str]) -> CliRequest {
        parse(args(list)).expect("this command line was meant to parse")
    }

    fn refused(list: &[&str]) -> CliFault {
        parse(args(list)).expect_err("this command line was meant to be refused")
    }

    /// PIN (§7.4a) — **the COM server's door opens on the first word and on
    /// nothing else, and COM's own words do not close it.**
    ///
    /// Three things at once, and each is a different failure. COM appends
    /// `-Embedding` to what the manifest wrote, so a door that wanted the switch
    /// alone would never open and the first-page menu item would silently never
    /// appear. An ordinary launch must pay one comparison and go on, so an empty
    /// command line and `--cwd` are both `false`. And the switch must **not** be
    /// read anywhere on the line: `folio --cwd D:\x --explorer-command` is a
    /// person asking for a window, and answering it with a server is a window
    /// that never opens.
    ///
    /// MUTATION: search the whole line instead of the first word and the last
    /// assertion goes red.
    #[test]
    fn explorer_starts_this_process_by_its_first_word() {
        assert!(explorer_command(args(&["--explorer-command"])));
        assert!(explorer_command(args(&[
            "--explorer-command",
            "-Embedding"
        ])));
        assert!(explorer_command(args(&[
            "--explorer-command",
            "-Embedding",
            "/whatever"
        ])));
        assert!(!explorer_command(args(&[])));
        assert!(!explorer_command(args(&["--cwd", r"D:\x"])));
        assert!(!explorer_command(args(&["-Embedding"])));
        assert!(!explorer_command(args(&[
            "--cwd",
            r"D:\x",
            "--explorer-command"
        ])));
        // And it is not a flag `parse` knows: a command line that reached the
        // window's grammar carrying it is one this process should refuse rather
        // than open a window for.
        assert_eq!(
            refused(&["--explorer-command"]),
            CliFault::UnknownFlag(EXPLORER_COMMAND_FLAG.to_owned())
        );
    }

    /// PIN — **the empty command line asks for nothing**, which is the launch
    /// every user who double-clicks the exe performs.
    ///
    /// MUTATION: make `names_a_place` answer `true` for a default request and
    /// `docs/DESIGN.md` §7.2's composition inverts — every launch would open a
    /// fresh tab in front of the restored ones, and the one-tab shortcut in
    /// `plan_launch` would stop firing for anybody.
    #[test]
    fn no_arguments_at_all_is_a_request_that_names_no_place() {
        let request = parsed(&[]);
        assert_eq!(request, CliRequest::default());
        assert!(!request.names_a_place());
    }

    /// PIN — the three forms of "open here", each read on its own.
    #[test]
    fn each_of_the_three_place_forms_is_read_and_each_one_names_a_place() {
        assert_eq!(
            parsed(&["--cwd", r"D:\Developer"]).cwd,
            Some(PathBuf::from(r"D:\Developer"))
        );
        assert_eq!(
            parsed(&["--profile", "winps"]).profile.as_deref(),
            Some("winps")
        );
        assert_eq!(
            parsed(&[r"D:\Developer"]).path,
            Some(PathBuf::from(r"D:\Developer"))
        );
        for line in [
            vec!["--cwd", r"D:\Developer"],
            vec!["--profile", "winps"],
            vec![r"D:\Developer"],
        ] {
            assert!(parsed(&line).names_a_place(), "{line:?} names a place");
        }
    }

    /// **RED (§7.59) — `--new-window` is read, and it is not a place.**
    ///
    /// Four things at once and each is a different failure. It parses at all,
    /// which before this slice was `CliFault::UnknownFlag` and a usage block. It
    /// does **not** name a place: `folio --new-window` asked for a window, not
    /// for a fresh pane, and a launch that read it as a place would hand the
    /// reader an empty tab instead of the session they left — which is §7.2's
    /// composition rule turned on by the wrong switch. It rides beside the three
    /// that *are* places, because `folio --new-window --cwd D:\x` is the ordinary
    /// way to ask for a second window on a folder. And it is refused twice, on
    /// the grammar's own rule for a flag given twice.
    ///
    /// MUTATION: fold it into `names_a_place` and the second assertion goes red;
    /// accept it through `is_flag` and `--new-window=1` stops being refused.
    #[test]
    fn a_window_of_its_own_is_asked_for_by_a_flag_that_names_no_place() {
        assert!(parsed(&[NEW_WINDOW_FLAG]).new_window);
        assert!(
            !parsed(&[NEW_WINDOW_FLAG]).names_a_place(),
            "asking for a window is not asking for a pane somewhere"
        );
        let both = parsed(&[NEW_WINDOW_FLAG, "--cwd", r"D:\Developer"]);
        assert!(both.new_window);
        assert_eq!(both.cwd, Some(PathBuf::from(r"D:\Developer")));
        assert!(!parsed(&["--cwd", r"D:\Developer"]).new_window);
        assert_eq!(
            refused(&[NEW_WINDOW_FLAG, NEW_WINDOW_FLAG]),
            CliFault::Repeated(NEW_WINDOW_FLAG)
        );
        assert_eq!(
            refused(&["--new-window=1"]),
            CliFault::UnknownFlag("--new-window=1".to_owned()),
            "it takes no value, so a caller who gave it one is told the flag \
             they wrote does not exist rather than having their value dropped"
        );
    }

    /// PIN — `--flag=value` is the same value as `--flag value`.
    ///
    /// Both spellings, on both flags, because the `=` half is parsed by hand out
    /// of the `OsString` and a copy-paste that stripped the wrong prefix would
    /// leave one flag reading `=D:\Developer`.
    #[test]
    fn the_joined_spelling_carries_the_same_value_as_the_separated_one() {
        assert_eq!(
            parsed(&[r"--cwd=D:\Developer"]),
            parsed(&["--cwd", r"D:\Developer"])
        );
        assert_eq!(parsed(&["--profile=wsl"]), parsed(&["--profile", "wsl"]));
    }

    /// PIN — everything the grammar accepts at once, in one line.
    #[test]
    fn one_command_line_can_carry_a_folder_a_profile_and_a_document() {
        let request = parsed(&[
            "--cwd",
            r"D:\Developer",
            "--profile",
            "gitbash",
            r"D:\a\b.rs",
        ]);
        assert_eq!(
            request,
            CliRequest {
                cwd: Some(PathBuf::from(r"D:\Developer")),
                profile: Some("gitbash".to_owned()),
                path: Some(PathBuf::from(r"D:\a\b.rs")),
                embedding: false,
                new_window: false,
                tab: false,
                origin: LaunchOrigin::Plain,
            }
        );
    }

    /// **RED (review C-6, 2026-09-11) — a folder somebody named is made absolute the way a shell
    /// would print it, and nothing else is touched.**
    ///
    /// The pure half of the fix for `folio .`: it opened a shell in the right folder on a cold
    /// machine and was refused on a warm one, because the wire's gate wants a drive-rooted path and
    /// nothing had ever turned the one the person typed into one.
    ///
    /// MUTATIONS: keep the `.` components and the answer stops being a path any gate recognises;
    /// let `..` walk over a root and an absolute path comes back relative; drop the prefix arm and
    /// a drive-qualified path joined onto another drive keeps the wrong drive.
    #[test]
    fn a_named_folder_is_made_absolute_lexically_and_nothing_else_is() {
        let here = Path::new(r"D:\Developer\Ledger");
        let asked = |folder: &str| absolute_from(Some(here), Path::new(folder));
        assert_eq!(asked("."), PathBuf::from(r"D:\Developer\Ledger"));
        assert_eq!(
            asked("crates"),
            PathBuf::from(r"D:\Developer\Ledger\crates")
        );
        assert_eq!(
            asked(r"crates\bt-app"),
            PathBuf::from(r"D:\Developer\Ledger\crates\bt-app")
        );
        assert_eq!(asked(".."), PathBuf::from(r"D:\Developer"));
        assert_eq!(
            asked(r"..\bt-wt\launch-window"),
            PathBuf::from(r"D:\Developer\bt-wt\launch-window")
        );
        assert_eq!(
            asked(r"..\..\..\..\..\.."),
            PathBuf::from(r"D:\"),
            "a walk above the root stops at the root, which is what every shell does"
        );
        assert_eq!(
            asked(r"D:\Other"),
            PathBuf::from(r"D:\Other"),
            "a folder that was already absolute is left exactly as it was written"
        );
        assert_eq!(
            absolute_from(None, Path::new(".")),
            PathBuf::from("."),
            "a caller with no working directory to resolve against resolves nothing"
        );
    }

    /// PIN — **a UNC path is a path**, in both the flag and the positional.
    ///
    /// It is written out because `\\server\share` is the one Windows path shape
    /// whose first character is also a path separator: a parser that trimmed
    /// leading separators, or that treated the argument as a `&str` and split on
    /// them, would hand back `server\share` and open a window in a folder that
    /// does not exist. `%V` produces this shape for every right-click on a
    /// mapped network location.
    #[test]
    fn a_unc_path_survives_both_doors_intact() {
        assert_eq!(
            parsed(&["--cwd", r"\\server\share\team"]).cwd,
            Some(PathBuf::from(r"\\server\share\team"))
        );
        assert_eq!(
            parsed(&[r"\\server\share\team"]).path,
            Some(PathBuf::from(r"\\server\share\team"))
        );
    }

    /// PIN — **a folder whose name has spaces in it arrives as one argument.**
    ///
    /// The quoting is Windows' own job and is over before `args_os` yields
    /// anything, so what this pins is the half that is ours: nothing here splits
    /// an argument on whitespace, and a value with a space in it is not a value
    /// followed by a stray positional.
    #[test]
    fn a_quoted_folder_with_spaces_is_one_value_and_not_two_arguments() {
        let request = parsed(&["--cwd", r"C:\Program Files\Some App"]);
        assert_eq!(
            request.cwd,
            Some(PathBuf::from(r"C:\Program Files\Some App"))
        );
        assert_eq!(request.path, None);
    }

    /// PIN — `--` hands the rest to the positional, `-`-leading or not.
    #[test]
    fn a_double_dash_ends_the_flags_and_what_follows_is_the_path() {
        let request = parsed(&["--", "-h"]);
        assert_eq!(request.path, Some(PathBuf::from("-h")));
        let request = parsed(&["--cwd", r"D:\x", "--", "--profile"]);
        assert_eq!(request.cwd, Some(PathBuf::from(r"D:\x")));
        assert_eq!(request.path, Some(PathBuf::from("--profile")));
    }

    /// PIN — the three spellings of "tell me what this takes", and the exit code
    /// that says it was not an error.
    #[test]
    fn every_spelling_of_help_asks_for_the_usage_and_exits_clean() {
        for spelling in ["--help", "-h", "/?"] {
            let fault = refused(&[spelling]);
            assert_eq!(fault, CliFault::HelpAsked);
            assert_eq!(fault.exit_code(), 0);
            assert_eq!(fault.notice(), None, "the usage block is the whole answer");
        }
        // It is answered wherever it appears, because a caller who typed it has
        // stopped caring about the rest of the line.
        assert_eq!(refused(&["--cwd", r"D:\x", "--help"]), CliFault::HelpAsked);
    }

    /// PIN — **`--version` is one line, is not an error, and is not `-v`.**
    ///
    /// Red gate: leave it out and the flag falls into `UnknownFlag`, which
    /// prints a usage block and exits `2` — a preview release whose users cannot
    /// say which build they have, and a `--version` that a packaging script
    /// reads as a failure.
    #[test]
    fn asking_which_build_this_is_is_answered_in_one_line_and_exits_clean() {
        let fault = refused(&["--version"]);
        assert_eq!(fault, CliFault::VersionAsked);
        assert_eq!(fault.exit_code(), 0);
        assert_eq!(fault.notice(), None, "nothing went wrong");

        let text = refusal_text(&fault);
        assert_eq!(text, crate::version::banner());
        assert_eq!(text.lines().count(), 1, "one line: {text}");
        assert!(text.contains(crate::version::VERSION), "{text}");
        assert!(
            !text.contains("--cwd"),
            "a version is not answered with a usage block: {text}"
        );
        assert!(
            text.contains(crate::APP_NAME),
            "and it says what it is the version of: {text}"
        );

        assert_eq!(
            refused(&["-v"]),
            CliFault::UnknownFlag("-v".to_owned()),
            "the short flag everyone means `verbose` by is not taken"
        );
        // The usage block offers it, because a person who runs `--help` to find
        // out what this takes is exactly the person who needs it.
        assert!(refusal_text(&CliFault::HelpAsked).contains("--version"));
    }

    /// PIN — every refusal that is an actual mistake exits `2`, and every one of
    /// them says which argument it is about.
    ///
    /// MUTATION: return `Ok(CliRequest::default())` for an unknown flag and this
    /// fails on the first case — a typo would otherwise open an ordinary window
    /// and lose the argument in silence.
    #[test]
    fn every_malformed_command_line_is_named_refused_and_exits_two() {
        let cases = [
            (vec!["--cwd"], CliFault::MissingValue("--cwd")),
            (vec!["--profile"], CliFault::MissingValue("--profile")),
            (vec!["--cwd="], CliFault::MissingValue("--cwd")),
            (vec!["--cwd", ""], CliFault::MissingValue("--cwd")),
            // The forgotten value, which is the whole reason a `-`-leading token
            // is not taken as one.
            (
                vec!["--cwd", "--profile", "pwsh"],
                CliFault::MissingValue("--cwd"),
            ),
            (vec!["--nope"], CliFault::UnknownFlag("--nope".to_owned())),
            (vec!["-x"], CliFault::UnknownFlag("-x".to_owned())),
            (
                vec!["--cwd", r"D:\a", "--cwd", r"D:\b"],
                CliFault::Repeated("--cwd"),
            ),
            (
                vec!["--profile", "pwsh", "--profile=cmd"],
                CliFault::Repeated("--profile"),
            ),
            (
                vec![r"D:\a", r"D:\b"],
                CliFault::ExtraPath(r"D:\b".to_owned()),
            ),
        ];
        for (line, expected) in cases {
            let fault = refused(&line);
            assert_eq!(fault, expected, "{line:?}");
            assert_eq!(fault.exit_code(), 2, "{line:?}");
            let notice = fault
                .notice()
                .expect("a mistake owes the caller a sentence");
            assert!(!notice.trim().is_empty(), "{line:?}");
        }
    }

    /// PIN — **a value that begins with `-` can still be given**, through the
    /// one spelling that cannot be confused with a missing one.
    ///
    /// Without this the refusal above would be a hole rather than a rule: there
    /// would be no way at all to name a folder called `-tmp`.
    #[test]
    fn the_joined_spelling_is_the_way_to_give_a_value_that_looks_like_a_flag() {
        assert_eq!(
            parsed(&[r"--cwd=-tmp"]).cwd,
            Some(PathBuf::from("-tmp")),
            "the escape hatch the separated form refuses"
        );
    }

    /// PIN (`spike-win-landing.md` §8) — **`-Embedding` is reserved, accepted
    /// and inert**, in both sigils and any case.
    ///
    /// MUTATION: delete the arm and this fails as `UnknownFlag`, which is
    /// exactly what a cold COM activation would have been answered with.
    #[test]
    fn the_reserved_com_flag_is_accepted_and_asks_for_no_place() {
        for spelling in ["-Embedding", "/Embedding", "-embedding"] {
            let request = parsed(&[spelling]);
            assert!(request.embedding, "{spelling}");
            assert!(!request.names_a_place(), "{spelling} is not a place");
        }
    }

    /// PIN — an argument this build cannot read as text is a path, not a flag.
    #[test]
    fn an_argument_that_is_not_text_is_taken_as_the_path() {
        use std::os::windows::ffi::OsStringExt;
        // A lone high surrogate: a name Windows will hand over and `to_str`
        // will refuse.
        let unreadable = OsString::from_wide(&[0x0044, 0xD800, 0x005C]);
        let request = parse(vec![unreadable.clone()]).expect("a name is not a syntax error");
        assert_eq!(request.path, Some(PathBuf::from(unreadable)));
    }

    /// A filesystem written out as a list, so the rules below touch no disk.
    fn table(entries: &[(&str, PathKind)]) -> impl Fn(&Path) -> PathKind + use<> {
        let entries: Vec<(PathBuf, PathKind)> = entries
            .iter()
            .map(|(path, kind)| (PathBuf::from(path), *kind))
            .collect();
        move |path: &Path| {
            entries
                .iter()
                .find(|(known, _)| known == path)
                .map_or(PathKind::Absent, |(_, kind)| *kind)
        }
    }

    const PWSH: usize = 0;

    /// PIN — a folder that is there is where the pane opens, and nothing is
    /// refused.
    #[test]
    fn a_folder_that_exists_is_the_first_panes_place() {
        let plan = resolve(
            &parsed(&["--cwd", r"D:\Developer"]),
            PWSH,
            table(&[(r"D:\Developer", PathKind::Directory)]),
        );
        assert_eq!(plan.cwd, Some(PathBuf::from(r"D:\Developer")));
        assert_eq!(plan.profile, PWSH);
        assert_eq!(plan.preview, None);
        assert!(plan.refusals.is_empty());
        assert!(plan.wants_pane);
    }

    /// PIN (the slice's own ruling) — **a folder that is not there opens the
    /// window anyway**, at the default place, and is named exactly once.
    ///
    /// The three ways to fail it are all worse and all plausible: refuse the
    /// launch (a registry verb the user cannot edit would stop opening
    /// terminals), open silently at the default (indistinguishable from a verb
    /// that was never registered), or a message box (a modal in front of no
    /// window, for a folder that was deleted).
    ///
    /// MUTATION: drop the `refusals.push` and the assertion on the count fails;
    /// return `None` for the whole plan and `wants_pane` fails, which is the
    /// half that keeps the window opening.
    #[test]
    fn a_folder_that_is_gone_costs_the_place_and_never_the_window() {
        for line in [vec!["--cwd", r"D:\gone"], vec!["--cwd", r"D:\a\file.txt"]] {
            let plan = resolve(
                &parsed(&line),
                PWSH,
                table(&[(r"D:\a\file.txt", PathKind::File)]),
            );
            assert_eq!(plan.cwd, None, "{line:?}");
            assert!(plan.wants_pane, "{line:?} still asked for a pane");
            assert_eq!(plan.refusals.len(), 1, "{line:?}");
            assert!(
                matches!(plan.refusals[0], CliRefusal::NoSuchFolder(_)),
                "{line:?} -> {:?}",
                plan.refusals[0]
            );
            assert!(!plan.refusals[0].notice().trim().is_empty());
        }
    }

    /// PIN — **the profile slug is resolved against this build's own table**,
    /// and an id it has not got costs the shell choice and nothing else.
    ///
    /// Every id is read out of the profile table rather than written here, so
    /// a profile added, removed or renamed is covered by this test on the day it
    /// moves instead of on the day somebody remembers.
    #[test]
    fn every_profile_id_this_build_has_resolves_and_an_unknown_one_falls_to_the_default() {
        for index in 0..profiles::count() {
            let id = profiles::id(index);
            let plan = resolve(&parsed(&["--profile", &id]), PWSH, table(&[]));
            assert_eq!(plan.profile, index, "{id}");
            assert!(plan.refusals.is_empty(), "{id}");
        }
        let plan = resolve(
            &parsed(&["--profile", "fish"]),
            profiles::fallback_profile(),
            table(&[]),
        );
        assert_eq!(plan.profile, profiles::fallback_profile());
        assert!(plan.wants_pane);
        assert_eq!(
            plan.refusals,
            vec![CliRefusal::NoSuchProfile("fish".to_owned())]
        );
    }

    /// PIN — a bare folder is `--cwd` and a bare file is a document.
    #[test]
    fn the_positional_is_a_place_when_it_is_a_folder_and_a_document_when_it_is_a_file() {
        let machine = table(&[
            (r"D:\Developer", PathKind::Directory),
            (r"D:\Developer\notes.md", PathKind::File),
        ]);
        let plan = resolve(&parsed(&[r"D:\Developer"]), PWSH, &machine);
        assert_eq!(plan.cwd, Some(PathBuf::from(r"D:\Developer")));
        assert_eq!(plan.preview, None);
        let plan = resolve(&parsed(&[r"D:\Developer\notes.md"]), PWSH, &machine);
        assert_eq!(plan.cwd, None, "a file names a document, not a place");
        assert_eq!(plan.preview, Some(PathBuf::from(r"D:\Developer\notes.md")));
        assert!(plan.refusals.is_empty());
        let plan = resolve(&parsed(&[r"D:\nothing"]), PWSH, &machine);
        assert_eq!(plan.preview, None);
        assert_eq!(
            plan.refusals,
            vec![CliRefusal::NoSuchPath(PathBuf::from(r"D:\nothing"))]
        );
    }

    /// PIN — `--cwd` names the place; a positional folder beside it is named and
    /// dropped, and a positional *file* beside it is not.
    #[test]
    fn a_flag_and_a_positional_folder_is_a_contradiction_and_the_flag_wins() {
        let machine = table(&[
            (r"D:\a", PathKind::Directory),
            (r"D:\b", PathKind::Directory),
            (r"D:\b\x.rs", PathKind::File),
        ]);
        let plan = resolve(&parsed(&["--cwd", r"D:\a", r"D:\b"]), PWSH, &machine);
        assert_eq!(plan.cwd, Some(PathBuf::from(r"D:\a")));
        assert_eq!(
            plan.refusals,
            vec![CliRefusal::PlaceAlreadyNamed(PathBuf::from(r"D:\b"))]
        );
        let plan = resolve(&parsed(&["--cwd", r"D:\a", r"D:\b\x.rs"]), PWSH, &machine);
        assert_eq!(plan.cwd, Some(PathBuf::from(r"D:\a")));
        assert_eq!(plan.preview, Some(PathBuf::from(r"D:\b\x.rs")));
        assert!(plan.refusals.is_empty(), "a document is not a second place");

        // And the flag still wins when the flag is the broken one — see
        // `CliRefusal::PlaceAlreadyNamed`. Both are read back; neither opens.
        let plan = resolve(&parsed(&["--cwd", r"D:\gone", r"D:\b"]), PWSH, &machine);
        assert_eq!(plan.cwd, None);
        assert_eq!(
            plan.refusals,
            vec![
                CliRefusal::NoSuchFolder(PathBuf::from(r"D:\gone")),
                CliRefusal::PlaceAlreadyNamed(PathBuf::from(r"D:\b")),
            ]
        );
    }

    /// PIN — **the folder crosses into the profile's own namespace**, and a
    /// folder that cannot cross is said out loud.
    ///
    /// This is the rule a split's folder chooser already obeys
    /// (`SplitSeed::Folder`), asked at the other door: a Windows path handed to
    /// a WSL shell unconverted names nothing at all.
    #[test]
    fn a_windows_folder_crosses_into_the_profiles_namespace_or_is_refused() {
        let wsl = profiles::index_of_id("wsl");
        let plan = resolve(
            &parsed(&["--cwd", r"D:\Developer", "--profile", "wsl"]),
            PWSH,
            table(&[(r"D:\Developer", PathKind::Directory)]),
        );
        assert_eq!(plan.profile, wsl);
        assert_eq!(plan.cwd, Some(PathBuf::from("/mnt/d/Developer")));
        assert!(plan.refusals.is_empty());
        let plan = resolve(
            &parsed(&["--cwd", r"\\server\share", "--profile", "wsl"]),
            PWSH,
            table(&[(r"\\server\share", PathKind::Directory)]),
        );
        assert_eq!(plan.cwd, None);
        assert_eq!(
            plan.refusals,
            vec![CliRefusal::UnreachableFolder {
                folder: PathBuf::from(r"\\server\share"),
                profile: wsl,
            }]
        );
        assert!(!plan.refusals[0].notice().trim().is_empty());
    }

    /// PIN — a launch nobody passed anything to asks for nothing and refuses
    /// nothing, whatever the machine looks like.
    #[test]
    fn an_empty_request_resolves_to_a_plan_that_wants_nothing() {
        let plan = resolve(
            &CliRequest::default(),
            profiles::fallback_profile(),
            table(&[]),
        );
        assert!(!plan.wants_pane);
        assert_eq!(plan.cwd, None);
        assert_eq!(plan.preview, None);
        assert_eq!(plan.profile, profiles::fallback_profile());
        assert!(plan.refusals.is_empty());
    }

    /// PIN — the usage block names every profile this build actually has.
    ///
    /// Derived from the table rather than written into the string, which is the
    /// same reason the test above walks it: a profile added with a literal list
    /// in `i18n` would ship a usage block that lies about what `--profile`
    /// takes.
    #[test]
    fn the_usage_block_lists_the_profile_ids_off_the_real_table() {
        let text = refusal_text(&CliFault::HelpAsked);
        for index in 0..profiles::count() {
            let id = profiles::id(index);
            assert!(text.contains(&id), "{} is not in the usage", id);
        }
        assert!(text.contains("--cwd"));
        assert!(text.contains("--profile"));
    }

    /// PIN — a mistake is reported *above* the usage rather than instead of it.
    #[test]
    fn a_refusal_carries_its_sentence_and_the_usage_under_it() {
        let text = refusal_text(&CliFault::UnknownFlag("--nope".to_owned()));
        assert!(text.contains("--nope"), "{text}");
        assert!(text.contains("--cwd"), "the usage is still there: {text}");
    }

    /// PIN — **`--json -` says where the payload is; every other value *is* the payload.**
    ///
    /// The two upstreams hand their payload over differently and neither can be talked out of it,
    /// so the difference lives in one word on a command line and is decided here, in a grammar,
    /// rather than by a reader guessing from a handle.
    ///
    /// MUTATION: read `-` as an inline payload and Claude Code's hook parses one byte of JSON,
    /// finds no `transcript_path` and quietly goes back to saying "Turn finished" — with nothing
    /// anywhere reporting that it failed, because a payload that does not parse is an ordinary
    /// state on this path.
    #[test]
    fn a_payload_is_either_on_the_command_line_or_on_the_handle_it_names() {
        let call = |list: &[&str]| {
            attention(args(list))
                .expect("the verb")
                .expect("a call and not a fault")
        };
        assert_eq!(
            call(&["attention", "claude-code:Stop", "--json", "-"]).payload,
            Some(AttentionPayload::Stdin)
        );
        let inline = r#"{"transcript_path":"D:\\x.jsonl"}"#;
        assert_eq!(
            call(&["attention", "codex:agent-turn-complete", "--json", inline]).payload,
            Some(AttentionPayload::Inline(inline.to_owned()))
        );
        // A call that asks for nothing gets nothing, which is every other hook this build installs.
        assert_eq!(
            call(&["attention", "claude-code:PostToolUse"]).payload,
            None
        );
        // `--json` with nothing after it is still a mistake, and `-` is still not a second event.
        assert_eq!(
            attention(args(&["attention", "claude-code:Stop", "--json"])).expect("the verb"),
            Err(AttentionFault::MissingValue(JSON_FLAG))
        );
        assert_eq!(
            call(&["attention", "claude-code:Stop", "--json", "-"]).event,
            "claude-code:Stop"
        );
    }
}
