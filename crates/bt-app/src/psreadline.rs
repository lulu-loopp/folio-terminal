//! The PSReadLine a Windows PowerShell can be given, and the one question this
//! window asks about it — `docs/DESIGN.md` §7.1.6c-3b.
//!
//! # What is broken and for whom
//!
//! `Windows PowerShell 5.1` ships PSReadLine 2.0.0 and nothing has ever
//! replaced it. On 2.0.0 the edit anchor is derived from a cell count taken
//! before the resize, so narrowing the window leaves the input line drawn where
//! it used to be, over text that has moved. Folio already sends a private
//! resize-anchor chord for this — and `folio.ps1`'s own comment says what
//! happens to it on 2.0.0: the chord is *consumed as a no-op*, because the only
//! repair 2.0.0 offers (`InvokePrompt`) clears the viewport. So on the shell
//! most Windows users open first, the fix this product ships does nothing at
//! all, and there is no way for the person holding the machine to find that
//! out.
//!
//! 2.4.6 is the first version whose anchor is derived from the prompt's own
//! cell width, which is what makes it survive a resize. This crate carries it
//! ([`BUNDLED_FILES`]) and can write it into the user's module path.
//!
//! # Why the detection is a process and not an escape sequence
//!
//! The obvious alternative — have `folio.ps1` report the version over a private
//! OSC — was considered and rejected on two grounds, both fatal:
//!
//! 1. **The integration script is opt-in and manual.** `Integration::PowerShellOptIn`
//!    says so in as many words: it is dot-sourced by the user into `$PROFILE`
//!    and this product never injects it. An OSC would therefore reach exactly
//!    the people who have already edited their profile — and those are the
//!    least likely to still be on 2.0.0. The person this is for opened Folio,
//!    configured nothing, and dragged the window narrower.
//! 2. **`AllSigned` blocks the script itself.** `folio.ps1` is unsigned, so on
//!    the very machines where the Install button must be *disabled*, the OSC
//!    would never arrive at all. A criterion that goes silent precisely where
//!    it has to speak is not a criterion.
//!
//! So the version is read out of band, once per process, by starting
//! `powershell.exe -NoProfile -NonInteractive` on a background thread.
//!
//! # Where the version number lives
//!
//! In exactly one place: [`PATCHED_VERSION`]. `folio.ps1`'s gate
//! (`$psReadLineVersion -ge [version]'2.4.6'`) is the same number and
//! `the_patched_version_is_the_one_the_integration_script_gates_on` reads the
//! shipped bytes to prove it. A second literal would agree today and diverge on
//! the first bump, and the symptom — a module installed that the script still
//! treats as unproven — is invisible from either side.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::i18n::{self, Text};

/// The module version this build carries and installs.
///
/// **The only place this number is written on the Rust side.** See the module
/// header; `folio.ps1` holds the other half and a test binds them together.
pub const PATCHED_VERSION: &str = "2.4.6";

/// Where a per-user module for `Windows PowerShell 5.1` lives, under Documents.
///
/// `WindowsPowerShell` and not `PowerShell`: the two editions keep separate
/// module paths, and the one that is broken is 5.1. Writing into `PowerShell`
/// would install a patch for the shell that does not need it.
pub const MODULE_RELATIVE_PATH: &str = r"WindowsPowerShell\Modules\PSReadLine";

/// `include_bytes!` wants a literal, so the shared prefix is a macro rather than
/// a `const`.
macro_rules! asset {
    ($name:literal) => {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/psreadline/2.4.6/",
            $name
        )
    };
}

/// The nine files that are the module, compiled into the executable.
///
/// Bundled rather than downloaded, for the reason `NOTO_COLOR_EMOJI_BYTES` is:
/// a terminal that has to reach the network to fix its own input line is a
/// terminal that cannot fix it on the machine most likely to need it. 437 KB.
///
/// `License.txt` is in the list and is not optional — PSReadLine is BSD-2
/// (Copyright 2013 Jason Shirk), and a binary distribution must carry the
/// notice. It is listed here rather than remembered because a file dropped from
/// this array is a file that silently stops being installed.
///
/// The paths use `/` and are joined onto the destination as relative paths, so
/// the two `Polyfiller.dll`s land in their own subdirectories.
pub const BUNDLED_FILES: [(&str, &[u8]); 9] = [
    ("Changes.txt", include_bytes!(asset!("Changes.txt"))),
    ("License.txt", include_bytes!(asset!("License.txt"))),
    (
        "Microsoft.PowerShell.PSReadLine.dll",
        include_bytes!(asset!("Microsoft.PowerShell.PSReadLine.dll")),
    ),
    (
        "Microsoft.PowerShell.Pager.dll",
        include_bytes!(asset!("Microsoft.PowerShell.Pager.dll")),
    ),
    (
        "PSReadLine.format.ps1xml",
        include_bytes!(asset!("PSReadLine.format.ps1xml")),
    ),
    ("PSReadLine.psd1", include_bytes!(asset!("PSReadLine.psd1"))),
    ("PSReadLine.psm1", include_bytes!(asset!("PSReadLine.psm1"))),
    (
        "net6plus/Microsoft.PowerShell.PSReadLine.Polyfiller.dll",
        include_bytes!(asset!(
            "net6plus/Microsoft.PowerShell.PSReadLine.Polyfiller.dll"
        )),
    ),
    (
        "netstd/Microsoft.PowerShell.PSReadLine.Polyfiller.dll",
        include_bytes!(asset!(
            "netstd/Microsoft.PowerShell.PSReadLine.Polyfiller.dll"
        )),
    ),
];

/// A `System.Version`, compared the way PowerShell's `-ge [version]'2.4.6'`
/// compares one.
///
/// Three components and not a string comparison, which is the whole reason the
/// type exists: `"2.10.0"` is newer than `"2.4.6"` and sorts before it as text.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
}

