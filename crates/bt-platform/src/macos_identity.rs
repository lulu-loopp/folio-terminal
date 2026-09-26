//! **Is this bundle the same publisher's Folio, and does Gatekeeper accept
//! it** — the macOS identity check of the self-updater (0.4.6 ticket U-16;
//! `docs/plans/design/self-update-2026-09-16.md` §E "macOS", C7, revision (b)
//! F-9 and experiment E-8).
//!
//! Two questions, asked of a copied bundle before anything is swapped, and
//! one decision over their answers:
//!
//! * **Identity** ([`verify_bundle`]): the bundle's signature is valid —
//!   `codesign --verify --strict --deep --all-architectures`, which is the
//!   note's "strict validation, all architectures and nested code" — **and**
//!   it satisfies a [`Requirement`]: the designated requirement of the running
//!   code ([`running_requirement`]), conjoined with Apple's Developer ID
//!   requirement ([`DEVELOPER_ID`]). There is no Team ID constant in this
//!   repository (C7, `docs/RELEASING.md`); the running build's own designated
//!   requirement names its certificate lineage and its bundle identifier, and
//!   the new bundle must satisfy that. The Developer ID clause is Apple's
//!   published requirement for a Developer ID application, not Folio's: it is
//!   what makes an ad-hoc running build (whose designated requirement is its
//!   own `cdhash`) admit nothing, and an ad-hoc bundle fail by name.
//! * **Gatekeeper** ([`assess`]): `spctl --status`, and when assessments are
//!   enabled, `spctl --assess --type execute -vv` on the bundle, each read
//!   from a fixed grammar ([`parse_status`], [`parse_assessment`]).
//! * **The decision** ([`identity_decision`], F-9): identity is required in
//!   every case, because `spctl` answers "some notarized Developer ID", which
//!   is not "Folio" (§E). Over a verified bundle: accepted with source
//!   *Notarized Developer ID* passes as [`Identity::Notarized`]; an explicit
//!   rejection is refused; assessments disabled passes as
//!   [`Identity::GatekeeperOff`] — the person turned Gatekeeper off, and that
//!   is not this product's to override (CONVENTIONS §十) — and anything else is
//!   refused.
//!
//! **Why the tools and not `Security.framework` in process.** The two reads
//! go through the child-process door ([`crate::quiet_command`]) as
//! `/usr/bin/codesign` and `/usr/sbin/spctl`, both part of macOS itself.
//! `SecStaticCodeCheckValidity` is reachable by a raw `extern` block, but an
//! in-process evaluation reads the whole bundle from this process with no door
//! of its own, and the ticket's architecture section names the child door and
//! no other. `codesign`'s exit status separates the two identity failures (1:
//! the code is not valid; 3: valid, but the requirement is not satisfied), and
//! `codesign -d -r- <pid>` reads the designated requirement of the running
//! process itself, not of a file that could have been replaced under it.
//! The absolute paths are named here rather than looked up on `PATH`
//! (`quiet_command_named` is the door's lookup, and it exists for programs
//! whose place is not fixed): an earlier `codesign` on `PATH` must never be the
//! one that answers.
//!
//! **Bounded, worker only.** Every call starts a child and waits for it under a
//! deadline ([`VERIFY_BOUND`], [`ASSESS_BOUND`], [`DISPLAY_BOUND`]); a child
//! past its deadline is ended by its own handle and the call is refused. Each
//! stream of a child's output is read up to [`OUTPUT_BOUND`] bytes, and a
//! refusal quotes at most the first line of it, cut to [`QUOTE_BOUND`]
//! characters. None of it may run on a window thread: a deep verification
//! hashes every page of the bundle, and `spctl` may ask the notarization
//! service. Today that is this sentence; the thread door's `WorkerCtx` (A1b)
//! and its prohibitions (A1e) are what will make it a type.
//!
//! **Off macOS** every call is refused with [`Why::NotMacOs`], naming this
//! door; the parsers and the decision are pure and compile everywhere.

