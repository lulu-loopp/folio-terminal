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
//! ARGV argv=["…\shell-probe.exe", "argv", "--cwd", "D:\\Developer\\BetterTerminal\\crates"]
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
            Self::HelpAsked => 0,
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
            Self::HelpAsked => None,
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

/// Turn a command line into a request, or into the fault that ends the launch.
///
/// The argument is everything **after** the program's own name;
/// `std::env::args_os().skip(1)` at the one real call site, and a literal list
/// in every test.
///
/// # The grammar
///
/// ```text
/// folio [--cwd <folder>] [--profile <id>] [--] [<path>]
/// ```
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
/// `from_wide` hands back exactly what Windows said.
///
/// Called only where [`is_flag`] has already said yes, which is what makes
/// "longer than the name" the test for the joined form.
fn value_for(
    name: &'static str,
    text: &str,
    arg: &OsString,
    args: &mut impl Iterator<Item = OsString>,
) -> Result<OsString, CliFault> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    if text.len() > name.len() {
        let wide: Vec<u16> = arg.encode_wide().collect();
        let value = OsString::from_wide(&wide[name.len() + 1..]);
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
    /// `crate::attention_wire`'s header — and today nothing is ever taken out of it, because no
    /// row of the mapping table declares an identifier to take.
    pub payload: Option<String>,
}

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
                payload = Some(value.to_string_lossy().into_owned());
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
            }
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
}