impl Version {
    /// Parse `Major.Minor[.Build[.Revision]]`, which is every shape
    /// `System.Version` prints.
    ///
    /// The revision is read and discarded rather than refused: PSReadLine has
    /// never shipped one, but a `2.4.6.0` from some future packaging must
    /// compare equal to `2.4.6` and not fail to parse into "unknown".
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let mut parts = text.split('.');
        let major = parts.next()?.trim().parse().ok()?;
        let minor = parts.next().unwrap_or("0").trim().parse().ok()?;
        let build = parts.next().unwrap_or("0").trim().parse().ok()?;
        if let Some(revision) = parts.next() {
            revision.trim().parse::<u32>().ok()?;
        }
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            build,
        })
    }

    /// The version as `System.Version` would print it.
    #[must_use]
    pub fn text(self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.build)
    }
}

/// The version this build installs, parsed. Panics only if [`PATCHED_VERSION`]
/// is malformed, which a test also pins.
#[must_use]
pub fn patched_version() -> Version {
    Version::parse(PATCHED_VERSION).expect("PATCHED_VERSION is a literal in this file")
}

/// Windows' script execution policy, as `Get-ExecutionPolicy` reports it.
///
/// Only the distinction that matters here is modelled — whether an unsigned
/// script module can be imported — but the six names are kept apart anyway,
/// because the reason line puts the policy's own name on screen and "the policy
/// is Blocked" is not a sentence Windows would ever write.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ExecutionPolicy {
    Restricted,
    AllSigned,
    RemoteSigned,
    Unrestricted,
    Bypass,
    /// No policy is set anywhere, which on a client Windows behaves as
    /// `Restricted` — but `Get-ExecutionPolicy` prints `Undefined`, and this is
    /// the name that goes on screen.
    Undefined,
    /// The probe did not answer, or answered something this build has no name
    /// for. Treated as permissive, because refusing on an unknown answer would
    /// disable the button on every machine whose PowerShell is missing.
    #[default]
    Unknown,
}

impl ExecutionPolicy {
    #[must_use]
    pub fn parse(text: &str) -> Self {
        match text.trim() {
            "Restricted" => Self::Restricted,
            "AllSigned" => Self::AllSigned,
            "RemoteSigned" => Self::RemoteSigned,
            "Unrestricted" => Self::Unrestricted,
            "Bypass" => Self::Bypass,
            "Undefined" => Self::Undefined,
            _ => Self::Unknown,
        }
    }

    /// `Get-ExecutionPolicy`'s own word, for the reason line.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Restricted => "Restricted",
            Self::AllSigned => "AllSigned",
            Self::RemoteSigned => "RemoteSigned",
            Self::Unrestricted => "Unrestricted",
            Self::Bypass => "Bypass",
            Self::Undefined => "Undefined",
            Self::Unknown => "Unknown",
        }
    }

    /// Whether a module Folio writes could be imported at all.
    ///
    /// **`Restricted` joins `AllSigned` and that is not an over-reach.** The
    /// question is not "does Windows trust Folio" but "will PowerShell load an
    /// unsigned `.psm1`" — PSReadLine is a script module, the bundled copy is
    /// unsigned (a fork's build is), and both policies answer no. Writing 437 KB
    /// of files that the shell will refuse at import, and reporting success, is
    /// the one outcome worse than a disabled button.
    ///
    /// `Undefined` is *not* on the list even though a client Windows resolves it
    /// to `Restricted`, because `Get-ExecutionPolicy` returns the **effective**
    /// policy: a machine that resolves `Undefined` to `Restricted` reports
    /// `Restricted`. Seeing `Undefined` come back means some scope explicitly
    /// set it, which is the permissive case.
    #[must_use]
    pub fn refuses_unsigned_modules(self) -> bool {
        matches!(self, Self::AllSigned | Self::Restricted)
    }
}

/// What the out-of-band probe found.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Probe {
    /// The highest PSReadLine `Get-Module -ListAvailable` reported, or `None` if
    /// PowerShell could not be started or reported nothing.
    pub version: Option<Version>,
    pub policy: ExecutionPolicy,
}

impl Probe {
    /// Whether the machine's own module already anchors itself.
    #[must_use]
    pub fn already_current(self) -> bool {
        self.version.is_some_and(|found| found >= patched_version())
    }

    /// The version to put on screen — the machine's own, or the patched one
    /// when the machine reported nothing at all.
    #[must_use]
    pub fn found_text(self) -> String {
        self.version
            .map_or_else(|| Text::PsReadLineProbing.text().to_owned(), Version::text)
    }
}

static PROBE: OnceLock<Probe> = OnceLock::new();

/// Start the probe, once per process, on a thread of its own.
///
/// **Two triggers, and both are "somebody is in a position to ask"**: the first
/// `Windows PowerShell` pane opening, and — since §7.1.6c-5 — the settings
/// dialog showing the page the answer is written on. A user who only ever opens
/// WSL still pays nothing; a user who never opens a 5.1 pane used to read
/// "Checking this machine's PSReadLine" forever, because the row was drawn by a
/// probe that had never been started. Calling it again is free.
///
/// The window is woken through [`install_wake`] when the answer lands, and the
/// wake belongs to the *process* rather than to whichever trigger happened to
/// fire first. That is the whole reason it is not an argument here: the answer
/// is a one-shot, so only the call that actually spawns the thread could carry
/// a callback — and the caller that spawns it is the pane, while the caller that
/// needs the repaint is the dialog, which may open minutes later while the probe
/// is still running.
pub fn begin_probe() {
    if PROBE.get().is_some() || probing_started() {
        return;
    }
    std::thread::Builder::new()
        .name("psreadline-probe".to_owned())
        .spawn(|| {
            let _ = PROBE.set(run_probe());
            // After the answer is published, never before: a wake that raced the
            // `set` would send the loop to read a row that is still `Probing`,
            // and there is no second wake coming.
            if let Some(wake) = WAKE.get() {
                wake();
            }
        })
        .ok();
}