use std::path::Path;
use std::time::Duration;

/// Apple's requirement for a **Developer ID application**: anchored at Apple,
/// the intermediate is Developer ID's (`1.2.840.113635.100.6.2.6`) and the
/// leaf is a Developer ID Application certificate (`1.2.840.113635.100.6.1.13`).
/// It is the Developer ID half of every notarized application's own designated
/// requirement (measured 2026-09-26 on the Mac mini: every notarized
/// third-party application on it carries these three clauses).
pub const DEVELOPER_ID: &str = "anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists";

/// The `source=` Gatekeeper gives a notarized Developer ID application, the one
/// acceptance F-9 passes.
pub const NOTARIZED_DEVELOPER_ID: &str = "Notarized Developer ID";

/// `codesign`, part of macOS.
pub const CODESIGN: &str = "/usr/bin/codesign";

/// `spctl`, part of macOS (not of Xcode).
pub const SPCTL: &str = "/usr/sbin/spctl";

/// **How long a verification may take.** A strict deep verification hashes
/// every page of every file of the bundle; a release bundle is about a hundred
/// megabytes, which takes about a second on the Mac mini. Two minutes is that
/// on a slow disk, with room, and a verification past it is a refusal.
pub const VERIFY_BOUND: Duration = Duration::from_secs(120);

/// **How long a Gatekeeper assessment may take.** `spctl` may ask Apple's
/// notarization service about a ticket it has not cached; how long it waits
/// offline before it answers from a stapled ticket is experiment E-8.
pub const ASSESS_BOUND: Duration = Duration::from_secs(60);

/// **How long `codesign -d` and `spctl --status` may take** — neither hashes
/// anything or reaches the network.
pub const DISPLAY_BOUND: Duration = Duration::from_secs(10);

/// The most bytes read from each of a child's two output streams. Every answer
/// this module reads is a few lines; more is a malformed answer.
pub const OUTPUT_BOUND: usize = 16 * 1024;

/// The most characters of a tool's first line a [`Refusal`] carries.
pub const QUOTE_BOUND: usize = 160;

/// **The requirement a new bundle must satisfy**: the running code's
/// designated requirement and [`DEVELOPER_ID`], both. Only
/// [`running_requirement`] makes one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Requirement {
    designated: String,
}

impl Requirement {
    fn of_designated(designated: String) -> Self {
        Self { designated }
    }

    /// The running code's designated requirement, as `codesign` printed it.
    #[must_use]
    pub fn designated(&self) -> &str {
        &self.designated
    }

    /// The requirement text a bundle is tested against: the designated
    /// requirement and the Developer ID requirement, conjoined.
    #[must_use]
    pub fn text(&self) -> String {
        format!("({}) and ({DEVELOPER_ID})", self.designated)
    }
}

/// **A bundle whose signature is valid and satisfies a [`Requirement`].** Made
/// only by [`verify_bundle`], so [`identity_decision`] cannot be handed one
/// that no verification produced.
#[derive(Debug, PartialEq, Eq)]
pub struct Verified(());

/// **What Gatekeeper said about a bundle.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Assessment {
    /// Accepted, and the rule that accepted it (`source=`).
    Accepted { source: String },
    /// Rejected, with the `source=` or the parenthesised reason when `spctl`
    /// gave one.
    Rejected { reason: Option<String> },
    /// `spctl --status` answered `assessments disabled`.
    Disabled,
}

/// **The identity a bundle passed with** — the value the updater records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Identity {
    /// Verified, and Gatekeeper accepted it as a notarized Developer ID
    /// application.
    Notarized,
    /// Verified, and Gatekeeper's assessments are turned off on this machine:
    /// the pass rests on code validity and the requirement alone (F-9).
    GatekeeperOff,
}

/// **The step a refusal happened at.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Reading the running code's designated requirement.
    RunningRequirement,
    /// The bundle's signature: `codesign --verify --strict --deep`.
    Validity,
    /// The bundle against the requirement: `codesign --verify -R`.
    Requirement,
    /// `spctl --status`.
    GatekeeperStatus,
    /// `spctl --assess`, and the decision over its answer.
    GatekeeperAssessment,
}

