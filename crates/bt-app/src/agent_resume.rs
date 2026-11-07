//! **An agent that was running in a pane comes back after Folio restarts, through its own
//! vendor's resume command** (0.4.9 plan row 195; 0.5's S1 in its smallest form; the
//! 2026-11-02 entry in `docs/DESIGN.md`).
//!
//! # What a pane remembers, and who tells it
//!
//! A pane remembers **program + folder + session id**, and it learns them from the one party
//! that knows: the agent's own hook, speaking from inside the pane. Two rows say it today —
//! Claude Code's `Stop` (`session_id`, `cwd`) and codex's `notify` (`thread-id`, `cwd`) — both
//! at the end of every turn, and both already read their payload, so nothing a reader installed
//! has to be installed again. The fields cross the attention wire as the third declared field
//! ([`attention_map::Resume`], checked here by [`Heard::is_well_formed`]), and the window keeps
//! them on the pane ([`AgentAccount`]) and writes them with the layout
//! ([`bt_persist::TermLeafV1::agent`]).
//!
//! # Which runs are recorded, and when a record ends
//!
//! * **A pane whose own program is the agent** — an agent profile's tab, or a pane this module
//!   resumed — records every hook of that vendor. Its record ends when the agent ends by itself.
//! * **A shell pane where somebody typed the agent** records it only while a command the shell
//!   marked (`OSC 133`) is running, and the record belongs to that command and to the first vendor
//!   heard inside it: the shell's next prompt ends it ([`AgentAccount::settle_command`]), and a mark
//!   retired while the command runs does not. A shell that marks nothing has no such end to read
//!   — codex says nothing when it stops — so nothing is recorded there and the pane comes back as
//!   its shell, as before.
//! * **`SessionEnd` ends nothing.** It can fire while the system is ending the agent, which would
//!   clear the record and autosave the loss before the shutdown's own question arrives.
//!
//! # What a restore does with it
//!
//! A restored pane whose record names a vendor this build can resume is born **as that vendor's
//! program with that vendor's resume arguments, in the recorded folder** ([`revived`],
//! [`Vendor::arguments`]). Folio starts the agent and never types anything into a shell. The
//! profile's environment comes along; its arguments do not (a resume never resurrects a grant,
//! and a positional argument would break `codex resume <id>`). When the resumed agent ends by
//! itself, a pane that was a shell gets its shell back in the same folder ([`Ended::HandBack`]).
//!
//! **Which program starts it is the platform's road** ([`Road`], T-AGENT-RESUME-ON-MACOS): on
//! Windows the vendor's own shipped profile, whose program a fresh logon environment finds; on a
//! Mac the pane's own shell, told to `exec` the vendor's resume, because a child of a Folio started
//! from Finder inherits launchd's `PATH` and only the person's shell builds theirs. Either way the
//! agent is the pane's process and nothing is typed.
//!
//! **A resume whose folder is gone is not started** (review F3): no vendor's resume is run in a
//! folder nobody chose, so the birth starts nothing and the pane is handed its shell with the line
//! below ([`AgentAccount::not_started`]).
//!
//! **A resume has failed when the resumed agent exits before any hook has spoken from it** — any
//! hook, a submitted prompt's as much as a finished turn's ([`AgentAccount::spoke`]) — no
//! clock: the vendors' refusals print text and exit (Claude Code: "No conversation found with
//! session ID: …", exit 1, measured 2026-11-02 in a scratch configuration folder), and a pane
//! that printed something is not a birth death. The pane's shell is then born in the folder with
//! one line saying which session did not come back, and the record is cleared.
//!
//! # How this maps onto 0.5's session identity (C1), which this does not build
//!
//! The record is C1's *vendor conversation / resume id* and nothing else; the leaf's place in the
//! saved tree stands in for C1's session id until C1 mints one; a resume is a new
//! [`crate::ShellSession`] incarnation; one record is one `agent_epoch`'s resume face, written at
//! its first turn's end and cleared at its end. No grant is recorded, so none is resurrected.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use bt_persist::AgentRecordV1;
use bt_platform::HostPlatform;
use bt_term::{CommandMark, CommandMarkId};