/// Every answer this module publishes out of band is published on a thread with
/// no window, so the window has to be told to come and read it.
static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Teach the probe how to bring the event loop round when its answer lands
/// (§7.1.6c-5).
///
/// Called once, at startup, beside [`install_probe_override`]: a settings dialog
/// standing on the Terminal page while the probe is still out has a row reading
/// "Checking this machine's PSReadLine", and nothing else in this window is
/// going to produce a frame on its own to replace it — a modal is up, so there
/// is no shell output, no hover and no keystroke coming.
///
/// A second call is ignored, which is what a process-lifetime answer means.
pub fn install_wake(wake: impl Fn() + Send + Sync + 'static) {
    let _ = WAKE.set(Box::new(wake));
}

static PROBE_STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn probing_started() -> bool {
    PROBE_STARTED.swap(true, std::sync::atomic::Ordering::SeqCst)
}

/// What the probe found, or `None` while it is still running.
#[must_use]
pub fn probe() -> Option<Probe> {
    PROBE.get().copied()
}

/// Seed the probe's answer directly.
///
/// **A diagnostics door, in the family of `BT_IME_TRACE` and
/// `BT_STARTUP_TRACE`**, and it earns its place for the reason those two do:
/// the state it produces cannot be reached on the machine that has to be
/// photographed. The invitation only appears on a Windows whose PSReadLine is
/// older than the patched one, and a development machine that has already been
/// given the patch can never show it again — so the dialog would ship having
/// been seen only in a unit test.
///
/// `BT_PSREADLINE_PROBE=<version>[,<policy>]`, e.g. `2.0.0` or
/// `2.0.0,AllSigned`. It replaces only what the machine is *read* as; the two
/// verbs still write and delete real files, and
/// [`documents_directory`]'s own door is what keeps those off a real module
/// path.
pub fn probe_override_from_env() -> Option<Probe> {
    let raw = std::env::var("BT_PSREADLINE_PROBE").ok()?;
    let mut parts = raw.split(',');
    let version = Version::parse(parts.next()?);
    let policy = parts.next().map_or(ExecutionPolicy::RemoteSigned, |name| {
        ExecutionPolicy::parse(name)
    });
    Some(Probe { version, policy })
}

/// Install the override, if one was asked for, before anything reads the probe.
pub fn install_probe_override() {
    if let Some(probe) = probe_override_from_env() {
        let _ = PROBE.set(probe);
        probing_started();
    }
}

/// The one command: the highest installed PSReadLine and the effective policy,
/// one per line.
///
/// `-NoProfile` because a profile is exactly what must not run — it may print,
/// it may take seconds, and on the machines this is aimed at it is where the
/// user's own PSReadLine configuration lives. `-NonInteractive` so nothing can
/// stop for a prompt on a thread with no console.
const PROBE_COMMAND: &str = "\
$m = Get-Module -ListAvailable PSReadLine | Sort-Object Version -Descending | Select-Object -First 1; \
if ($m) { $m.Version.ToString() } else { '' }; \
(Get-ExecutionPolicy).ToString()";

#[cfg(windows)]
fn run_probe() -> Probe {
    use std::os::windows::process::CommandExt;
    // `CREATE_NO_WINDOW`. Without it a console flashes on screen every time a
    // PowerShell pane is opened for the first time in a session.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", PROBE_COMMAND])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let Ok(output) = output else {
        return Probe::default();
    };
    parse_probe_output(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(windows))]
fn run_probe() -> Probe {
    Probe::default()
}

/// Read the two lines the probe command writes.
///
/// Split out so the parsing is testable without a PowerShell: the failure this
/// guards is a build that reads the policy off the version line, which on a
/// machine with no PSReadLine would report the policy as the version.
#[must_use]
pub fn parse_probe_output(stdout: &str) -> Probe {
    let mut lines = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    // The version line may be absent entirely — the command writes `''`, which
    // `lines()` then drops as empty — so the policy can arrive first. It is told
    // apart by shape rather than by position: a version parses and a policy name
    // does not.
    let mut version = None;
    let mut policy = ExecutionPolicy::Unknown;
    for line in lines.by_ref() {
        match Version::parse(line) {
            Some(parsed) if version.is_none() => version = Some(parsed),
            _ => {
                policy = ExecutionPolicy::parse(line);
                break;
            }
        }
    }
    Probe { version, policy }
}

// ── the row on the Terminal page ────────────────────────────────────────────

/// What the Terminal page's PSReadLine row is currently describing.
///
/// Derived from the probe and the stored invitation state together, never from
/// either alone: what is on the machine and what this user was told about it
/// are two facts, and the row's job is to reconcile them out loud.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RowState {
    /// The probe has not answered yet.
    #[default]
    Probing,
    /// Older than the patched module, and Folio did not put it there.
    Outdated,
    /// Folio wrote the module and it is still on disk.
    InstalledByFolio,
    /// The machine's own module is already new enough.
    AlreadyCurrent,
    /// `settings.json` says Folio installed it and it is not there any more.
    RemovedElsewhere,
}

/// Reconcile the probe with the stored state.
///
/// `installed_on_disk` is a filesystem question the caller answers, because this
/// function is pure and the answer changes under it: the user may install or
/// remove during the session, long after the probe's one reading.
#[must_use]
pub fn row_state(
    probe: Option<Probe>,
    invite: bt_persist::PsReadLineInviteV1,
    installed_on_disk: bool,
) -> RowState {
    if installed_on_disk {
        return RowState::InstalledByFolio;
    }
    if invite == bt_persist::PsReadLineInviteV1::Installed {
        // The file says Folio installed it and the directory is gone. Said out
        // loud rather than quietly rewritten to `NotAsked`, because the reader
        // is the only one who can know whether that was them.
        return RowState::RemovedElsewhere;
    }
    let Some(probe) = probe else {
        return RowState::Probing;
    };
    if probe.already_current() {
        RowState::AlreadyCurrent
    } else {
        RowState::Outdated
    }
}