impl Stage {
    /// The stage's name, as a refusal prints it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::RunningRequirement => "running-requirement",
            Self::Validity => "validity",
            Self::Requirement => "requirement",
            Self::GatekeeperStatus => "gatekeeper-status",
            Self::GatekeeperAssessment => "gatekeeper-assessment",
        }
    }
}

/// **Why a stage refused.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Why {
    /// This build has no macOS arm.
    NotMacOs,
    /// The tool could not be started.
    DidNotStart,
    /// The tool ran past its bound and was ended.
    TimedOut,
    /// The tool wrote more than [`OUTPUT_BOUND`] bytes to a stream.
    OutputPastBound,
    /// The tool's output is not UTF-8.
    NotText,
    /// The tool exited with this status (`None`: ended by a signal).
    Failed(Option<i32>),
    /// The code is valid and does not satisfy the requirement (`codesign`
    /// exit 3).
    NotSatisfied,
    /// The output is not a sentence of the stage's grammar.
    Malformed,
    /// Gatekeeper rejected the bundle.
    Rejected,
    /// Gatekeeper accepted the bundle under a rule other than
    /// [`NOTARIZED_DEVELOPER_ID`], named here.
    NotNotarizedDeveloperId(String),
}

/// **A bundle that is not this publisher's Folio, or not known to be** — the
/// stage, the reason, and at most the first line of what the tool said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub stage: Stage,
    pub why: Why,
    pub first_line: Option<String>,
}

impl Refusal {
    fn at(stage: Stage, why: Why) -> Self {
        Self {
            stage,
            why,
            first_line: None,
        }
    }