use crate::attention_map;
use crate::profiles;

/// **How one vendor's CLI is told to resume one session** — read off its own `--help`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResumeForm {
    /// `<program> <flag> <id>`: Claude Code's `-r, --resume [value]` ("Resume a conversation by
    /// session ID or name").
    Flag(&'static str),
    /// `<program> <subcommand> <id>`: codex's `resume [SESSION_ID]` ("Session id (UUID) or
    /// session name. UUIDs take precedence").
    Subcommand(&'static str),
}

/// **One vendor whose sessions this build resumes** — data, one row each.
#[derive(Debug)]
pub(crate) struct Vendor {
    /// The attention family its hook speaks as ([`attention_map::CLAUDE_CODE`]).
    pub(crate) family: &'static str,
    /// The shipped agent profile that starts it on Windows (`profiles::windows_shipped`; a Mac
    /// ships none, and resumes through the pane's shell instead: [`Road`]).
    pub(crate) profile: &'static str,
    /// The file stem its program has, whatever the install route (`claude.cmd`, `claude.exe`): how
    /// a profile somebody made for it is told apart from a shell, and the name a shell is told to
    /// `exec` on the Mac's road ([`Vendor::shell_command`]).
    pub(crate) program_stem: &'static str,
    /// Its product name, as the failure line says it. Not localised, like every product name in
    /// the profile table.
    pub(crate) title: &'static str,
    /// The variable it reads its configuration folder from — the folder its sessions are filed in.
    pub(crate) home_variable: &'static str,
    pub(crate) form: ResumeForm,
}

/// **The vendors this build resumes**, in the order their adapters arrived. Copilot CLI, Kimi Code
/// and Gemini CLI also have resume commands; each lands vendor by vendor, from that vendor's
/// verified facts, as a row here and a [`attention_map::Resume`] on its turn-end row.
pub(crate) const VENDORS: &[Vendor] = &[
    Vendor {
        family: attention_map::CLAUDE_CODE,
        profile: "claude",
        program_stem: "claude",
        title: "Claude Code",
        home_variable: "CLAUDE_CONFIG_DIR",
        form: ResumeForm::Flag("--resume"),
    },
    Vendor {
        family: attention_map::CODEX,
        profile: "codex",
        program_stem: "codex",
        title: "Codex",
        home_variable: "CODEX_HOME",
        form: ResumeForm::Subcommand("resume"),
    },
];

/// The vendor a family name (a record's `vendor`, a hook's family) names, if this build resumes it.
#[must_use]
pub(crate) fn vendor(family: &str) -> Option<&'static Vendor> {
    VENDORS.iter().find(|vendor| vendor.family == family)
}

impl Vendor {
    /// **The resume command's arguments**, and nothing else: no profile argument, no flag of
    /// Folio's choosing.
    #[must_use]
    pub(crate) fn arguments(&self, id: &str) -> Vec<OsString> {
        let word = match self.form {
            ResumeForm::Flag(flag) => flag,
            ResumeForm::Subcommand(subcommand) => subcommand,
        };
        vec![OsString::from(word), OsString::from(id)]
    }

    /// **The one command a shell is handed to become this vendor's resume** (the Mac's road,
    /// [`Road::ThroughTheShell`]): `exec`, the program's name, the resume's arguments. No word
    /// needs quoting in any shell: the name and the flag are this table's constants, and the id is
    /// one [`session_is_well_formed`] passed, hex digits and hyphens. `exec` makes the agent the
    /// pane's own process (the child the reaper waits on) and skips the shell's aliases and
    /// functions, so an alias that adds a flag resurrects no grant.
    #[must_use]
    pub(crate) fn shell_command(&self, id: &str) -> OsString {
        let mut command = OsString::from("exec");
        for word in std::iter::once(OsString::from(self.program_stem)).chain(self.arguments(id)) {
            command.push(" ");
            command.push(word);
        }
        command
    }