/// The row's description line.
///
/// `&'static str` because [`crate::settings::SettingsRow::description`] is, and
/// that signature is the i18n ruling's own constraint. The versions inside these
/// sentences are runtime values, so each state's sentence is built once into a
/// `OnceLock` — which is sound because the probe is itself a one-shot: a state's
/// text cannot change once that state has been reached.
///
/// **One slot per language, not one slot** (§7.1.6c-3c). The language can move
/// while the window is up, and this is the only cache in the app that would have
/// survived the move with the old words in it: a reader who switched to Chinese
/// with the Terminal page open would have watched every line on it change except
/// this one. The probe's one-shot argument still holds — what a *state* says
/// cannot change — so nothing here is ever invalidated; a second language simply
/// fills a second slot the first time it is asked.
#[must_use]
pub fn row_description(state: RowState) -> &'static str {
    row_description_in(state, i18n::current())
}

/// The same line in a named language — the entry point for the test that reads
/// both columns out of the cache at once.
///
/// The array length is [`i18n::Lang::COUNT`] and the index is
/// [`i18n::Lang::index`], so a third language is a compile error here rather
/// than a third column quietly sharing the second's slot.
#[must_use]
pub fn row_description_in(state: RowState, lang: i18n::Lang) -> &'static str {
    static OUTDATED: [OnceLock<String>; i18n::Lang::COUNT] = [OnceLock::new(), OnceLock::new()];
    static INSTALLED: [OnceLock<String>; i18n::Lang::COUNT] = [OnceLock::new(), OnceLock::new()];
    static CURRENT: [OnceLock<String>; i18n::Lang::COUNT] = [OnceLock::new(), OnceLock::new()];
    let slot = lang.index();
    match state {
        RowState::Probing => Text::PsReadLineProbing.in_lang(lang),
        RowState::RemovedElsewhere => Text::PsReadLineRowGone.in_lang(lang),
        RowState::Outdated => OUTDATED[slot]
            .get_or_init(|| {
                i18n::psreadline_row_outdated_in(lang, &probe().unwrap_or_default().found_text())
            })
            .as_str(),
        RowState::InstalledByFolio => INSTALLED[slot]
            .get_or_init(|| i18n::psreadline_row_installed_in(lang, PATCHED_VERSION))
            .as_str(),
        RowState::AlreadyCurrent => CURRENT[slot]
            .get_or_init(|| {
                i18n::psreadline_row_current_in(lang, &probe().unwrap_or_default().found_text())
            })
            .as_str(),
    }
}

/// Whether the row's `On` item can be chosen.
///
/// Off under a policy that would refuse the module, and off while nothing is
/// known yet — a picker that let the user ask for an install before the machine
/// had been read would be a picker that could install over a newer module.
#[must_use]
pub fn install_available(probe: Option<Probe>, state: RowState) -> bool {
    let Some(probe) = probe else {
        return false;
    };
    !probe.policy.refuses_unsigned_modules()
        && matches!(state, RowState::Outdated | RowState::RemovedElsewhere)
}

/// Whether the row's `Off` item can be chosen.
///
/// **Only ever removes what Folio wrote.** A machine whose own PSReadLine is
/// newer, or older, is not this row's to touch — the `Off` item is dark there,
/// which is the same sentence the default-profile picker's greyed rows speak.
#[must_use]
pub fn remove_available(state: RowState) -> bool {
    state == RowState::InstalledByFolio
}

// ── the invitation ──────────────────────────────────────────────────────────

/// Whether the invitation is owed, and why.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InviteDecision {
    /// Show the dialog.
    Show,
    /// Say nothing.
    Stay,
}

/// The trigger table — `docs/DESIGN.md` §7.1.6c-3b.
///
/// | probe | stored state | after a font-size change | answer |
/// |---|---|---|---|
/// | `< 2.4.6` | `NotAsked` | either | **Show** |
/// | `< 2.4.6` | `Declined` | no | Stay |
/// | `< 2.4.6` | `Declined` | yes | **Show**, once, then `Dismissed` |
/// | `< 2.4.6` | `Installed` / `Dismissed` | either | Stay |
/// | `>= 2.4.6` | any | either | Stay |
/// | not answered | any | either | Stay |
///
/// The one row worth arguing is `Declined` + a font-size change. A user who said
/// no is owed silence, and this is the single exception: changing the font size
/// is the one action in the whole product whose visible consequence on an
/// unpatched 5.1 *is the bug* — the grid re-flows under an input line that stays
/// where it was. Asking there is asking while the thing being offered is on
/// screen. It happens once and the state moves to `Dismissed` whatever the
/// answer, so there is no second exception.
#[must_use]
pub fn invite_decision(
    probe: Option<Probe>,
    invite: bt_persist::PsReadLineInviteV1,
    after_font_size_change: bool,
) -> InviteDecision {
    use bt_persist::PsReadLineInviteV1 as State;
    let Some(probe) = probe else {
        return InviteDecision::Stay;
    };
    if probe.already_current() {
        return InviteDecision::Stay;
    }
    match invite {
        State::NotAsked => InviteDecision::Show,
        State::Declined if after_font_size_change => InviteDecision::Show,
        State::Declined | State::Installed | State::Dismissed => InviteDecision::Stay,
    }
}

/// The state a refusal moves the file to.
///
/// `NotAsked` becomes `Declined` — one more showing is owed — and everything
/// else becomes `Dismissed`. Written as a function because the two-strike rule
/// is the whole of the invitation's contract with the user and a rule spelled at
/// the call site is a rule the second call site gets wrong.
#[must_use]
pub fn state_after_decline(
    invite: bt_persist::PsReadLineInviteV1,
) -> bt_persist::PsReadLineInviteV1 {
    use bt_persist::PsReadLineInviteV1 as State;
    match invite {
        State::NotAsked => State::Declined,
        State::Declined => State::Dismissed,
        // Neither state shows the dialog, so neither can be refused from it —
        // and an `Installed` quietly rewritten to `Dismissed` here would be a
        // record of an install this product would then deny having made.
        settled @ (State::Installed | State::Dismissed) => settled,
    }
}