    fn quoting(stage: Stage, why: Why, output: &str) -> Self {
        Self {
            stage,
            why,
            first_line: first_line(output),
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "macos_identity {}: {:?}", self.stage.name(), self.why)?;
        if let Some(line) = &self.first_line {
            write!(f, ": {line}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Refusal {}

/// The first non-empty line of a tool's output, cut to [`QUOTE_BOUND`]
/// characters — the most of it a [`Refusal`] ever carries.
fn first_line(output: &str) -> Option<String> {
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(QUOTE_BOUND).collect())
}

/// **The designated requirement of the running process**, conjoined with
/// [`DEVELOPER_ID`] (see [`Requirement`]).
///
/// Read with `codesign -d -r- <pid>`, which asks for the running code rather
/// than for the file it was started from. An ad-hoc or linker-signed build has
/// an implicit designated requirement (`cdhash H"…"`, printed after `# `); it is
/// read like any other, and nothing ad-hoc satisfies the Developer ID half.
///
/// # Errors
/// A [`Refusal`] at [`Stage::RunningRequirement`]: the tool did not start or
/// ran past [`DISPLAY_BOUND`], the code is not signed at all (`codesign`
/// fails), or its answer is not one designated requirement; [`Why::NotMacOs`]
/// off macOS.
pub fn running_requirement() -> Result<Requirement, Refusal> {
    designated_requirement_of(&std::process::id().to_string()).map(Requirement::of_designated)
}

/// The designated requirement `codesign` prints for `code` — a path, or a
/// process id in decimal.
fn designated_requirement_of(code: &str) -> Result<String, Refusal> {
    let stage = Stage::RunningRequirement;
    let answer = arm::run(stage, CODESIGN, &["-d", "-r-", code], DISPLAY_BOUND)?;
    if answer.status != Some(0) {
        return Err(Refusal::quoting(
            stage,
            Why::Failed(answer.status),
            &answer.stderr,
        ));
    }
    parse_designated(&answer.stdout)
        .ok_or_else(|| Refusal::quoting(stage, Why::Malformed, &answer.stdout))
}

/// **The designated requirement in `codesign -d -r-`'s standard output.**
///
/// The grammar: non-empty lines of the form `<kind> => <requirement>`, each
/// optionally after `# ` (an implicit requirement, which the system computed
/// rather than the signature carries), where `<kind>` is one of `host`,
/// `guest`, `designated`, `library` or `plugin`; exactly one `designated`.
/// Anything else is `None`.
#[must_use]
pub fn parse_designated(stdout: &str) -> Option<String> {
    const KINDS: [&str; 5] = ["host", "guest", "designated", "library", "plugin"];
    let mut designated = None;
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let line = line.strip_prefix("# ").unwrap_or(line);
        let (kind, requirement) = line.split_once(" => ")?;
        if !KINDS.contains(&kind) || requirement.trim().is_empty() {
            return None;
        }
        if kind == "designated" {
            if designated.is_some() {
                return None;
            }
            designated = Some(requirement.trim().to_owned());
        }
    }
    designated
}

/// **Whether the bundle at `path` is valid and satisfies `requirement`.**
///
/// Two `codesign` runs, so that each failure names its own stage: first
/// `--verify --strict --deep --all-architectures` (every file sealed and
/// unaltered, nested code verified in full rather than shallowly, every
/// architecture of a universal binary), then the same with `-R=<text>`, whose
/// exit 3 is "valid, and does not satisfy it". `path` is absolute.
///
/// # Errors
/// A [`Refusal`] at [`Stage::Validity`] or [`Stage::Requirement`];
/// [`Why::NotMacOs`] off macOS.
pub fn verify_bundle(path: &Path, requirement: &Requirement) -> Result<Verified, Refusal> {
    verify_against(path, &requirement.text())
}

fn verify_against(path: &Path, requirement: &str) -> Result<Verified, Refusal> {
    const STRICT: [&str; 4] = ["--verify", "--strict", "--deep", "--all-architectures"];
    let target = path.to_string_lossy();

    let stage = Stage::Validity;
    let mut arguments = STRICT.to_vec();
    arguments.push(&target);
    let validity = arm::run(stage, CODESIGN, &arguments, VERIFY_BOUND)?;
    if validity.status != Some(0) {
        return Err(Refusal::quoting(
            stage,
            Why::Failed(validity.status),
            &validity.stderr,
        ));
    }

    let stage = Stage::Requirement;
    let test = format!("-R={requirement}");
    let mut arguments = STRICT.to_vec();
    arguments.push(&test);
    arguments.push(&target);
    let matched = arm::run(stage, CODESIGN, &arguments, VERIFY_BOUND)?;
    match matched.status {
        Some(0) => Ok(Verified(())),
        Some(3) => Err(Refusal::quoting(stage, Why::NotSatisfied, &matched.stderr)),
        status => Err(Refusal::quoting(
            stage,
            Why::Failed(status),
            &matched.stderr,
        )),
    }
}

/// **What Gatekeeper says about the bundle at `path`** (F-9).
///
/// `spctl --status` first: `assessments disabled` is [`Assessment::Disabled`]
/// and nothing is assessed. Otherwise `spctl --assess --type execute -vv`,
/// read by [`parse_assessment`]. `path` is absolute.
///
/// # Errors
/// A [`Refusal`] at [`Stage::GatekeeperStatus`] or
/// [`Stage::GatekeeperAssessment`] when a tool did not start, ran past its
/// bound, or answered outside its grammar; [`Why::NotMacOs`] off macOS.
pub fn assess(path: &Path) -> Result<Assessment, Refusal> {
    let stage = Stage::GatekeeperStatus;
    let status = arm::run(stage, SPCTL, &["--status"], DISPLAY_BOUND)?;
    let enabled = if status.status == Some(0) {
        parse_status(&status.stdout)
    } else {
        None
    }
    .ok_or_else(|| {
        Refusal::quoting(
            stage,
            Why::Malformed,
            &format!("{}\n{}", status.stdout, status.stderr),
        )
    })?;
    if !enabled {
        return Ok(Assessment::Disabled);
    }

    let stage = Stage::GatekeeperAssessment;
    let target = path.to_string_lossy();
    let answer = arm::run(
        stage,
        SPCTL,
        &["--assess", "--type", "execute", "-vv", &target],
        ASSESS_BOUND,
    )?;
    parse_assessment(&target, answer.status, &answer.stderr)
        .ok_or_else(|| Refusal::quoting(stage, Why::Malformed, &answer.stderr))
}

/// **`spctl --status`'s standard output**: `Some(true)` for
/// `assessments enabled`, `Some(false)` for `assessments disabled`, `None` for
/// anything else.
#[must_use]
pub fn parse_status(stdout: &str) -> Option<bool> {
    match stdout.trim() {
        "assessments enabled" => Some(true),
        "assessments disabled" => Some(false),
        _ => None,
    }
}

/// **`spctl --assess --type execute -vv <target>`'s answer**, from its exit
/// status and its standard error.
///
/// The grammar, measured on macOS 26.6 (2026-09-26):
///
/// ```text
/// <target>: accepted            exit 0
/// <target>: rejected[ (<reason>)]   exit 3
/// then lines key=value, key one of source, origin, override, each at most once
/// ```
///
/// Accepted needs a `source=`. Anything else — another first line (`spctl`
/// exits 1 with `<target>: a sealed resource is missing or invalid` and the
/// like), an exit status that disagrees with the verdict, an unknown or
/// repeated key — is `None`.
#[must_use]
pub fn parse_assessment(target: &str, status: Option<i32>, stderr: &str) -> Option<Assessment> {
    let mut lines = stderr.lines().filter(|line| !line.trim().is_empty());
    let verdict = lines.next()?.strip_prefix(target)?.strip_prefix(": ")?;
    let (mut source, mut origin, mut overridden) = (None, None, None);
    for line in lines {
        let (key, value) = line.split_once('=')?;
        let slot = match key {
            "source" => &mut source,
            "origin" => &mut origin,
            "override" => &mut overridden,
            _ => return None,
        };
        if slot.replace(value.trim().to_owned()).is_some() {
            return None;
        }
    }
    if verdict == "accepted" {
        return (status == Some(0)).then_some(Assessment::Accepted { source: source? });
    }
    let reason = verdict
        .strip_prefix("rejected")?
        .trim()
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
        .map(str::to_owned);
    if !(verdict == "rejected" || reason.is_some()) || status != Some(3) {
        return None;
    }
    Some(Assessment::Rejected {
        reason: source.or(reason),
    })
}

/// **F-9's decision**: identity is required in every case, and Gatekeeper's
/// answer decides how it passes.
///
/// | `verify` | `assess` | answer |
/// |---|---|---|
/// | refused | any | that refusal |
/// | verified | refused (tool or grammar) | that refusal |
/// | verified | accepted, *Notarized Developer ID* | [`Identity::Notarized`] |
/// | verified | accepted, another source | refused, [`Why::NotNotarizedDeveloperId`] |
/// | verified | rejected | refused, [`Why::Rejected`] |
/// | verified | disabled | [`Identity::GatekeeperOff`] |
///
/// # Errors
/// The [`Refusal`] of the table.
pub fn identity_decision(
    verify: Result<Verified, Refusal>,
    assess: Result<Assessment, Refusal>,
) -> Result<Identity, Refusal> {
    let Verified(()) = verify?;
    let stage = Stage::GatekeeperAssessment;
    match assess? {
        Assessment::Accepted { source } if source == NOTARIZED_DEVELOPER_ID => {
            Ok(Identity::Notarized)
        }
        Assessment::Accepted { source } => {
            Err(Refusal::at(stage, Why::NotNotarizedDeveloperId(source)))
        }
        Assessment::Rejected { reason } => Err(Refusal {
            stage,
            why: Why::Rejected,
            first_line: reason.as_deref().and_then(first_line),
        }),
        Assessment::Disabled => Ok(Identity::GatekeeperOff),
    }
}

/// What one child said: its exit status (`None` when a signal ended it) and
/// its two streams, as text.
struct Answer {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

#[cfg(target_os = "macos")]
mod arm {
    use super::{Answer, OUTPUT_BOUND, Refusal, Stage, Why};
    use std::io::{self, Read};
    use std::os::fd::AsRawFd;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    /// How often a waiting call looks at its child.
    const POLL: Duration = Duration::from_millis(10);

    /// Make a pipe's reads answer `WouldBlock` rather than wait, so a child that
    /// writes without end cannot hold the call past its bound.
    fn nonblocking(pipe: &impl AsRawFd) -> io::Result<()> {
        let fd = pipe.as_raw_fd();
        // SAFETY: `fd` is a pipe this call's child handle owns and keeps open
        // across both calls; `fcntl` with these commands reads and sets the
        // descriptor's status flags and touches no memory.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        // SAFETY: as above.
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Read what `pipe` has now into `into`. `Ok(true)` at end of stream;
    /// `Err(OutputPastBound)` past [`OUTPUT_BOUND`].
    fn drain(pipe: &mut impl Read, into: &mut Vec<u8>) -> Result<bool, Why> {
        let mut buffer = [0u8; 4096];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) => return Ok(true),
                Ok(read) => {
                    if into.len() + read > OUTPUT_BOUND {
                        return Err(Why::OutputPastBound);
                    }
                    into.extend_from_slice(&buffer[..read]);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return Ok(true),
            }
        }
    }

    /// **Run `program` with `arguments` under `bound`**, through the child
    /// door; a child past its bound, or past [`OUTPUT_BOUND`] on a stream, is
    /// ended by its own handle and the call refused at `stage`.
    pub(super) fn run(
        stage: Stage,
        program: &str,
        arguments: &[&str],
        bound: Duration,
    ) -> Result<Answer, Refusal> {
        let refuse = |why| Refusal::at(stage, why);
        let mut child = crate::quiet_command(program)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| refuse(Why::DidNotStart))?;
        let (mut stdout, mut stderr) = (
            child.stdout.take().expect("stdout was piped"),
            child.stderr.take().expect("stderr was piped"),
        );
        let deadline = Instant::now() + bound;
        let collected = (|| {
            nonblocking(&stdout).map_err(|_| Why::DidNotStart)?;
            nonblocking(&stderr).map_err(|_| Why::DidNotStart)?;
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let (mut out_done, mut err_done) = (false, false);
            loop {
                out_done = out_done || drain(&mut stdout, &mut out)?;
                err_done = err_done || drain(&mut stderr, &mut err)?;
                if let Some(status) = child.try_wait().map_err(|_| Why::DidNotStart)? {
                    // The child has gone; what it wrote is in the pipes.
                    if !out_done {
                        drain(&mut stdout, &mut out)?;
                    }
                    if !err_done {
                        drain(&mut stderr, &mut err)?;
                    }
                    return Ok((status.code(), out, err));
                }
                if Instant::now() >= deadline {
                    return Err(Why::TimedOut);
                }
                std::thread::sleep(POLL);
            }
        })();
        let (status, out, err) = collected.map_err(|why| {
            // Only the child this call started is ended, by its own handle.
            let _ = child.kill();
            let _ = child.wait();
            refuse(why)
        })?;
        let text = |bytes: Vec<u8>| String::from_utf8(bytes).map_err(|_| refuse(Why::NotText));
        Ok(Answer {
            status,
            stdout: text(out)?,
            stderr: text(err)?,
        })
    }
}

#[cfg(not(target_os = "macos"))]
mod arm {
    use super::{Answer, Refusal, Stage, Why};
    use std::time::Duration;

    /// Refused by name: this door has no arm off macOS.
    pub(super) fn run(
        stage: Stage,
        _program: &str,
        _arguments: &[&str],
        _bound: Duration,
    ) -> Result<Answer, Refusal> {
        Err(Refusal::at(stage, Why::NotMacOs))
    }
}

#[cfg(test)]
#[path = "macos_identity_tests.rs"]
mod tests;