    /// Whether `program` is this vendor's CLI, by its file stem (either case).
    #[must_use]
    pub(crate) fn runs(&self, program: &Path) -> bool {
        program
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.eq_ignore_ascii_case(self.program_stem))
    }

    /// Whether the profile `id` starts this vendor's CLI: its shipped row, or a row somebody made
    /// that names the program by path.
    #[must_use]
    pub(crate) fn is_started_by(&self, id: &str) -> bool {
        id == self.profile
            || profiles::row_of(id).is_some_and(|row| match &row.program {
                profiles::ProgramSource::Path(path) => self.runs(path),
                _ => false,
            })
    }
}

/// The most characters a folder may cross the wire as. A path a vendor reports is the folder it
/// was started in, and one longer than this is not a folder anybody works in.
pub(crate) const MAX_FOLDER_CHARS: usize = 1024;

/// **What a hook said about the session it is running in** — the attention wire's third declared
/// field (`attention_wire::Message::resume`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Heard {
    /// The vendor's session id.
    pub(crate) id: String,
    /// The folder the vendor files the session under.
    pub(crate) cwd: String,
    /// The vendor's configuration folder, when the hook's own environment set one.
    pub(crate) home: Option<String>,
}

impl Heard {
    /// **Whether every part is one this build could have sent**: a UUID-shaped id
    /// ([`bt_persist::resume_id_is_well_formed`]), and folders that are absolute, bounded and free
    /// of anything that moves text about rather than being text. Checked by the hook before it
    /// sends and by the window when it reads, because neither end has cause to trust the other.
    #[must_use]
    pub(crate) fn is_well_formed(&self) -> bool {
        session_is_well_formed(&self.id, &self.cwd, self.home.as_deref())
    }

    /// The record this becomes on a pane, for `vendor`.
    #[must_use]
    pub(crate) fn record(&self, vendor: &Vendor) -> AgentRecordV1 {
        AgentRecordV1 {
            vendor: vendor.family.to_owned(),
            resume_id: self.id.clone(),
            cwd: self.cwd.clone(),
            home: self.home.clone(),
        }
    }
}

/// **The one rule a session's three facts are held to**, wherever they come from (review N5): the
/// wire's [`Heard`] and the saved record [`revived`] reads alike — a UUID-shaped id
/// ([`bt_persist::resume_id_is_well_formed`]) and folders [`folder_is_well_formed`] passes.
#[must_use]
pub(crate) fn session_is_well_formed(id: &str, cwd: &str, home: Option<&str>) -> bool {
    bt_persist::resume_id_is_well_formed(id)
        && folder_is_well_formed(cwd)
        && home.is_none_or(folder_is_well_formed)
}

/// An absolute folder of a bounded length with no control, format or separator character in it.
#[must_use]
pub(crate) fn folder_is_well_formed(folder: &str) -> bool {
    !folder.is_empty()
        && folder.chars().count() <= MAX_FOLDER_CHARS
        && !folder.chars().any(|character| {
            character.is_control()
                || crate::is_format_or_bidi_control(character)
                || matches!(character, '\u{2028}' | '\u{2029}')
        })
        && folder_is_absolute(folder)
}

/// Absolute on either platform's spelling — a drive or UNC path, or a POSIX root — because the
/// window that reads the folder may be checking a path a hook on the same machine wrote, and the
/// check must not depend on which build is doing it.
fn folder_is_absolute(folder: &str) -> bool {
    let bytes = folder.as_bytes();
    folder.starts_with('/')
        || folder.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
}

/// **What the pane's own program is, for the question "whose hook is this"** — asked by
/// [`AgentAccount::hear`].
#[derive(Clone, Copy, Debug)]
pub(crate) enum Child<'a> {
    /// The pane's program is this vendor's CLI.
    Agent(&'a Vendor),
    /// The pane's program is a shell (or anything else), and these are the commands it marked.
    Shell(&'a [CommandMark]),
}

/// **The child a pane's program makes it** for [`AgentAccount::hear`]: the vendor whose CLI it
/// runs, or a shell with the commands it marked. A pane whose child is a resume is asked through
/// [`AgentAccount::child`], which knows its vendor whatever its program is called.
#[must_use]
pub(crate) fn child_of<'a>(program: Option<&Path>, marks: &'a [CommandMark]) -> Child<'a> {
    program
        .and_then(|program| VENDORS.iter().find(|vendor| vendor.runs(program)))
        .map_or(Child::Shell(marks), Child::Agent)
}