/// Whether the invitation is up, and what the pointer is over.
///
/// [`crate::restore::DirtyGate`]'s shape, and a separate type from it for the
/// reason `psreadline.rs` exists at all: the gate asks about work that is about
/// to be lost and this asks about a module, and one type answering both would
/// be one `open` flag that two unrelated ladders of Esc have to share.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Invite {
    open: bool,
    hover: Option<crate::restore::InviteTarget>,
}

impl Invite {
    #[must_use]
    pub fn is_open(self) -> bool {
        self.open
    }

    pub fn open(&mut self) {
        self.open = true;
        self.hover = None;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.hover = None;
    }

    #[must_use]
    pub fn hover(self) -> Option<crate::restore::InviteTarget> {
        self.hover
    }

    /// Returns whether the drawing has to change.
    pub fn set_hover(&mut self, hover: Option<crate::restore::InviteTarget>) -> bool {
        let changed = self.hover != hover;
        self.hover = hover;
        changed
    }
}

/// The sentences the invitation shows, before they are wrapped and measured.
///
/// Returned together because they are decided together: whether Install is
/// offered is what decides whether there is a reason line, and a caller that
/// derived one without the other could show a dark button with nothing beside
/// it.
#[must_use]
pub fn invite_body(probe: Option<Probe>, install_path: &Path) -> (String, Option<String>) {
    let probe = probe.unwrap_or_default();
    let body = i18n::psreadline_invite_body(
        &probe.found_text(),
        PATCHED_VERSION,
        &install_path.display().to_string(),
    );
    let reason = probe
        .policy
        .refuses_unsigned_modules()
        .then(|| i18n::psreadline_policy_reason(probe.policy.name()));
    (body, reason)
}

// ── writing and removing the module ─────────────────────────────────────────

/// The directory the module goes in, under a Documents folder.
#[must_use]
pub fn module_directory(documents: &Path) -> PathBuf {
    documents.join(MODULE_RELATIVE_PATH).join(PATCHED_VERSION)
}