/// A resume in this pane's child, and what its end hands back.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Resumed {
    /// Whether a hook has spoken from this child — the line between a resume that failed and one
    /// that ran and ended. Any hook from the pane counts, `UserPromptSubmit` as much as `Stop`
    /// ([`AgentAccount::spoke`]): something in the pane answered, so the resume ran.
    heard: bool,
    /// The birth found the recorded folder gone and started nothing in its place: a resume is
    /// never run in a folder the session was not filed under ([`AgentAccount::not_started`]).
    not_started: bool,
    /// The shell profile the pane was before its agent was resumed in it, or `None` for a pane
    /// whose own profile is the agent's.
    hand_back: Option<String>,
    /// The vendor's title, for the failure line.
    title: &'static str,
}

/// **A pane's agent: the record it keeps, and the resume its child is** — on
/// [`crate::ShellSession`], with the leaf's lifetime.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct AgentAccount {
    record: Option<AgentRecordV1>,
    /// The shell command the record belongs to, for an agent somebody typed into a shell.
    command: Option<CommandMarkId>,
    resumed: Option<Resumed>,
    /// A shell is being born in this pane's place after its resumed agent ended
    /// ([`crate::runtime`]'s hand-back): the pane holds its tab until it lands.
    handing_back: bool,
}

/// **What the end of a pane's own child asks of the pane.**
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ended {
    /// Nothing changed on the record: the pane goes the way an exited pane goes.
    Unchanged,
    /// The record was cleared; the pane goes the way an exited pane goes.
    Cleared,
    /// A shell is owed in the pane's place, in `cwd`: the profile it was before its agent was
    /// resumed (`None`: the default profile), and the failure line when the resume never ran.
    HandBack {
        profile: Option<String>,
        cwd: PathBuf,
        failed: Option<CouldNotResume>,
    },
}

/// The one line a pane says when its agent's session did not come back.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CouldNotResume {
    pub(crate) title: &'static str,
    pub(crate) id: String,
}

impl CouldNotResume {
    /// The line's text in the window's language (`zh pending`).
    #[must_use]
    pub(crate) fn text(&self) -> String {
        crate::i18n::could_not_resume_text(self.title, short_id(&self.id))
    }
}

/// The first eight characters of an id — enough to tell two sessions apart in one line.
#[must_use]
pub(crate) fn short_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

impl AgentAccount {
    /// **The account a pane is born with**: a resume's, when `seed` asks for one and its program
    /// is the one being started; nothing otherwise.
    #[must_use]
    pub(crate) fn born(seed: &SeedAgent, resuming: bool) -> Self {
        match seed {
            SeedAgent::Resume(resume) if resuming => Self {
                record: Some(resume.record.clone()),
                command: None,
                resumed: Some(Resumed {
                    heard: false,
                    not_started: false,
                    hand_back: resume.hand_back.clone(),
                    title: vendor(&resume.record.vendor).map_or("", |vendor| vendor.title),
                }),
                handing_back: false,
            },
            _ => Self::default(),
        }
    }

    /// The record the layout writes, when there is one.
    #[must_use]
    pub(crate) fn record(&self) -> Option<&AgentRecordV1> {
        self.record.as_ref()
    }

    /// **The profile the layout writes for this pane**: the shell it was before its agent was
    /// resumed in it, or the profile it was launched as.
    #[must_use]
    pub(crate) fn saved_profile<'a>(&'a self, launched: &'a str) -> &'a str {
        self.resumed
            .as_ref()
            .and_then(|resumed| resumed.hand_back.as_deref())
            .unwrap_or(launched)
    }

    /// Whether this pane's child is a resumed agent.
    #[must_use]
    pub(crate) fn is_resumed(&self) -> bool {
        self.resumed.is_some()
    }

    /// **The child this pane is, for the question "whose hook is this"** (T-AGENT-RESUME-ON-MACOS):
    /// a resume is its vendor's agent by its record, whatever its program is called. On a Mac the
    /// program is the shell that `exec`ed it; on Windows it is the vendor's own, so the answer
    /// there is the one [`child_of`] gives. Any other pane is what [`child_of`] says.
    #[must_use]
    pub(crate) fn child<'a>(&self, program: Option<&Path>, marks: &'a [CommandMark]) -> Child<'a> {
        self.resumed
            .as_ref()
            .and(self.record.as_ref())
            .and_then(|record| vendor(&record.vendor))
            .map_or_else(|| child_of(program, marks), Child::Agent)
    }

    /// **The birth found the recorded folder gone and started nothing** (row 195, review F3): the
    /// pane's shell is owed in its place with the line, at the reaper's next turn
    /// ([`Self::take_not_started`]).
    pub(crate) fn not_started(&mut self) {
        if let Some(resumed) = self.resumed.as_mut() {
            resumed.not_started = true;
        }
    }

    /// **The hand-back a resume that was not started owes**, once: what [`Self::child_ended`]
    /// answers for a resume that never ran — the pane's shell, the line, the record cleared.
    pub(crate) fn take_not_started(&mut self) -> Option<Ended> {
        self.resumed
            .as_ref()
            .is_some_and(|resumed| resumed.not_started)
            .then(|| self.child_ended())
    }

    /// **A hook spoke from this pane**, whatever it said (review N3): a resumed agent that was
    /// heard from at all ran, so its end is no failed resume.
    pub(crate) fn spoke(&mut self) {
        if let Some(resumed) = self.resumed.as_mut() {
            resumed.heard = true;
        }
    }

    /// Whether a shell is being born in this pane's place.
    #[must_use]
    pub(crate) fn handing_back(&self) -> bool {
        self.handing_back
    }

    /// The hand-back's shell has been asked for.
    pub(crate) fn begin_hand_back(&mut self) {
        self.handing_back = true;
    }

    /// The hand-back's shell was refused: the pane ends as an exited pane ends.
    pub(crate) fn end_hand_back(&mut self) {
        self.handing_back = false;
    }

    /// **A hook of `vendor`'s spoke from this pane, naming its session.** Answers whether the
    /// record changed (and the layout is owed a write).
    ///
    /// The pane's own agent is recorded on every turn's end; a shell's only while one of its marked
    /// commands is running, for that command. Any other hook — another vendor's from inside the
    /// pane's agent, a shell that marks nothing — records nothing.
    ///
    /// **One run, one vendor, one folder** (review F1, F7). A shell's command is bound to the first
    /// vendor heard from inside it: a `codex exec` that a typed `claude` starts speaks from the same
    /// pane, and is refused until the command ends. Within one run only the id may change — a
    /// `/clear` or `/new` mints a new one, which is the same agent's next session — and the folder
    /// and configuration folder stay those of the run's first turn, the folder the vendor filed the
    /// session under, wherever the agent has `cd`'d since. A nested headless run of the same
    /// vendor (`claude -p` inside Claude Code) is not told apart from a `/clear`: no field names
    /// it, and it is a named limit in `docs/DESIGN.md`.
    pub(crate) fn hear(&mut self, vendor: &Vendor, heard: &Heard, child: Child<'_>) -> bool {
        if !heard.is_well_formed() {
            return false;
        }
        let command = match child {
            Child::Agent(own) if own.family == vendor.family => None,
            Child::Agent(_) => return false,
            Child::Shell(marks) => match self.command_heard_in(marks) {
                Some(command) => Some(command),
                None => return false,
            },
        };
        let same_run = self.command == command;
        let kept = self.record.as_ref().filter(|_| same_run);
        if kept.is_some_and(|kept| kept.vendor != vendor.family) {
            return false;
        }
        let record = match kept {
            Some(kept) => AgentRecordV1 {
                resume_id: heard.id.clone(),
                ..kept.clone()
            },
            None => heard.record(vendor),
        };
        self.spoke();
        let changed = self.record.as_ref() != Some(&record) || self.command != command;
        self.record = Some(record);
        self.command = command;
        changed
    }

    /// The marked command a hook in a shell pane is heard for: the one the shell is running now,
    /// or the one already recorded while it goes on ([`command_goes_on`]) — a mark that was
    /// retired under a running command (Clear scrollback, an `ED 3`, eviction, a reflow) does not
    /// end it.
    fn command_heard_in(&self, marks: &[CommandMark]) -> Option<CommandMarkId> {
        running_command(marks).or_else(|| {
            self.command
                .filter(|command| command_goes_on(*command, marks))
        })
    }

    /// **A typed agent's record ends with its command**: cleared once the shell has marked that
    /// command finished, or marked a newer one (review F2). A mark retired with no newer one after
    /// it keeps the record: the command it stood for may well be running. Answers whether it was
    /// cleared.
    pub(crate) fn settle_command(&mut self, marks: &[CommandMark]) -> bool {
        let Some(command) = self.command else {
            return false;
        };
        if command_goes_on(command, marks) {
            return false;
        }
        self.record = None;
        self.command = None;
        true
    }

    /// **The pane's own child ended by itself** — not by the system's end of the session, which
    /// keeps everything. What that asks of the pane, and the record is cleared in every case.
    pub(crate) fn child_ended(&mut self) -> Ended {
        let record = self.record.take();
        self.command = None;
        match self.resumed.take() {
            Some(Resumed {
                heard: false,
                hand_back,
                title,
                ..
            }) => match record {
                Some(record) => Ended::HandBack {
                    profile: hand_back,
                    cwd: PathBuf::from(&record.cwd),
                    failed: Some(CouldNotResume {
                        title,
                        id: record.resume_id,
                    }),
                },
                None => Ended::Cleared,
            },
            Some(Resumed {
                heard: true,
                hand_back: Some(profile),
                ..
            }) => Ended::HandBack {
                profile: Some(profile),
                cwd: record.map_or_else(PathBuf::new, |record| PathBuf::from(record.cwd)),
                failed: None,
            },
            _ if record.is_some() => Ended::Cleared,
            _ => Ended::Unchanged,
        }
    }
}