/// Where this machine's Documents folder is, asked of Windows.
///
/// `None` on a machine Windows would not answer for, which disables the
/// invitation rather than guessing: `%USERPROFILE%\Documents` is wrong on every
/// redirected profile, and a module written there is a module PowerShell never
/// looks at.
#[must_use]
pub fn documents_directory() -> Option<PathBuf> {
    // **The second half of the diagnostics door**, and the half that makes the
    // first one usable. `BT_PSREADLINE_PROBE` can make a machine read as though
    // it needed the module; without somewhere else to put it, exercising the
    // Install button on a development machine would write into that developer's
    // own `Documents\WindowsPowerShell\Modules` — the one directory this
    // feature must be provably careful with. `BT_PSREADLINE_DOCUMENTS=<dir>`
    // moves the whole module path, read and write together, so an install can
    // be performed and photographed for real inside a scratch directory.
    //
    // Read and write *together* is the point: a door that redirected only the
    // reading would produce a row saying "installed" beside files that went
    // somewhere else, which is the one state this row exists to make
    // impossible. It is the same seam `install_into` and `remove_from` already
    // take for their tests, exposed rather than duplicated.
    if let Some(sandbox) = std::env::var_os("BT_PSREADLINE_DOCUMENTS") {
        let sandbox = PathBuf::from(sandbox);
        if !sandbox.as_os_str().is_empty() {
            return Some(sandbox);
        }
    }
    #[cfg(windows)]
    {
        bt_platform::documents_directory()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Write the nine bundled files into `documents`.
///
/// Creates the directories it needs and overwrites what is there, which is the
/// right behaviour for the only case that reaches it: a previous install that
/// was interrupted, or a file that was edited. It never touches a *different*
/// version's directory — `2.4.6` is a leaf of its own, which is how PowerShell's
/// module path is organised and why a per-version directory is the unit here.
///
/// Takes a Documents root rather than reading one, so a test can install into a
/// temporary directory and read back exactly what a real install would write.
pub fn install_into(documents: &Path) -> io::Result<PathBuf> {
    let root = module_directory(documents);
    for (name, bytes) in BUNDLED_FILES {
        let path = root.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes)?;
    }
    Ok(root)
}

/// Whether the directory under `documents` holds **this build's** copy, byte for
/// byte.
///
/// **The guard on removal, and it is content and not a marker file.** A marker
/// would have to be written into a directory PowerShell scans for modules, and
/// it would answer the wrong question anyway: what has to be true before Folio
/// deletes something is not "Folio wrote a note here" but "these are the bytes
/// Folio ships". A user who built the same fork themselves, or who copied
/// Folio's install by hand, gets the same answer for the same reason — the files
/// are interchangeable.
///
/// Every file is compared, not a sample: a directory holding eight of the nine
/// plus somebody's own edit of the ninth is not this build's copy.
#[must_use]
pub fn is_folios_copy(documents: &Path) -> bool {
    let root = module_directory(documents);
    if !root.is_dir() {
        return false;
    }
    BUNDLED_FILES.iter().all(|(name, bytes)| {
        std::fs::read(root.join(name)).is_ok_and(|found| found.as_slice() == *bytes)
    })
}

/// Delete the module, and only if [`is_folios_copy`] says it is Folio's.
///
/// The check is inside rather than at the call site, because a delete guarded
/// from outside is a delete that the next caller performs unguarded. `Ok(false)`
/// means the directory was left alone — there was nothing there, or what was
/// there was not this build's.
pub fn remove_from(documents: &Path) -> io::Result<bool> {
    if !is_folios_copy(documents) {
        return Ok(false);
    }
    std::fs::remove_dir_all(module_directory(documents))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_persist::PsReadLineInviteV1 as State;

    fn temp_dir(tag: &str) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("folio-psreadline-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// PIN (N28) — **the version this build installs and the version
    /// `folio.ps1` gates on are one number.**
    ///
    /// The two halves cannot see each other. The script's branch reads
    /// `$psReadLineVersion -ge [version]'2.4.6'` and decides whether to send the
    /// reflection repair or to swallow the chord; this file decides what to
    /// write to disk. Bump one alone and the product installs a module its own
    /// script still treats as unproven — the chord is consumed as a no-op, the
    /// resize bug stays, and every surface reports success.
    ///
    /// It reads the shipped bytes rather than a copy of them, for the reason
    /// `the_integration_script_names_the_profiles_own_titles` gives.
    ///
    /// MUTATION: change either literal alone and this fails naming the one that
    /// moved.
    #[test]
    fn the_patched_version_is_the_one_the_integration_script_gates_on() {
        let script = crate::shell_integration::script_source_ps1();
        let gate = format!("[version]'{PATCHED_VERSION}'");
        assert!(
            script.contains(&gate),
            "folio.ps1 gates the reflection repair on a version literal; it does \
             not contain {gate:?}, so the module this build installs is not the \
             one the script would recognise"
        );
        assert_eq!(
            script.matches("[version]'").count(),
            1,
            "the script names a PSReadLine version in one place, not two"
        );
        assert_eq!(patched_version(), Version::parse("2.4.6").unwrap());
    }

    /// PIN — and the module that ships says the same number in its own manifest.
    ///
    /// PowerShell resolves a module's version from `ModuleVersion` in the
    /// `.psd1`, not from the directory name, so a bundle whose manifest
    /// disagreed with [`PATCHED_VERSION`] would install into a `2.4.6` directory
    /// and be reported by `Get-Module` as something else — and the script's gate
    /// reads what `Get-Module` says.
    #[test]
    fn the_bundled_manifest_declares_the_version_this_build_installs() {
        let (_, manifest) = BUNDLED_FILES
            .iter()
            .find(|(name, _)| *name == "PSReadLine.psd1")
            .expect("the manifest is one of the nine");
        let text = String::from_utf8_lossy(manifest);
        assert!(
            text.contains(&format!("ModuleVersion = '{PATCHED_VERSION}'")),
            "the bundled PSReadLine.psd1 does not declare ModuleVersion = \
             '{PATCHED_VERSION}'"
        );
    }

    /// PIN — the licence ships with the binary.
    ///
    /// PSReadLine is BSD-2. A binary distribution must carry the notice, and the
    /// only place it can travel is inside the bundle, so its absence from the
    /// array is a licence violation that nothing else in the build would notice.
    #[test]
    fn the_bundle_carries_the_licence_and_all_nine_files() {
        assert_eq!(BUNDLED_FILES.len(), 9);
        let (_, licence) = BUNDLED_FILES
            .iter()
            .find(|(name, _)| *name == "License.txt")
            .expect("BSD-2 requires the notice to travel with the binary");
        let text = String::from_utf8_lossy(licence);
        assert!(text.contains("Copyright"), "the notice must be the notice");
        for (_, bytes) in BUNDLED_FILES {
            assert!(!bytes.is_empty(), "a bundled file must not be empty");
        }
    }

    /// PIN — a version is compared as three numbers, never as text.
    ///
    /// `"2.10.0" < "2.4.6"` as strings, and the day PSReadLine reaches 2.10 a
    /// text comparison would start offering to *downgrade* every machine.
    #[test]
    fn versions_compare_as_numbers_and_parse_every_shape_windows_prints() {
        assert!(Version::parse("2.10.0").unwrap() > patched_version());
        assert!(Version::parse("2.0.0").unwrap() < patched_version());
        assert_eq!(Version::parse("2.4.6.0").unwrap(), patched_version());
        assert_eq!(
            Version::parse("2.4").unwrap(),
            Version {
                major: 2,
                minor: 4,
                build: 0
            }
        );
        assert_eq!(Version::parse(""), None);
        assert_eq!(Version::parse("Restricted"), None);
        assert_eq!(Version::parse("2.4.6.0.1"), None);
        assert_eq!(patched_version().text(), PATCHED_VERSION);
    }

    /// PIN — the probe reads two answers and cannot mistake one for the other.
    ///
    /// The failure this catches is the one a positional reader has: on a machine
    /// with no PSReadLine at all the command writes an empty line, `lines()`
    /// drops it, and a reader taking "line 0" as the version would read the
    /// policy name there — parsing it as no version and then reporting the
    /// policy as `Unknown`, which would silently enable Install under
    /// `AllSigned`.
    #[test]
    fn the_probe_tells_the_version_line_from_the_policy_line_by_shape() {
        let both = parse_probe_output("2.0.0\r\nAllSigned\r\n");
        assert_eq!(both.version, Version::parse("2.0.0"));
        assert_eq!(both.policy, ExecutionPolicy::AllSigned);

        let policy_only = parse_probe_output("\r\nAllSigned\r\n");
        assert_eq!(
            policy_only.version, None,
            "a machine with no PSReadLine writes an empty version line"
        );
        assert_eq!(
            policy_only.policy,
            ExecutionPolicy::AllSigned,
            "and the policy must still be read, not swallowed as a bad version"
        );

        let nothing = parse_probe_output("");
        assert_eq!(nothing, Probe::default());
        assert_eq!(
            nothing.policy,
            ExecutionPolicy::Unknown,
            "a probe that answered nothing must not read as a restrictive policy"
        );
    }

    /// PIN — only `AllSigned` and `Restricted` stop the install, and the
    /// permissive answers include the one that means "nothing was set".
    ///
    /// MUTATION: add `Undefined` to the refusing list and the button goes dark
    /// on every machine whose administrator set a policy at one scope and left
    /// another undefined — which is most of them.
    #[test]
    fn only_the_policies_that_refuse_an_unsigned_module_disable_the_install() {
        for policy in [ExecutionPolicy::AllSigned, ExecutionPolicy::Restricted] {
            assert!(policy.refuses_unsigned_modules(), "{policy:?}");
        }
        for policy in [
            ExecutionPolicy::RemoteSigned,
            ExecutionPolicy::Unrestricted,
            ExecutionPolicy::Bypass,
            ExecutionPolicy::Undefined,
            ExecutionPolicy::Unknown,
        ] {
            assert!(!policy.refuses_unsigned_modules(), "{policy:?}");
        }
        assert_eq!(
            ExecutionPolicy::parse("AllSigned"),
            ExecutionPolicy::AllSigned
        );
        assert_eq!(ExecutionPolicy::parse("nonsense"), ExecutionPolicy::Unknown);
    }

    /// PIN — **the trigger table, every row of it.**
    ///
    /// The rule this exists to hold is "asked once, and once more only where the
    /// bug is visible". Every row that says `Stay` is a launch on which nothing
    /// interrupts the user, and the failure mode of getting one wrong is a
    /// dialog that comes back after being refused — which is the single worst
    /// thing an invitation like this can do.
    #[test]
    fn the_invitation_is_owed_once_and_once_more_only_after_a_font_size_change() {
        let old = Some(Probe {
            version: Version::parse("2.0.0"),
            policy: ExecutionPolicy::RemoteSigned,
        });
        let current = Some(Probe {
            version: Version::parse("2.4.6"),
            policy: ExecutionPolicy::RemoteSigned,
        });
        let newer = Some(Probe {
            version: Version::parse("2.10.0"),
            policy: ExecutionPolicy::RemoteSigned,
        });

        assert_eq!(
            invite_decision(old, State::NotAsked, false),
            InviteDecision::Show
        );
        assert_eq!(
            invite_decision(old, State::Declined, false),
            InviteDecision::Stay,
            "a user who said no is owed silence on the next launch"
        );
        assert_eq!(
            invite_decision(old, State::Declined, true),
            InviteDecision::Show,
            "and exactly one more showing, where the symptom is on screen"
        );
        for state in [State::Installed, State::Dismissed] {
            for after in [false, true] {
                assert_eq!(
                    invite_decision(old, state, after),
                    InviteDecision::Stay,
                    "{state:?} after={after}"
                );
            }
        }
        for probe in [current, newer] {
            for state in [State::NotAsked, State::Declined] {
                assert_eq!(
                    invite_decision(probe, state, true),
                    InviteDecision::Stay,
                    "a machine that already anchors itself is never asked"
                );
            }
        }
        for state in [State::NotAsked, State::Declined] {
            assert_eq!(
                invite_decision(None, state, true),
                InviteDecision::Stay,
                "and neither is one that has not been read yet"
            );
        }
    }

    /// PIN — refusing twice is the end of it.
    #[test]
    fn a_refusal_costs_one_strike_and_the_second_ends_the_invitation() {
        assert_eq!(state_after_decline(State::NotAsked), State::Declined);
        assert_eq!(state_after_decline(State::Declined), State::Dismissed);
        assert_eq!(state_after_decline(State::Dismissed), State::Dismissed);
        assert_eq!(
            state_after_decline(State::Installed),
            State::Installed,
            "a dialog cannot be refused in a state that never shows it, but if it \
             were, the install must not be forgotten"
        );
    }

    /// PIN — the row reconciles what is on the machine with what the file says,
    /// and says so when the two disagree.
    #[test]
    fn the_row_tells_an_install_from_a_newer_module_from_one_that_vanished() {
        let old = Some(Probe {
            version: Version::parse("2.0.0"),
            policy: ExecutionPolicy::RemoteSigned,
        });
        let current = Some(Probe {
            version: Version::parse("2.4.6"),
            policy: ExecutionPolicy::RemoteSigned,
        });
        assert_eq!(row_state(None, State::NotAsked, false), RowState::Probing);
        assert_eq!(row_state(old, State::NotAsked, false), RowState::Outdated);
        assert_eq!(
            row_state(current, State::NotAsked, false),
            RowState::AlreadyCurrent
        );
        assert_eq!(
            row_state(old, State::Installed, true),
            RowState::InstalledByFolio
        );
        assert_eq!(
            row_state(old, State::Installed, false),
            RowState::RemovedElsewhere,
            "the file says Folio wrote it and the directory is gone — a fact the \
             row owes the reader rather than one to quietly correct"
        );
    }

    /// PIN — the two picker items are dark exactly where the action would be a
    /// lie.
    ///
    /// `Off` on a module Folio did not write is the dangerous one: it is the
    /// only path in this product that deletes a directory outside its own data
    /// folder.
    #[test]
    fn the_picker_offers_only_the_action_it_can_actually_perform() {
        let signed = Some(Probe {
            version: Version::parse("2.0.0"),
            policy: ExecutionPolicy::AllSigned,
        });
        let open = Some(Probe {
            version: Version::parse("2.0.0"),
            policy: ExecutionPolicy::RemoteSigned,
        });
        assert!(install_available(open, RowState::Outdated));
        assert!(
            !install_available(signed, RowState::Outdated),
            "a module written under AllSigned would be refused at import"
        );
        assert!(
            !install_available(open, RowState::AlreadyCurrent),
            "nothing is gained by writing over a newer module"
        );
        assert!(
            !install_available(None, RowState::Probing),
            "and nothing is written before the machine has been read"
        );
        assert!(remove_available(RowState::InstalledByFolio));
        for state in [
            RowState::Probing,
            RowState::Outdated,
            RowState::AlreadyCurrent,
            RowState::RemovedElsewhere,
        ] {
            assert!(
                !remove_available(state),
                "{state:?} — a module Folio did not write is not Folio's to delete"
            );
        }
    }

    /// PIN — an install writes all nine files where PowerShell looks, and a
    /// remove takes exactly them back.
    #[test]
    fn an_install_round_trips_through_a_documents_folder() {
        let documents = temp_dir("round-trip");
        assert!(!is_folios_copy(&documents));
        assert!(
            !remove_from(&documents).unwrap(),
            "there is nothing there to remove"
        );

        let root = install_into(&documents).unwrap();
        assert!(
            root.ends_with(PathBuf::from(MODULE_RELATIVE_PATH).join(PATCHED_VERSION)),
            "the module must land where PowerShell's per-user module path looks: \
             {root:?}"
        );
        for (name, bytes) in BUNDLED_FILES {
            let written = std::fs::read(root.join(name)).expect(name);
            assert_eq!(written.as_slice(), bytes, "{name} was written altered");
        }
        assert!(is_folios_copy(&documents));

        assert!(remove_from(&documents).unwrap());
        assert!(!root.exists());
        assert!(
            documents.join(MODULE_RELATIVE_PATH).exists(),
            "only the version's own directory goes; the PSReadLine folder may \
             hold other versions this build has no business deleting"
        );
        std::fs::remove_dir_all(&documents).unwrap();
    }

    /// PIN — **a module that is not this build's copy is never deleted.**
    ///
    /// The one destructive path in the feature, and the guard is content rather
    /// than a note Folio left behind. Somebody's own 2.4.6 — a different build,
    /// a hand-edited `psm1`, a Microsoft release that reaches this number — must
    /// survive `Off` untouched.
    ///
    /// MUTATION: compare only the manifest and the edited `psm1` below is
    /// deleted with everything else.
    #[test]
    fn a_module_this_build_did_not_write_survives_a_removal() {
        let documents = temp_dir("foreign");
        let root = install_into(&documents).unwrap();
        // One byte of one file, changed the way a user editing their own module
        // would change it.
        let psm1 = root.join("PSReadLine.psm1");
        let mut text = std::fs::read(&psm1).unwrap();
        text.extend_from_slice(b"\n# my own edit\n");
        std::fs::write(&psm1, &text).unwrap();

        assert!(
            !is_folios_copy(&documents),
            "an edited file makes the directory somebody else's"
        );
        assert!(!remove_from(&documents).unwrap());
        assert!(psm1.exists(), "and nothing in it was deleted");
        assert_eq!(std::fs::read(&psm1).unwrap(), text);

        // A directory that is merely incomplete is equally not Folio's.
        std::fs::remove_file(root.join("License.txt")).unwrap();
        assert!(!is_folios_copy(&documents));
        assert!(!remove_from(&documents).unwrap());
        std::fs::remove_dir_all(&documents).unwrap();
    }

    /// PIN — installing over an interrupted install repairs it.
    #[test]
    fn a_second_install_repairs_a_half_written_one() {
        let documents = temp_dir("repair");
        install_into(&documents).unwrap();
        let root = module_directory(&documents);
        std::fs::write(root.join("PSReadLine.psd1"), b"truncated").unwrap();
        std::fs::remove_file(root.join("netstd/Microsoft.PowerShell.PSReadLine.Polyfiller.dll"))
            .unwrap();
        assert!(!is_folios_copy(&documents));

        install_into(&documents).unwrap();
        assert!(is_folios_copy(&documents));
        std::fs::remove_dir_all(&documents).unwrap();
    }

    /// PIN — the diagnostics door reads a version and a policy, and defaults the
    /// policy to a permissive one so a bare version is usable.
    #[test]
    fn the_probe_override_reads_a_version_and_an_optional_policy() {
        // Parsed through the same code the env var feeds, without touching the
        // process environment (which other tests share).
        let parse = |raw: &str| {
            let mut parts = raw.split(',');
            let version = Version::parse(parts.next().unwrap());
            let policy = parts
                .next()
                .map_or(ExecutionPolicy::RemoteSigned, ExecutionPolicy::parse);
            Probe { version, policy }
        };
        assert_eq!(
            parse("2.0.0"),
            Probe {
                version: Version::parse("2.0.0"),
                policy: ExecutionPolicy::RemoteSigned
            }
        );
        assert_eq!(
            parse("2.0.0,AllSigned"),
            Probe {
                version: Version::parse("2.0.0"),
                policy: ExecutionPolicy::AllSigned
            }
        );
    }

    /// PIN (§7.1.6c-3c) — **the row's cached line is cached per language.**
    ///
    /// Red before this slice: `row_description` held one `OnceLock` per state,
    /// filled from whichever language happened to ask first, and nothing could
    /// ask it for the other column at all. It is the only process-lifetime
    /// string cache in this app built out of `crate::i18n`'s table, so it is the
    /// only one a hot language switch could have left standing with the wrong
    /// words in it — on the very page the switch is made from.
    ///
    /// Every state is walked, including the two that are not cached, because
    /// what is being pinned is the *answer* and not the storage: a state that
    /// stopped being cached would still have to change language.
    ///
    /// MUTATION: index the slots with `0` instead of `lang.index()` and this
    /// fails on the first state whose two columns then come back equal.
    #[test]
    fn no_line_this_row_has_cached_survives_a_language_switch() {
        for state in [
            RowState::Probing,
            RowState::RemovedElsewhere,
            RowState::Outdated,
            RowState::InstalledByFolio,
            RowState::AlreadyCurrent,
        ] {
            let english = row_description_in(state, i18n::Lang::English);
            let chinese = row_description_in(state, i18n::Lang::Chinese);
            assert_ne!(
                english, chinese,
                "{state:?} says the same thing in both languages"
            );
            assert!(
                chinese
                    .chars()
                    .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
                "{state:?} reads {chinese:?} in Chinese, which has no Chinese in it"
            );
            // Asked a second time, which is the half that reads the cache rather
            // than filling it: a slot shared between the two languages answers
            // the first caller's words to the second.
            assert_eq!(row_description_in(state, i18n::Lang::English), english);
            assert_eq!(row_description_in(state, i18n::Lang::Chinese), chinese);
        }
    }

    /// PIN — the ambient entry point is the named one asked for whatever is in
    /// force, and nothing else.
    #[test]
    fn the_rows_line_is_the_named_line_in_the_language_in_force() {
        for state in [RowState::Probing, RowState::AlreadyCurrent] {
            assert_eq!(
                row_description(state),
                row_description_in(state, i18n::current())
            );
        }
    }
}