/// The marked command the shell is running now: its last mark, executed and not finished.
pub(crate) fn running_command(marks: &[CommandMark]) -> Option<CommandMarkId> {
    marks
        .last()
        .filter(|mark| mark.executed.is_some() && mark.finished.is_none())
        .map(|mark| mark.id)
}

/// **Whether the marked command `command` may still be running**: the shell has marked no newer
/// command (mark ids are monotonic and never reused), and its own mark, if the session still holds
/// it, is not finished. A mark the session no longer holds — retired by Clear scrollback, an
/// `ED 3`, the scrollback quota or a reflow that could not reseat it — says nothing about the
/// command it stood for.
pub(crate) fn command_goes_on(command: CommandMarkId, marks: &[CommandMark]) -> bool {
    !marks.iter().any(|mark| mark.id > command)
        && marks
            .iter()
            .find(|mark| mark.id == command)
            .is_none_or(|mark| mark.finished.is_none())
}

/// **A resume a restored pane's seed carries**: the record, and the shell profile to hand back
/// when the pane was a shell.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Resume {
    pub(crate) record: AgentRecordV1,
    pub(crate) hand_back: Option<String>,
}

/// **What a seed says about an agent** (`crate::LeafSeed::agent`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum SeedAgent {
    #[default]
    None,
    /// Born as the vendor's resume ([`revived`]).
    Resume(Resume),
    /// A shell handed back after a resume that did not come back, saying so in its first line.
    CouldNotResume(CouldNotResume),
}

/// **Whether this run resumes agents at all, and by which road**: the Agents page's switch, on a
/// platform this build resumes on (Windows since 0.4.9, macOS since T-AGENT-RESUME-ON-MACOS), and
/// that platform's [`Road`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Restore {
    pub(crate) switch: bool,
    pub(crate) platform: HostPlatform,
}

impl Restore {
    /// No resume: what every revive that is not a launch's or the card's asks for — today only the
    /// tests' `revive_plan`, every product revive being one of those two.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn off() -> Self {
        Self {
            switch: false,
            platform: bt_platform::host_platform(),
        }
    }

    /// This process's answer: `settings.json`'s `resume_agents` on this platform.
    #[must_use]
    pub(crate) fn of_this_run(settings: &bt_persist::SettingsV1) -> Self {
        Self {
            switch: settings.resume_agents,
            platform: bt_platform::host_platform(),
        }
    }

    fn resumes(self) -> bool {
        self.switch && crate::first_run::Capability::AgentResume.on(self.platform)
    }
}

/// **Which program a resume is started as** (T-AGENT-RESUME-ON-MACOS, 0.4.10 plan row 15): a
/// value of the platform, so a test on either machine asks what the other does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Road {
    /// The vendor's own program, through its shipped profile ([`Vendor::profile`]), with the
    /// resume's arguments in place of the profile's. A child on Windows is given the account's
    /// fresh logon environment, so the `PATH` it is found on and runs with is the person's.
    VendorProgram,
    /// The pane's own shell, as its profile starts it, told to run [`Vendor::shell_command`] as
    /// an interactive shell ([`crate::profiles::CommandFlags`]). Off Windows a child inherits
    /// Folio's own environment (launchd's bare system `PATH` for an app opened from Finder), and
    /// the `PATH` an agent was typed under is the one the person's login and rc files build, so
    /// the shell that reads them is what starts it.
    ThroughTheShell,
}

impl Road {
    /// The road `platform` resumes by.
    #[must_use]
    pub(crate) const fn on(platform: HostPlatform) -> Self {
        match platform {
            HostPlatform::Windows => Self::VendorProgram,
            HostPlatform::MacOs | HostPlatform::OtherUnix => Self::ThroughTheShell,
        }
    }
}

/// **A saved pane's profile, folder and agent, as its seed is to carry them**: `(profile, folder,
/// agent)`. A well-formed record of a vendor this build resumes, while `restore` resumes, makes
/// the pane its vendor's resume in the recorded folder, handing back `profile` when that was a
/// shell; anything else leaves the pane as it was saved.
///
/// **The vendor is the record's, and the program is the road's** ([`Road::on`]): a pane whose own
/// profile starts the vendor keeps it on either road; a shell pane is born as the vendor's profile
/// on Windows, and as that same shell on a Mac, which `exec`s the vendor's resume.
#[must_use]
pub(crate) fn revived(
    profile: String,
    folder: Option<PathBuf>,
    record: Option<&AgentRecordV1>,
    restore: Restore,
) -> (String, Option<PathBuf>, SeedAgent) {
    let Some((record, vendor)): Option<(&AgentRecordV1, &'static Vendor)> = record
        .filter(|_| restore.resumes())
        .filter(|record| {
            record.is_well_formed()
                && session_is_well_formed(&record.resume_id, &record.cwd, record.home.as_deref())
        })
        .and_then(|record| vendor(&record.vendor).map(|vendor| (record, vendor)))
    else {
        return (profile, folder, SeedAgent::None);
    };
    let (started, hand_back) = if vendor.is_started_by(&profile) {
        (profile, None)
    } else {
        match Road::on(restore.platform) {
            Road::VendorProgram => (vendor.profile.to_owned(), Some(profile)),
            Road::ThroughTheShell => (profile.clone(), Some(profile)),
        }
    };
    (
        started,
        Some(PathBuf::from(&record.cwd)),
        SeedAgent::Resume(Resume {
            record: record.clone(),
            hand_back,
        }),
    )
}

/// The profile a hand-back's shell is started as: the pane's own, or the default when the pane's
/// own profile was the agent's.
#[must_use]
pub(crate) fn hand_back_profile(profile: Option<String>) -> String {
    profile.unwrap_or_else(|| profiles::DEFAULT_IDENTITY.to_owned())
}
