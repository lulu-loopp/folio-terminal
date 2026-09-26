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

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::i18n::{self, Text};

/// The module version this build carries and installs.
///
/// **The only place this number is written on the Rust side.** See the module
/// header; `folio.ps1` holds the other half and a test binds them together.
pub const PATCHED_VERSION: &str = "2.4.6";

/// **The build stamp inside the module this build carries**, and the whole of
/// how one Folio-patched PSReadLine is told from another.
///
/// `PATCHED_VERSION` is the module's `ModuleVersion` — the number PowerShell
/// resolves, the number `folio.ps1` gates on, and a number that has been `2.4.6`
/// for every `-bt` build this product has ever shipped. The *patch* is Folio's
/// own (see 33d9ec9: `2.4.6-bt.anchorfix` seeded its resize anchor from a column
/// the starting width had already reduced, and `2.4.6-bt.2` takes it from where
/// the console says the cursor is), and the only place its identity is written
/// down is the `ProductVersion` string in the DLL's Win32 version resource.
///
/// So this is the number the *upgrade door* reads. Pinned to the shipped bytes
/// by `the_bundled_module_carries_the_build_this_file_names`, exactly as
/// `PATCHED_VERSION` is pinned to `folio.ps1`: a literal that agrees today and
/// diverges at the next patch is the one shape this file has already refused
/// once.
pub const PATCHED_BUILD: &str = "2.4.6-bt.2";

/// What every Folio-patched build's `ProductVersion` begins with.
///
/// Derived rather than written, so that the day `PATCHED_VERSION` moves to
/// `2.5.0` the family moves with it and a `2.4.6-bt.*` left on disk stops being
/// recognised as this build's family — which is correct: it would then be a
/// module for a version this build no longer patches, and the row would offer to
/// replace it exactly as it offers to replace a stock 2.0.0.
#[must_use]
pub fn family_prefix() -> String {
    format!("{PATCHED_VERSION}-bt.")
}

/// The file whose version resource carries the build stamp.
///
/// The patched assembly and not the manifest beside it: the manifest says
/// `ModuleVersion = '2.4.6'` in every bundle this product has shipped, so it
/// cannot tell two of them apart, and it is not where the patch lives anyway.
const BUILD_STAMP_FILE: &str = "Microsoft.PowerShell.PSReadLine.dll";

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
/// [`Text::PsReadLineProbing`] forever, because the row was drawn by a probe
/// that had never been started. Calling it again is free.
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
    // In the workers' band: this starts a PowerShell to ask a question about a
    // module, and it must never be the reason a frame was late.
    bt_platform::spawn_at_priority(
        "psreadline-probe",
        bt_platform::ThreadPriority::BelowNormal,
        |_ctx| {
            let _ = PROBE.set(run_probe());
            // After the answer is published, never before: a wake that raced the
            // `set` would send the loop to read a row that is still `Probing`,
            // and there is no second wake coming.
            if let Some(wake) = WAKE.get() {
                wake();
            }
        },
    )
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
    // Through the quiet door (§7.40 ①): without `CREATE_NO_WINDOW` a console
    // window opens on screen every time a PowerShell pane is opened for the
    // first time in a session.
    //
    // **By name and therefore by absolute path** (R1-17): `CreateProcess` reads
    // the process's working directory before it reads `PATH`, and a
    // `powershell.exe` left in a folder somebody cloned is not the PowerShell
    // this probe is asking about. `quiet_command_named` looks where a program
    // is supposed to live and nowhere else; nothing found there means no
    // answer, never a bare name to fall back on.
    let Some(mut command) =
        bt_platform::quiet_command_named(std::path::Path::new("powershell.exe"))
    else {
        return Probe::default();
    };
    let output = command
        .args(["-NoProfile", "-NonInteractive", "-Command", PROBE_COMMAND])
        .output()
        .inspect(|output| {
            bt_platform::file_reads::pipe_output(bt_platform::file_reads::Lane::Settings, output)
        });
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
    /// **A Folio build wrote the module and it was not this one** (user ruling
    /// 2026-08-18).
    ///
    /// The hole this closes: `is_folios_copy` recognised only the bytes of the
    /// build asking, so a module an *older* Folio installed answered "not mine"
    /// to both halves of the row. `Off` was dark because the guard would not
    /// delete it, and `On` was dark because the probe reported `2.4.6` and
    /// `already_current` was true — the module PowerShell was loading could
    /// therefore be neither removed nor replaced from the one row in the product
    /// that exists to manage it, and the reader was left to do it by hand in
    /// `Documents`.
    UpdateAvailable,
    /// The machine's own module is already new enough.
    AlreadyCurrent,
    /// `settings.json` says Folio installed it and it is not there any more.
    RemovedElsewhere,
    /// **A module somebody else wrote is standing in the directory Folio
    /// installs into** (audit 3, E-1).
    ///
    /// The hole this closes: `installed_copy` answered `None` for *"nothing is
    /// there"* and for *"somebody else's module is there"* alike, so the row
    /// read an occupied directory as an empty one and offered `On` over it —
    /// from `Outdated` whenever the one-shot probe was stale or blind, and from
    /// `RemovedElsewhere` with no race at all. Both verbs are dark here: this is
    /// not Folio's module to replace and not Folio's to delete.
    NotOurs,
}

/// Who owns the module directory under a Documents root.
///
/// Four answers and not a `bool`, because each of the middle two is a ruling.
/// `OlderBuild` is the 2026-08-18 one: "there is a module here and Folio's
/// family wrote it, but not this build" is a different sentence from either
/// "this build wrote it" or "nothing of ours is here", and a caller handed a
/// `bool` has to pick one of the two to lie with.
///
/// [`Self::Foreign`] is audit 3's (E-1) and it is the same mistake one step
/// further out: until it existed, "nothing is there" and "somebody else's module
/// is there" were both [`Self::None`], so the *install* door could not tell an
/// empty directory from an occupied one and wrote over a module PowerShellGet
/// had put in the reader's own `Documents`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InstalledCopy {
    /// **The place is Folio's to write** — nothing is there, or what is there is
    /// this build's own work in a state `is_folios_copy` will not vouch for:
    /// an interrupted write, or an edit somebody made to Folio's own files.
    #[default]
    None,
    /// Byte for byte, or build for build, what this executable carries.
    ThisBuild,
    /// A `2.4.6-bt.*` that is not this one — an older Folio's work.
    OlderBuild,
    /// **A module Folio did not write.** Neither verb on the row touches it.
    Foreign,
}

/// Reconcile the probe with the stored state.
///
/// `installed` is a filesystem question the caller answers, because this
/// function is pure and the answer changes under it: the user may install or
/// remove during the session, long after the probe's one reading.
#[must_use]
pub fn row_state(
    probe: Option<Probe>,
    invite: bt_persist::PsReadLineInviteV1,
    installed: InstalledCopy,
) -> RowState {
    match installed {
        InstalledCopy::ThisBuild => return RowState::InstalledByFolio,
        // **Ahead of the probe deliberately.** The probe reports the module's
        // `ModuleVersion`, which every `-bt` build says is `2.4.6`, so
        // `already_current` is true of an older Folio copy and would file it
        // under `AlreadyCurrent` — the row telling a reader their module is
        // fine while shipping a newer repair for it.
        InstalledCopy::OlderBuild => return RowState::UpdateAvailable,
        // **Ahead of the stored state as well as the probe** (audit 3, E-1).
        // `RemovedElsewhere` used to be answered before the disk was consulted
        // at all, so a reader whose invitation said `Installed` was offered `On`
        // over a gallery module however fresh the reading of the machine was.
        // What is on the disk is a fact about the disk; what `settings.json`
        // remembers is a fact about this reader — and the first one decides
        // whether the directory may be written to.
        InstalledCopy::Foreign => return RowState::NotOurs,
        InstalledCopy::None => {}
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
    static UPDATE: [OnceLock<String>; i18n::Lang::COUNT] = [OnceLock::new(), OnceLock::new()];
    let slot = lang.index();
    match state {
        RowState::Probing => Text::PsReadLineProbing.in_lang(lang),
        RowState::RemovedElsewhere => Text::PsReadLineRowGone.in_lang(lang),
        // **Not `AlreadyCurrent`'s sentence, though the directory is a 2.4.6
        // leaf.** That line names what the *probe* found, and the machines this
        // state exists for are exactly the ones whose probe is stale or blind —
        // it would tell a reader their PSReadLine is 2.0.0 while a 2.4.6 sits
        // in front of the row. What is true here is about ownership, not about
        // a version, and it takes no runtime value, so it is an entry rather
        // than a cached line.
        RowState::NotOurs => Text::PsReadLineRowNotOurs.in_lang(lang),
        // **Two sentences, because there are two situations** (§7.47): a module
        // that is merely old, and a module that is old on a machine whose
        // execution policy will not take the replacement. The second reader's
        // question is not "what have I got" but "why is the switch dark", and
        // until 2026-08-29 the only surface that answered it was the invitation
        // — which is gone the moment it is answered. The `OnceLock` is as sound
        // for this as for the others: the probe is a one-shot, so the policy
        // cannot move under the cache.
        RowState::Outdated => OUTDATED[slot]
            .get_or_init(|| outdated_line(lang, probe().unwrap_or_default()))
            .as_str(),
        RowState::InstalledByFolio => INSTALLED[slot]
            .get_or_init(|| i18n::psreadline_row_installed_in(lang, PATCHED_VERSION))
            .as_str(),
        // **The one sentence in this row that names two builds**, because the
        // reader's question here is not "what have I got" but "what would
        // pressing On change". The `OnceLock` is sound for the reason the three
        // above it are: the installed build is read once per state, and reaching
        // this state at all means the directory held that build when the row was
        // last refreshed.
        RowState::UpdateAvailable => UPDATE[slot]
            .get_or_init(|| {
                i18n::psreadline_row_update_in(
                    lang,
                    installed_build_text().as_deref().unwrap_or(PATCHED_VERSION),
                    PATCHED_BUILD,
                )
            })
            .as_str(),
        RowState::AlreadyCurrent => CURRENT[slot]
            .get_or_init(|| {
                i18n::psreadline_row_current_in(lang, &probe().unwrap_or_default().found_text())
            })
            .as_str(),
    }
}

/// What [`RowState::Outdated`] says, which is two sentences and not one
/// (§7.47).
///
/// Split out of the cache so it can be asked about a probe the process does not
/// have: the answer is cached for the life of the run, so a test that could
/// only reach it through [`row_description_in`] could only ever see the machine
/// it is running on — and the machine that has to be described is the one whose
/// execution policy refuses the module.
#[must_use]
pub fn outdated_line(lang: i18n::Lang, probe: Probe) -> String {
    if probe.policy.refuses_unsigned_modules() {
        i18n::psreadline_row_blocked_in(lang, &probe.found_text(), probe.policy.name())
    } else {
        i18n::psreadline_row_outdated_in(lang, &probe.found_text())
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
        && matches!(
            state,
            // **The update is the same verb**, and that is the ruling's own
            // wording: On means "have Folio's module", and turning it on over an
            // older Folio module writes this build's files into the same
            // directory. A third item would have been a second way to say the
            // one thing this row says.
            RowState::Outdated | RowState::RemovedElsewhere | RowState::UpdateAvailable
        )
}

/// Whether the row's `Off` item can be chosen.
///
/// **Only ever removes what Folio wrote.** A machine whose own PSReadLine is
/// newer, or older, is not this row's to touch — the `Off` item is dark there,
/// which is the same sentence the default-profile picker's greyed rows speak.
#[must_use]
pub fn remove_available(state: RowState) -> bool {
    // A module an older Folio wrote is still a module Folio wrote, and the
    // reader who wants it gone must not have to find `Documents` to do it. What
    // guards the delete is the `-bt` stamp — a string only this project's own
    // builds put in that file — and never a version number a stock module also
    // carries. See [`installed_copy`].
    matches!(
        state,
        RowState::InstalledByFolio | RowState::UpdateAvailable
    )
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

/// What a write into the module directory did.
///
/// A second answer beside the path, and not an `io::Error`, because a directory
/// that belongs to somebody else is not a failure of the machine's: nothing went
/// wrong, and the sentence the reader is owed names an owner rather than quoting
/// Windows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Wrote {
    /// The nine files are on disk, under this root.
    Module(PathBuf),
    /// The directory holds a module Folio did not write. **Nothing was
    /// written.**
    NotOurs,
}

/// Write the nine bundled files into `documents`, and record the root first.
///
/// Takes a Documents root rather than reading one, so a test can install into a
/// temporary directory and read back exactly what a real install would write.
///
/// The record is taken **before** the first byte lands and inside the same
/// occupancy check as the write, so an install interrupted halfway still leaves
/// the cleanup door something to find, and a directory that is refused leaves no
/// claim on a path Folio never wrote to.
pub fn install_recorded(documents: &Path, data: &Path) -> io::Result<Wrote> {
    use crate::shell_integration::profile_marks::{self, Marks};
    install_checked(documents, |root| {
        if !root.is_absolute() {
            return Err(io::Error::other(crate::i18n::Text::ShellMarksPath.text()));
        }
        // One of Folio's own writers: this is a press on the invitation or on
        // the Settings row, so it stands in the queue rather than reporting one.
        let lock = profile_marks::lock(data, profile_marks::Asker::InApp)?;
        let mut marks = Marks::read(data)?;
        if !marks.psreadline_module_roots.iter().any(|it| it == root) {
            marks.psreadline_module_roots.push(root.to_owned());
        }
        marks.write(data)?;
        // Held across the write, as it was when this function did both halves
        // itself.
        Ok(lock)
    })
}

/// Historical roots must have exactly the shape this writer owns.
pub fn documents_for_module_root(root: &Path) -> Option<PathBuf> {
    let documents = root
        .ancestors()
        .find(|parent| module_directory(parent) == root)?;
    Some(documents.to_owned())
}

/// Write the nine bundled files into `documents`, recording nothing.
///
/// Creates the directories it needs and overwrites what is there, which is the
/// right behaviour for the cases that reach it: a previous install of Folio's
/// that was interrupted, an older Folio build being updated, or a file of
/// Folio's that was edited. It never touches a *different* version's directory —
/// `2.4.6` is a leaf of its own, which is how PowerShell's module path is
/// organised and why a per-version directory is the unit here.
///
/// **Tests only**, as [`apply`] is and for its reason: the product's one road to
/// the disk records the root it is about to write to, and a second road that
/// skipped the record would be a module the cleanup door cannot find.
#[cfg(test)]
pub fn install_into(documents: &Path) -> io::Result<Wrote> {
    install_checked(documents, |_| Ok(()))
}

/// **The only writer, and the occupancy check is inside it** (audit 3, E-1).
///
/// This is [`remove_from`]'s own rule turned round, in the words that function's
/// doc comment has carried since the feature was written: *a delete guarded from
/// outside is a delete that the next caller performs unguarded* — and so is a
/// write. Until this check existed the whole guard was a [`RowState`] the caller
/// computed, out of a probe read once per process, and two of that table's
/// states let the write through over a module PowerShellGet had installed in the
/// reader's own `Documents`: `Outdated` whenever the probe was stale or blind,
/// and `RemovedElsewhere` with no race at all.
///
/// The disk is read **here**, at the moment of the write. That is an edge — a
/// press — and not a poll, so it owes nothing to the clock-run budget.
///
/// `before` is what the caller wants done between the check passing and the
/// first byte landing, and its return value is held until the write is over:
/// [`install_recorded`] takes the marks lock there.
fn install_checked<G>(
    documents: &Path,
    before: impl FnOnce(&Path) -> io::Result<G>,
) -> io::Result<Wrote> {
    let root = module_directory(documents);
    if installed_copy(documents) == InstalledCopy::Foreign {
        return Ok(Wrote::NotOurs);
    }
    let _held = before(&root)?;
    // **The build stamp lands last** (ticket 56). The stamp is the whole of how
    // one Folio build is told from another, so a write that stops part-way —
    // the usual cause is a PowerShell that has another of these DLLs loaded,
    // which Windows will not let anybody overwrite — must leave the stamp it
    // found. Written in array order, a stop after the assembly and before a
    // later file left this build's stamp over a mix of two builds' bytes, which
    // `installed_copy` reads as an edit (`None`): no longer an older build, so
    // never replaced again, and a row reading "removed elsewhere" over a module
    // PowerShell still loads. Last, the old stamp stands until every other file
    // is in, so the copy still reads as the older build and the next launch
    // tries again.
    let stamp_last = BUNDLED_FILES
        .iter()
        .filter(|(name, _)| *name != BUILD_STAMP_FILE)
        .chain(
            BUNDLED_FILES
                .iter()
                .filter(|(name, _)| *name == BUILD_STAMP_FILE),
        );
    for (name, bytes) in stamp_last {
        let path = root.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes)?;
    }
    Ok(Wrote::Module(root))
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
///
/// **Tests only.** The product asks [`installed_copy`], which needs the same
/// comparison with the `is_dir` already answered and calls `is_folios_copy_at`
/// directly; this is that question asked from a Documents root.
#[cfg(test)]
#[must_use]
pub fn is_folios_copy(documents: &Path) -> bool {
    let root = module_directory(documents);
    installed_disk::is_dir(&root) && is_folios_copy_at(&root)
}

/// Whether the module directory holds this build's copy, byte for byte, with
/// the directory question already answered.
///
/// Split so that [`installed_copy`] asks `is_dir` exactly once for the whole of
/// its decision — the clock-run budget counts that call, and a second one would
/// be a second reading of a fact this function was handed.
fn is_folios_copy_at(root: &Path) -> bool {
    BUNDLED_FILES.iter().all(|(name, bytes)| {
        installed_disk::read(&root.join(name)).is_ok_and(|found| found.as_slice() == *bytes)
    })
}

/// The bytes this build ships under one of the [`BUNDLED_FILES`] names.
fn bundled(name: &str) -> &'static [u8] {
    BUNDLED_FILES
        .iter()
        .find(|(bundled, _)| *bundled == name)
        .expect("a name out of BUNDLED_FILES")
        .1
}

/// The `ProductVersion` stamped into the module installed under `documents`, if
/// there is one and it belongs to **Folio's own patch family**.
///
/// `None` for a directory that is not there, a file with no version resource,
/// and — the case that matters — a perfectly good stock `2.4.6` somebody
/// installed from the gallery. Only this project's builds put `-bt.` in that
/// string, so the prefix is a claim about *who wrote the file* and not about how
/// new it is, which is exactly the claim [`remove_from`] needs before it deletes
/// anything.
///
/// Read from the DLL's Win32 version resource rather than from the `.psd1`
/// beside it. The manifest carries `ModuleVersion = '2.4.6'` in every `-bt`
/// bundle ever shipped, so it cannot tell two of them apart; the version
/// resource is where the patch's own identity is stamped, and it is already
/// there in every copy an older Folio wrote — which a marker file invented today
/// could never be.
#[must_use]
pub fn installed_build(documents: &Path) -> Option<String> {
    let stamp = module_directory(documents).join(BUILD_STAMP_FILE);
    let build = installed_disk::product_version(&stamp)?;
    build.starts_with(&family_prefix()).then_some(build)
}

/// Which copy of Folio's module is under `documents`.
///
/// **Byte identity first, and it stays the rule for "did Folio write exactly
/// this".** It is the strongest answer available and it costs one read of files
/// that are already in the page cache. The family stamp is the fallback and
/// answers a strictly weaker question — "did some build of Folio's patch write
/// this" — which is the only question a copy from an older release can answer at
/// all, and it is enough for both things the row does with it: replacing a
/// module of ours, and deleting one.
///
/// **A copy stamped with this build but not byte-identical to it is `None`**,
/// which is `a_module_this_build_did_not_write_survives_a_removal` still holding
/// its ground: an edited `psm1` beside our own DLL is somebody's own module now,
/// and the strongest answer available about it is available, so the weaker one
/// does not get to overrule it. The stamp is consulted only where byte identity
/// *cannot* answer - a build whose bytes this executable does not carry - and
/// there it is the only claim anybody can make.
#[must_use]
pub fn installed_copy(documents: &Path) -> InstalledCopy {
    let root = module_directory(documents);
    if !installed_disk::is_dir(&root) {
        return InstalledCopy::None;
    }
    if is_folios_copy_at(&root) {
        return InstalledCopy::ThisBuild;
    }
    match installed_build(documents) {
        // Our own family, our own build number, and bytes that are not ours:
        // this directory has been edited since Folio wrote it. See above.
        Some(build) if build == PATCHED_BUILD => InstalledCopy::None,
        Some(_) => InstalledCopy::OlderBuild,
        None => unclaimed_or_foreign(&root),
    }
}

/// Whether a directory that carries no `-bt` stamp is **free** or **somebody
/// else's** (audit 3, E-1).
///
/// Reached only when the two claims above have failed, so what is known on the
/// way in is: the leaf exists, its nine files are not byte for byte this
/// build's, and nothing in it carries Folio's family stamp.
///
/// **The assembly decides, and the rest of the leaf only when there is none.**
/// PSReadLine is a binary module: `PSReadLine.psd1` names
/// `Microsoft.PowerShell.PSReadLine.dll` as its `RootModule`, so whoever wrote
/// that file wrote the module PowerShell loads, and the files beside it are
/// bookkeeping. That is what makes the *mixed* leaf decidable — Folio's own
/// assembly with PowerShellGet's `PSGetModuleInfo.xml`, `en-US\` and catalog
/// still standing beside it, which is the damaged state a Folio before this fix
/// left on real machines. The module there is Folio's, so the directory is
/// Folio's to update; the sidecars are not Folio's to delete, and
/// [`remove_from`] leaves them.
///
/// **Byte identity and not the stamp**, which the caller has already tried: a
/// host with no version resources to read — every non-Windows build of this
/// crate — must answer the same question the same way, and the bytes are
/// readable everywhere.
///
/// With no assembly at all there is no module of anybody's here, so the names
/// answer: a leaf holding nothing but the names this build writes is a write of
/// Folio's that was interrupted before the assembly landed, and anything else in
/// it belongs to somebody.
fn unclaimed_or_foreign(root: &Path) -> InstalledCopy {
    let assembly = root.join(BUILD_STAMP_FILE);
    match installed_disk::read(&assembly) {
        Ok(found) if found.as_slice() == bundled(BUILD_STAMP_FILE) => InstalledCopy::None,
        Ok(_) => InstalledCopy::Foreign,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if nothing_but_our_names(root) {
                InstalledCopy::None
            } else {
                InstalledCopy::Foreign
            }
        }
        // A leaf whose assembly cannot be read is a leaf whose owner cannot be
        // established, and an unestablished owner is never Folio.
        Err(_) => InstalledCopy::Foreign,
    }
}

/// Whether every entry under `root` is one this build writes.
///
/// Walks the leaf rather than sampling it, and the walk is bounded by the rule
/// itself: the only directories [`BUNDLED_FILES`] puts anything in are
/// `net6plus` and `netstd`, so any other directory is answered `false` where it
/// is found rather than descended into.
fn nothing_but_our_names(root: &Path) -> bool {
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        let Ok(entries) = installed_disk::entries(&root.join(&relative)) else {
            return false;
        };
        for (name, is_directory) in entries {
            let child = relative.join(&name);
            let ours = BUNDLED_FILES.iter().any(|(bundled, _)| {
                let bundled = Path::new(bundled);
                if is_directory {
                    bundled.starts_with(&child) && bundled != child
                } else {
                    bundled == child
                }
            });
            if !ours {
                return false;
            }
            if is_directory {
                pending.push(child);
            }
        }
    }
    true
}

/// Read the shared App fact after a module write or a Terminal-page visit.
pub fn refresh_installed(cache: &mut Option<InstalledCopy>, documents: Option<&Path>) {
    *cache = Some(documents.map_or(InstalledCopy::None, installed_copy));
}

/// The headless part of the clock-run invite check. `None` means unread, not
/// "no module"; all windows borrow the same App-owned slot.
pub fn installed_on_probe(
    cache: &mut Option<InstalledCopy>,
    documents: Option<&Path>,
    probe: Option<Probe>,
) -> InstalledCopy {
    if cache.is_none() && probe.is_some() {
        refresh_installed(cache, documents);
    }
    cache.unwrap_or_default()
}

/// The installed-module IO door. Tests replace all four operations on this
/// thread; neither a real account's module nor its version resource is touched.
mod installed_disk {
    use super::*;

    pub(super) trait Disk {
        fn is_dir(&self, path: &Path) -> bool;
        fn read(&self, path: &Path) -> std::io::Result<Vec<u8>>;
        fn product_version(&self, path: &Path) -> Option<String>;
        /// What is directly in `directory`, and which of it is itself a
        /// directory. **Names, not content** — no file is opened, so this door
        /// carries no lane: the read ledger counts bytes of file content and
        /// there are none here.
        fn entries(&self, directory: &Path) -> std::io::Result<Vec<(OsString, bool)>>;
    }

    struct System;

    impl Disk for System {
        fn is_dir(&self, path: &Path) -> bool {
            path.is_dir()
        }
        fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
            // Through the read ledger: this is the door that ran away on
            // 2026-09-20, and the tripwire has to be able to see it.
            bt_platform::file_reads::read(bt_platform::file_reads::Lane::Settings, path)
        }
        fn product_version(&self, path: &Path) -> Option<String> {
            file_product_version(path)
        }
        fn entries(&self, directory: &Path) -> std::io::Result<Vec<(OsString, bool)>> {
            let mut entries = Vec::new();
            for entry in std::fs::read_dir(directory)? {
                let entry = entry?;
                let is_directory = entry.file_type()?.is_dir();
                entries.push((entry.file_name(), is_directory));
            }
            Ok(entries)
        }
    }

    #[cfg(test)]
    thread_local! {
        pub(super) static OVERRIDE: std::cell::RefCell<Option<std::rc::Rc<dyn Disk>>> =
            const { std::cell::RefCell::new(None) };
    }

    fn with<R>(f: impl FnOnce(&dyn Disk) -> R) -> R {
        #[cfg(test)]
        if let Some(disk) = OVERRIDE.with(|slot| slot.borrow().clone()) {
            return f(disk.as_ref());
        }
        f(&System)
    }

    pub(super) fn is_dir(path: &Path) -> bool {
        with(|disk| disk.is_dir(path))
    }
    pub(super) fn read(path: &Path) -> std::io::Result<Vec<u8>> {
        with(|disk| disk.read(path))
    }
    pub(super) fn product_version(path: &Path) -> Option<String> {
        with(|disk| disk.product_version(path))
    }
    pub(super) fn entries(directory: &Path) -> std::io::Result<Vec<(OsString, bool)>> {
        with(|disk| disk.entries(directory))
    }
}

/// The installed build's stamp as the row's sentence wants it.
fn installed_build_text() -> Option<String> {
    installed_build(&documents_directory()?)
}

/// A file's `ProductVersion`, asked of Windows — and `None` everywhere else.
///
/// The `cfg` is `documents_directory`'s, for `documents_directory`'s reason:
/// this crate builds on hosts that have no version resources at all, and a
/// machine that cannot be asked has no Folio module on it either.
fn file_product_version(path: &Path) -> Option<String> {
    #[cfg(windows)]
    {
        bt_platform::file_product_version(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

/// Delete the module, and only if [`installed_copy`] says a Folio build wrote
/// it.
///
/// The check is inside rather than at the call site, because a delete guarded
/// from outside is a delete that the next caller performs unguarded. `Ok(false)`
/// means the directory was left alone — there was nothing there, or what was
/// there was somebody else's.
///
/// **Widened from byte identity on 2026-08-18**, and only as far as the ruling
/// asked. What may be deleted is a directory whose module carries Folio's own
/// `-bt` build stamp; a stock `2.4.6` from the gallery, a fork with its own
/// stamp, and a directory holding somebody's hand-edited module all still say no
/// — they have no `2.4.6-bt.` in them. The old guard could not delete what an
/// older Folio had written, which left the reader with a module this product had
/// put on their machine and would not take off it.
///
/// **Narrowed from the directory to the files on 2026-09-20** (audit 3, E-1).
/// It used to `remove_dir_all` the version leaf once the guard said yes, which
/// answered the wrong question: the guard establishes that the *module* is
/// Folio's, and it says nothing about what stands beside it. On a machine an
/// older Folio had already written over, what stood beside it was
/// PowerShellGet's `PSGetModuleInfo.xml`, `en-US\` and catalog — the gallery's
/// own record of the module Folio replaced — and `Off` took those too. Now the
/// nine names go, and the directories go only while they are empty.
pub fn remove_from(documents: &Path) -> io::Result<Removed> {
    let root = module_directory(documents);
    match installed_copy(documents) {
        InstalledCopy::None => return Ok(Removed::Nothing),
        InstalledCopy::Foreign => return Ok(Removed::NotOurs),
        InstalledCopy::ThisBuild | InstalledCopy::OlderBuild => {}
    }
    for (name, _) in BUNDLED_FILES {
        match std::fs::remove_file(root.join(name)) {
            Ok(()) => {}
            // An interrupted install leaves some of the nine missing, and a
            // removal that refused over one absent file would be a module this
            // product could not take off a machine it half-wrote.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    // Deepest first, and `remove_dir` rather than `remove_dir_all`: each of
    // these goes only if the nine files were the whole of what was in it.
    for name in ["net6plus", "netstd"] {
        let _ = std::fs::remove_dir(root.join(name));
    }
    if std::fs::remove_dir(&root).is_ok() {
        return Ok(Removed::Took { left: Vec::new() });
    }
    let left = installed_disk::entries(&root).map_or_else(
        |_| Vec::new(),
        |entries| {
            entries
                .into_iter()
                .map(|(name, _)| root.join(name))
                .collect()
        },
    );
    Ok(Removed::Took { left })
}

/// What [`remove_from`] did to the module directory.
///
/// Three answers and not `bool`, because the door that runs unattended
/// (`folio --uninstall-cleanup`) prints one line per mark and each of these is a
/// different line: *removed*, *not present*, and *left standing because it is
/// not Folio's*. The last two were one `Ok(false)` until 2026-09-20, which is
/// how a machine carrying somebody else's PSReadLine reported "not present".
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Removed {
    /// Folio's own files went. `left` is what stood beside them and stayed,
    /// because Folio did not write it — empty when the directory itself went.
    Took { left: Vec<PathBuf> },
    /// Nothing of Folio's was there to take.
    Nothing,
    /// A module Folio did not write. It is exactly where it was.
    NotOurs,
}

// ── Folio's own older copy, replaced at launch (ruling 2026-09-21, option A) ─

/// **One Folio patch build, as the `ProductVersion` stamp in its DLL names it**
/// (ticket 56).
///
/// The upgrade has to know whether a stamp on disk is *older* than
/// [`PATCHED_BUILD`], and until this type there was no order at all: every
/// `-bt.` stamp that was not this build's was [`InstalledCopy::OlderBuild`],
/// whether it came before this build or after it.
///
/// **The order.** Two builds are ordered only when their module version is the
/// same — the family is per [`PATCHED_VERSION`] ([`family_prefix`]), and a
/// build of another version is not an earlier or later patch of this one. Within
/// a version, a numbered build `-bt.N` is ordered by `N` as a number (so `bt.10`
/// comes after `bt.9`, which a text comparison would get backwards). A named
/// build — `-bt.anchorfix`, the one this product shipped before it began to
/// number them (see [`PATCHED_BUILD`]) — comes before every numbered build. Two
/// named builds are not ordered against each other: nothing says which came
/// first, and a stamp nobody can place is kept rather than replaced.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Build {
    version: Version,
    patch: Patch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Patch {
    /// `-bt.<name>`: a build named before the numbering began.
    Named(String),
    /// `-bt.<N>`.
    Numbered(u32),
}

impl Build {
    /// Parse `<version>-bt.<patch>`; `None` for anything without Folio's `-bt.`
    /// mark, which is the whole of what a stock or gallery module's stamp is.
    #[must_use]
    pub fn parse(stamp: &str) -> Option<Self> {
        let (version, patch) = stamp.trim().split_once("-bt.")?;
        let version = Version::parse(version)?;
        let patch = if patch.is_empty() {
            return None;
        } else if patch.bytes().all(|byte| byte.is_ascii_digit()) {
            Patch::Numbered(patch.parse().ok()?)
        } else {
            Patch::Named(patch.to_owned())
        };
        Some(Self { version, patch })
    }

    /// The build this executable carries — [`PATCHED_BUILD`], parsed.
    #[must_use]
    pub fn bundled() -> Self {
        Self::parse(PATCHED_BUILD).expect("PATCHED_BUILD is a literal in this file")
    }

    /// Whether this build came before `other` — the order in the type's doc.
    #[must_use]
    pub fn predates(&self, other: &Self) -> bool {
        self.version == other.version
            && match (&self.patch, &other.patch) {
                (Patch::Numbered(this), Patch::Numbered(that)) => this < that,
                (Patch::Named(_), Patch::Numbered(_)) => true,
                (Patch::Numbered(_) | Patch::Named(_), Patch::Named(_)) => false,
            }
    }

    /// The stamp as the DLL spells it.
    #[must_use]
    pub fn text(&self) -> String {
        match &self.patch {
            Patch::Named(name) => format!("{}-bt.{name}", self.version.text()),
            Patch::Numbered(number) => format!("{}-bt.{number}", self.version.text()),
        }
    }
}

/// What stands in the module directory, as far as the upgrade is concerned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledBuild {
    /// The Folio build stamped in the module; `None` for a module that carries
    /// no Folio stamp — somebody else's.
    pub build: Option<Build>,
    /// Whether Folio's own records say Folio put a module here.
    pub recorded: bool,
}

impl InstalledBuild {
    /// Read what is under `documents`, and whether Folio recorded installing it.
    ///
    /// `None` when [`installed_copy`] finds nothing of anybody's that the
    /// upgrade could replace or keep — the case the invitation exists for. The
    /// copy is classified by [`installed_copy`] and nothing else, so what the
    /// upgrade calls "Folio's older build" is exactly what the Settings row calls
    /// `Update`.
    #[must_use]
    pub fn found(
        documents: &Path,
        data: &Path,
        invite: bt_persist::PsReadLineInviteV1,
    ) -> Option<Self> {
        let build = match installed_copy(documents) {
            InstalledCopy::None => return None,
            InstalledCopy::ThisBuild => Some(Build::bundled()),
            InstalledCopy::OlderBuild => {
                installed_build(documents).as_deref().and_then(Build::parse)
            }
            InstalledCopy::Foreign => None,
        };
        Some(Self {
            build,
            recorded: recorded_install(documents, data, invite),
        })
    }
}

/// **Whether Folio recorded installing the module under `documents`.**
///
/// Two records say so, and either is enough. The marks record
/// (`psreadline_module_roots`, written by [`install_recorded`] before the first
/// byte) names the root, and it exists only since 0.4.3. Every install pressed
/// under 0.1.0–0.4.2 left only the other one — `settings.json` moving the
/// invitation to `Installed`, which `Runtime::apply_psreadline` writes on the
/// same press — and requiring the marks record alone would leave every one of
/// those copies out of the ruling. The replacement then goes through
/// [`install_recorded`], so such a copy leaves the upgrade carrying the marks
/// record too, and the uninstall door can find it.
///
/// A record is not the only guard: [`upgrade_decision`] also asks for Folio's
/// `-bt.` stamp, which only this project's builds put in the DLL.
fn recorded_install(documents: &Path, data: &Path, invite: bt_persist::PsReadLineInviteV1) -> bool {
    use crate::shell_integration::profile_marks::Marks;
    if invite == bt_persist::PsReadLineInviteV1::Installed {
        return true;
    }
    let root = module_directory(documents);
    Marks::read(data).is_ok_and(|marks| marks.psreadline_module_roots.contains(&root))
}

/// What Folio does about the module it finds at launch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Upgrade {
    /// Folio's own older build, which Folio recorded installing: replace it with
    /// the bundled build without asking.
    Replace,
    /// Leave it: this build's own copy, a newer or unplaceable Folio build, a
    /// Folio-stamped copy Folio has no record of installing, or a module
    /// somebody else wrote.
    Keep,
    /// Nothing is there to replace: whether to offer the module is the
    /// invitation's question ([`invite_decision`]), and it is never answered
    /// with a silent install.
    Invite,
}

/// **The upgrade's decision table** (ruling 2026-09-21, option A; ticket 56).
///
/// | on disk | Folio's record | stamp against `bundled` | answer |
/// |---|---|---|---|
/// | nothing Folio could replace | any | — | `Invite` |
/// | a module with no Folio stamp | any | — | `Keep` |
/// | a Folio build | no | any | `Keep` |
/// | a Folio build | yes | older ([`Build::predates`]) | **`Replace`** |
/// | a Folio build | yes | the same, newer, or unordered | `Keep` |
#[must_use]
pub fn upgrade_decision(installed: Option<&InstalledBuild>, bundled: &Build) -> Upgrade {
    let Some(installed) = installed else {
        return Upgrade::Invite;
    };
    match &installed.build {
        Some(build) if installed.recorded && build.predates(bundled) => Upgrade::Replace,
        _ => Upgrade::Keep,
    }
}

/// What the launch's replacement did.
#[derive(Debug)]
pub struct Replacement {
    /// The build that was on disk.
    pub from: String,
    /// What [`install_recorded`] answered.
    pub wrote: io::Result<Wrote>,
}

impl Replacement {
    /// The one `diagnostics.log` line the replacement owes; there is no card.
    ///
    /// A failure is said here and nowhere else, and changes nothing that
    /// matters: the stamp is written last ([`install_checked`]), so the copy
    /// still reads as the older build and the next launch tries again.
    #[must_use]
    pub fn log_line(&self) -> String {
        let from = &self.from;
        match &self.wrote {
            Ok(Wrote::Module(root)) => format!(
                "BT_PSREADLINE upgraded {from} to {PATCHED_BUILD} in {}",
                root.display()
            ),
            Ok(Wrote::NotOurs) => {
                format!("BT_PSREADLINE upgrade from {from} refused why=occupied")
            }
            Err(error) => format!(
                "BT_PSREADLINE upgrade from {from} to {PATCHED_BUILD} failed, \
                 kept until the next launch: {error}"
            ),
        }
    }
}

/// **Replace Folio's own older copy, at launch, without asking** (ruling
/// 2026-09-21, option A; ticket 56).
///
/// `None` when [`upgrade_decision`] says anything but `Replace`: nothing is
/// written and nothing is said. Otherwise the write is the first install's own
/// road, [`install_recorded`] — the same occupancy check, the same marks record
/// taken before the first byte, the same files.
///
/// Called once per process, from the launch, before the first window exists —
/// so before any pane of this Folio has started a PowerShell that would hold
/// the old DLL open.
pub fn upgrade_recorded(
    documents: &Path,
    data: &Path,
    invite: bt_persist::PsReadLineInviteV1,
) -> Option<Replacement> {
    let installed = InstalledBuild::found(documents, data, invite);
    if upgrade_decision(installed.as_ref(), &Build::bundled()) != Upgrade::Replace {
        return None;
    }
    // **Not in an update's trial** (`update_trial`, F-7): the module, its
    // stamp and the marks record are O's until the trial is committed, and the
    // commit asks again.
    if crate::update_trial::defer(crate::update_trial::Writer::PsReadLineUpgrade) {
        return None;
    }
    let from = installed
        .and_then(|installed| installed.build)
        .map(|build| build.text())?;
    Some(Replacement {
        from,
        wrote: install_recorded(documents, data),
    })
}

// ── one door, and it always says something (§7.47) ──────────────────────────

/// Why a press on this row changed nothing.
///
/// **Every variant carries what a reader could act on**, which for five of the
/// six is the path — a person told that an install did not happen and not told
/// where it would have gone has been handed a sentence with nothing in it. The
/// sixth is [`Self::NoDocuments`], and it has no path for the reason it exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// The probe has not answered, so nothing is known about the machine yet.
    StillReading,
    /// Windows' script execution policy would refuse the module at import.
    ///
    /// **Measured rather than assumed** (2026-08-29, clean Windows 10 Pro
    /// 22H2): under `Restricted` the nine files write perfectly well and
    /// `Import-Module PSReadLine` then loads *nothing* — `FormatsToProcess`
    /// names `PSReadLine.format.ps1xml`, a script file, and the policy refuses
    /// it, taking the import down with it. So writing under this policy really
    /// does produce 437 KB no shell will load, and the refusal is right. What
    /// was wrong was that it was silent.
    Policy {
        policy: ExecutionPolicy,
        path: PathBuf,
    },
    /// Windows would not say where this user's `Documents` folder is.
    NoDocuments,
    /// `On` on a machine whose own module is already new enough.
    AlreadyCurrent { found: String, path: PathBuf },
    /// `On` over a directory that already holds this build's module.
    AlreadyThere { path: PathBuf },
    /// **`On` over a module Folio did not write** (audit 3, E-1).
    ///
    /// Its own variant and not [`Self::NotOurs`], which is the removal's word:
    /// the two refusals are about the same ownership and about opposite acts,
    /// and a card that told a reader nothing was *removed* after they pressed
    /// `On` would be the door answering a question nobody asked.
    Occupied { path: PathBuf },
    /// The write failed, and this is what Windows said about it.
    Write { path: PathBuf, message: String },
    /// `Off` over a directory the guard would not touch — somebody else's
    /// module, or nothing at all.
    NotOurs { path: PathBuf },
    /// The delete failed, and this is what Windows said about it.
    Remove { path: PathBuf, message: String },
}

impl Refusal {
    /// The card's sentence, in the language in force.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            Self::StillReading => i18n::psreadline_still_reading(),
            Self::Policy { policy, path } => i18n::psreadline_policy_refused(
                PATCHED_VERSION,
                policy.name(),
                &path.display().to_string(),
            ),
            Self::NoDocuments => i18n::psreadline_no_documents(PATCHED_VERSION),
            Self::AlreadyCurrent { found, path } => {
                i18n::psreadline_already_current(found, &path.display().to_string())
            }
            Self::AlreadyThere { path } => {
                i18n::psreadline_already_there(PATCHED_VERSION, &path.display().to_string())
            }
            Self::Occupied { path } => i18n::psreadline_occupied(&path.display().to_string()),
            Self::Write { path, message } => {
                i18n::psreadline_install_failed(&path.display().to_string(), message)
            }
            Self::NotOurs { path } => i18n::psreadline_not_ours(&path.display().to_string()),
            Self::Remove { path, message } => {
                i18n::psreadline_remove_failed(&path.display().to_string(), message)
            }
        }
    }

    /// The one word `diagnostics.log` files this refusal under.
    ///
    /// A name and not the sentence: the log is read by whoever is holding the
    /// machine open, and it has to be greppable in either language.
    #[must_use]
    pub fn tag(&self) -> &'static str {
        match self {
            Self::StillReading => "still-reading",
            Self::Policy { .. } => "policy",
            Self::NoDocuments => "no-documents",
            Self::AlreadyCurrent { .. } => "already-current",
            Self::AlreadyThere { .. } => "already-there",
            Self::Occupied { .. } => "occupied",
            Self::Write { .. } => "write-failed",
            Self::NotOurs { .. } => "not-ours",
            Self::Remove { .. } => "remove-failed",
        }
    }
}

/// What a press on the row did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    Installed(PathBuf),
    Removed(PathBuf),
    /// Nothing on disk moved, and this is the sentence that says why.
    Refused(Refusal),
}

/// **The only road from a press to the disk, and it never returns in silence**
/// (§7.47).
///
/// Before this function existed the same decision was spread across three
/// places that each had their own way of saying nothing: [`install_available`]
/// greyed the item, `SettingsPanel::hit` answered `Menu` over a greyed item and
/// dropped the press, and the runtime's own `apply_psreadline` returned early
/// on a missing Documents folder. A machine could therefore refuse an install
/// for a good reason and leave no trace of it on any surface — no card, no row
/// text, no line in `diagnostics.log`. That is the 2026-08-29 defect, and one
/// door with no silent exit is the fix.
///
/// `install_available` is still the *drawing's* answer and this is still the
/// *press's*, but they are now two readings of one table rather than two
/// tables — `the_greyed_item_and_the_refusal_agree_on_every_state` pins them
/// together.
#[must_use]
pub fn apply_recorded(
    install: bool,
    documents: Option<&Path>,
    state: RowState,
    probe: Option<Probe>,
    data: &Path,
) -> Outcome {
    apply_with(install, documents, state, probe, |documents| {
        install_recorded(documents, data)
    })
}

#[cfg(test)]
pub fn apply(
    install: bool,
    documents: Option<&Path>,
    state: RowState,
    probe: Option<Probe>,
) -> Outcome {
    apply_with(install, documents, state, probe, install_into)
}

fn apply_with(
    install: bool,
    documents: Option<&Path>,
    state: RowState,
    probe: Option<Probe>,
    writer: impl FnOnce(&Path) -> io::Result<Wrote>,
) -> Outcome {
    let Some(documents) = documents else {
        return Outcome::Refused(Refusal::NoDocuments);
    };
    let path = module_directory(documents);
    if !install {
        // The guard lives inside `remove_from`, so this is a report of what it
        // did rather than a second copy of its rule.
        return match remove_from(documents) {
            Ok(Removed::Took { .. }) => Outcome::Removed(path),
            // One sentence for the two, which is that refusal's own ruling: a
            // reader can act on the path either way and not on the distinction.
            Ok(Removed::Nothing | Removed::NotOurs) => Outcome::Refused(Refusal::NotOurs { path }),
            Err(error) => Outcome::Refused(Refusal::Remove {
                path,
                message: error.to_string(),
            }),
        };
    }
    let Some(probe) = probe else {
        return Outcome::Refused(Refusal::StillReading);
    };
    // **What is already on the machine is asked before the policy is**, because
    // a directory that already holds the module is a truer answer to "why did
    // nothing happen" than a policy that would have stopped a write nobody
    // needed. The order can only matter on a machine whose policy tightened
    // after an install, and there the sentence a reader wants is the one about
    // their own disk.
    match state {
        RowState::InstalledByFolio => return Outcome::Refused(Refusal::AlreadyThere { path }),
        RowState::AlreadyCurrent => {
            return Outcome::Refused(Refusal::AlreadyCurrent {
                found: probe.found_text(),
                path,
            });
        }
        // The drawing's answer for this one is a dark `On`; this is the press's,
        // and the writer refuses it a third time on the disk it reads itself.
        RowState::NotOurs => return Outcome::Refused(Refusal::Occupied { path }),
        // `Probing` cannot be reached with a probe in hand — `row_state` answers
        // it only when there is none — and the other three are the states this
        // verb exists for.
        RowState::Probing
        | RowState::Outdated
        | RowState::RemovedElsewhere
        | RowState::UpdateAvailable => {}
    }
    if probe.policy.refuses_unsigned_modules() {
        return Outcome::Refused(Refusal::Policy {
            policy: probe.policy,
            path,
        });
    }
    match writer(documents) {
        Ok(Wrote::Module(root)) => Outcome::Installed(root),
        Ok(Wrote::NotOurs) => Outcome::Refused(Refusal::Occupied { path }),
        Err(error) => Outcome::Refused(Refusal::Write {
            path,
            message: error.to_string(),
        }),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use bt_persist::PsReadLineInviteV1 as State;

    /// **Stand Folio's own older PSReadLine in `documents`**, as the launch's
    /// upgrade finds it — for a test elsewhere that runs a start's writers
    /// (`update_trial`). `false` where this build has no PSReadLine to stand
    /// (every platform but Windows).
    pub(crate) fn an_older_build_stands_in(documents: &Path) -> bool {
        #[cfg(windows)]
        {
            older_build_in(documents, "2.4.6-bt.1");
            true
        }
        #[cfg(not(windows))]
        {
            let _ = documents;
            false
        }
    }

    struct CountingDisk {
        root: PathBuf,
        reads: std::cell::Cell<usize>,
        bytes: std::cell::Cell<usize>,
        directories: std::cell::Cell<usize>,
        versions: std::cell::Cell<usize>,
        listings: std::cell::Cell<usize>,
    }

    impl installed_disk::Disk for CountingDisk {
        fn is_dir(&self, path: &Path) -> bool {
            assert_eq!(path, self.root);
            self.directories.set(self.directories.get() + 1);
            true
        }
        fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
            assert!(
                BUNDLED_FILES
                    .iter()
                    .any(|(name, _)| path == self.root.join(name))
            );
            self.reads.set(self.reads.get() + 1);
            let bytes = std::fs::read(path)?;
            self.bytes.set(self.bytes.get() + bytes.len());
            Ok(bytes)
        }
        fn product_version(&self, path: &Path) -> Option<String> {
            assert_eq!(path, self.root.join(BUILD_STAMP_FILE));
            self.versions.set(self.versions.get() + 1);
            Some(format!("{}0", family_prefix()))
        }
        fn entries(&self, directory: &Path) -> std::io::Result<Vec<(OsString, bool)>> {
            self.listings.set(self.listings.get() + 1);
            let mut entries = Vec::new();
            for entry in std::fs::read_dir(directory)? {
                let entry = entry?;
                let is_directory = entry.file_type()?.is_dir();
                entries.push((entry.file_name(), is_directory));
            }
            Ok(entries)
        }
    }

    struct DiskOverride;

    impl Drop for DiskOverride {
        fn drop(&mut self) {
            installed_disk::OVERRIDE.with(|slot| *slot.borrow_mut() = None);
        }
    }

    #[test]
    fn psreadline_clock_run_reads_only_at_edges() {
        let documents = temp_dir("clock-edges");
        let root = module_directory(&documents);
        std::fs::create_dir_all(&root).unwrap();
        let fixture = vec![b'x'; 41_345];
        std::fs::write(root.join("Changes.txt"), &fixture).unwrap();
        let disk = std::rc::Rc::new(CountingDisk {
            root,
            reads: Default::default(),
            bytes: Default::default(),
            directories: Default::default(),
            versions: Default::default(),
            listings: Default::default(),
        });
        installed_disk::OVERRIDE.with(|slot| *slot.borrow_mut() = Some(disk.clone()));
        let _override = DiskOverride;
        let probe = Some(probe_at("2.0.0", ExecutionPolicy::RemoteSigned));
        let mut app_fact = None;
        for _ in 0..200 {
            assert_eq!(
                installed_on_probe(&mut app_fact, Some(&documents), None),
                InstalledCopy::None
            );
        }
        assert_eq!(disk.reads.get(), 0);
        // Several windows borrow this one slot; no window owns a disk snapshot.
        for _window in 0..4 {
            for _ in 0..50 {
                let installed = installed_on_probe(&mut app_fact, Some(&documents), probe);
                assert_eq!(installed, InstalledCopy::OlderBuild);
                assert_eq!(
                    row_state(probe, State::NotAsked, installed),
                    RowState::UpdateAvailable
                );
            }
        }
        assert_eq!(
            disk.reads.get(),
            1,
            "200 invite checks must read one Changes.txt"
        );
        assert_eq!(disk.bytes.get(), fixture.len());
        assert_eq!(disk.directories.get(), 1);
        // **The press is an edge and it reads** (audit 3, E-1): `install_into`
        // asks who owns the directory at the moment of the write, which is one
        // module inspection of its own — one `is_dir`, the first bundled file
        // compared, one version stamp. That is the whole cost of the guard and
        // it is charged to a press, never to a turn.
        assert!(matches!(
            apply(true, Some(&documents), RowState::UpdateAvailable, probe),
            Outcome::Installed(_)
        ));
        // Runtime refreshes once after the successful writer. This time every
        // bundled file matches: one module inspection, nine content reads.
        refresh_installed(&mut app_fact, Some(&documents));
        assert_eq!(disk.directories.get(), 3);
        assert_eq!(app_fact, Some(InstalledCopy::ThisBuild));
        assert_eq!(
            row_state(probe, State::Installed, app_fact.unwrap()),
            RowState::InstalledByFolio
        );
        assert_eq!(disk.reads.get(), 2 + BUNDLED_FILES.len());
        // An out-of-band replacement becomes visible on opening Terminal.
        std::fs::write(disk.root.join("Changes.txt"), &fixture).unwrap();
        refresh_installed(&mut app_fact, Some(&documents));
        assert_eq!(disk.directories.get(), 4);
        assert_eq!(app_fact, Some(InstalledCopy::OlderBuild));
        let edge_reads = 3 + BUNDLED_FILES.len();
        let edge_bytes = 3 * fixture.len()
            + BUNDLED_FILES
                .iter()
                .map(|(_, bytes)| bytes.len())
                .sum::<usize>();
        for _ in 0..200 {
            installed_on_probe(&mut app_fact, Some(&documents), probe);
        }
        assert_eq!(disk.reads.get(), edge_reads);
        assert_eq!(disk.directories.get(), 4);
        assert_eq!(disk.versions.get(), 3);
        assert_eq!(disk.bytes.get(), edge_bytes);
        // A failed Documents lookup is a known absence, not an unread fact.
        refresh_installed(&mut app_fact, None);
        assert_eq!(app_fact, Some(InstalledCopy::None));
        for _ in 0..200 {
            assert_eq!(
                installed_on_probe(&mut app_fact, Some(&documents), probe),
                InstalledCopy::None
            );
        }
        assert_eq!(
            disk.reads.get(),
            edge_reads,
            "a failed edge never becomes a retry poll"
        );
        // A failed content read can still identify an older DLL. Cache that
        // result too; the next visit/write is the only reason to ask again.
        std::fs::remove_file(disk.root.join("Changes.txt")).unwrap();
        refresh_installed(&mut app_fact, Some(&documents));
        for _ in 0..200 {
            assert_eq!(
                installed_on_probe(&mut app_fact, Some(&documents), probe),
                InstalledCopy::OlderBuild
            );
        }
        assert_eq!(disk.reads.get(), edge_reads + 1);
        assert_eq!(disk.directories.get(), 5);
        assert_eq!(disk.versions.get(), 4);
        assert_eq!(disk.bytes.get(), edge_bytes);
        assert_eq!(
            disk.listings.get(),
            0,
            "the ownership walk over the leaf's own names is reached only where \
             no assembly at all is there; a module of Folio's family answers \
             before it"
        );
        std::fs::remove_dir_all(documents).unwrap();
    }

    // ── what these two pins ask the crate ─────────────────
    //
    // **P3's deletion commit for this module**
    // (`docs/plans/bt-app-split-prep.md` §6.3, and §6.0 rule 3). The commit
    // before this one read every body twice — once as a slice of a named
    // file, once as the body of an item of this crate — and asserted the two
    // were the same bytes, and did the same for the counts; this one removes
    // the older of the two, because two implementations of one judgement do
    // not vouch for each other (`docs/CONVENTIONS.md` §十 rule 4). The
    // pattern is `main.rs::pty_drain_budget_tests`', not re-derived here.
    //
    // Two readings change shape.
    //
    // * The app's **fact** was a line of `main.rs` matched as text; it is a
    //   field of `App`, so it is asked for as one and the refusal names the
    //   fields the type does carry if it ever stops carrying this one.
    // * The count of the refresh edges widens from `main.rs` to the package,
    //   and is filtered by `in_the_product` — both grains, for the reason
    //   written there. This file compiles into the product and its own
    //   assertions below spell the call five more times; `needle!` excludes one
    //   construction expression rather than every mention.
    fn source_index() -> &'static bt_source::Index {
        bt_source::Index::of_package("bt-app")
    }

    /// The body of `owner::name`, braces included — the identity of §2.4
    /// rather than a line of a file.
    fn method_body(owner: &str, name: &str) -> &'static str {
        source_index()
            .body_of(&bt_source::ItemQuery::method(owner, name))
            .unwrap_or_else(|failure| panic!("{failure}"))
    }

    /// The body of one free function of this crate.
    fn free_fn_body(name: &str) -> &'static str {
        source_index()
            .body_of(&bt_source::ItemQuery::function(name))
            .unwrap_or_else(|failure| panic!("{failure}"))
    }

    /// One search over the whole package, refusing loudly rather than
    /// answering a smaller question.
    fn found(needle: bt_source::Needle, view: bt_source::View) -> bt_source::Found {
        source_index()
            .search(&bt_source::Search::new(needle, view))
            .unwrap_or_else(|failure| panic!("{failure}"))
    }

    /// **How many of these occurrences a product build compiles** —
    /// `bt_source::Found::in_the_product`, which owns that rule and both of the
    /// grains it takes: §2.3's file and §2.4's item.
    ///
    /// This module needs both, which is why it is asked here rather than left
    /// to the file grain: the spellings counted below are written again in this
    /// module's own assertions, and again in whole test files elsewhere in the
    /// package. The rule used to be written out here, in one of twelve copies
    /// of it this crate carried.
    fn in_the_product(found: &bt_source::Found) -> usize {
        found.in_the_product(source_index()).len()
    }

    /// The same count of one raw needle — the view `include_str!` handed this
    /// module.
    fn in_the_product_raw(needle: bt_source::Needle) -> usize {
        in_the_product(&found(needle, bt_source::View::Raw))
    }

    #[test]
    fn psreadline_clock_run_source_has_an_unread_edge() {
        let body = method_body("Runtime", "raise_psreadline_invite_if_due");
        assert!(
            !body.contains("self.refresh_psreadline_installed()"),
            "the turn must enter the shared unread gate, never refresh directly"
        );
        assert!(body.contains("psreadline::installed_on_probe("));
        let gate = free_fn_body("installed_on_probe");
        assert!(gate.contains("if cache.is_none() && probe.is_some() {\n        refresh_installed(cache, documents);\n    }"));
        assert_eq!(gate.matches("refresh_installed(").count(), 1);
        assert!(!body.contains("installed_copy("));
        assert!(body.contains("if installed != psreadline::InstalledCopy::None {"));
    }

    #[test]
    fn psreadline_readers_and_refresh_edges_are_wired_to_the_app_fact() {
        let body = |name: &str| method_body("Runtime", name);
        assert!(
            source_index()
                .declaration_of(&bt_source::ItemQuery::field("App", "psreadline_installed"))
                .unwrap_or_else(|failure| panic!("{failure}"))
                .contains("psreadline_installed: Option<psreadline::InstalledCopy>")
        );
        let row = body("psreadline_row_state");
        assert!(row.contains("self.app.psreadline_installed.unwrap_or_default()"));
        assert!(!row.contains("installed_copy("));
        let refresh = body("refresh_psreadline_installed");
        assert!(refresh.contains("psreadline::refresh_installed("));
        assert!(refresh.contains("&mut self.app.psreadline_installed"));
        let apply = body("apply_psreadline");
        assert_eq!(
            apply
                .matches("self.refresh_psreadline_installed();")
                .count(),
            2
        );
        for outcome in ["Outcome::Installed(root)", "Outcome::Removed(root)"] {
            assert!(
                apply
                    .split_once(outcome)
                    .unwrap()
                    .1
                    .split("Ok(true)")
                    .next()
                    .unwrap()
                    .contains("self.refresh_psreadline_installed();")
            );
        }
        let layout = body("settings_layout");
        let compact: String = layout.split_whitespace().collect();
        assert!(compact.contains("self.window.settings.take_psreadline_open_edge("));
        assert!(layout.contains("if psreadline_opened {"));
        assert_eq!(
            layout
                .matches("self.refresh_psreadline_installed();")
                .count(),
            1
        );
        let edge = layout
            .split_once("if psreadline_opened {")
            .unwrap()
            .1
            .split('}')
            .next()
            .unwrap();
        assert!(edge.contains("self.refresh_psreadline_installed();"));
        assert_eq!(
            in_the_product_raw(bt_source::needle!(bt_source::Pattern::text(
                "self.refresh_psreadline_installed();"
            ))),
            3,
            "only install, remove, and Terminal-page open refresh the disk fact"
        );
    }

    fn temp_dir(tag: &str) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("folio-psreadline-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// [`install_into`] into a place that is Folio's to write, which is what
    /// every fixture below lays out for itself. A refusal here is the fixture
    /// being wrong, not the claim, so it panics naming which.
    fn install_ours(documents: &Path) -> PathBuf {
        match install_into(documents) {
            Ok(Wrote::Module(root)) => root,
            other => panic!("the fixture's own directory was refused: {other:?}"),
        }
    }

    /// [`remove_from`] read the way the row's `Off` reads it: did Folio's own
    /// files go?
    fn removed_ours(documents: &Path) -> bool {
        matches!(remove_from(documents).unwrap(), Removed::Took { .. })
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
        let nothing = InstalledCopy::None;
        assert_eq!(row_state(None, State::NotAsked, nothing), RowState::Probing);
        assert_eq!(row_state(old, State::NotAsked, nothing), RowState::Outdated);
        assert_eq!(
            row_state(current, State::NotAsked, nothing),
            RowState::AlreadyCurrent
        );
        assert_eq!(
            row_state(old, State::Installed, InstalledCopy::ThisBuild),
            RowState::InstalledByFolio
        );
        assert_eq!(
            row_state(old, State::Installed, nothing),
            RowState::RemovedElsewhere,
            "the file says Folio wrote it and the directory is gone — a fact the \
             row owes the reader rather than one to quietly correct"
        );
        // **And the state the 2026-08-18 ruling added, from both sides of the
        // probe.** An older Folio module reports its `ModuleVersion` as 2.4.6,
        // so the probe says `already_current` and the old `bool` filed it under
        // `AlreadyCurrent` - the row telling a reader nothing was owed while
        // this build carried a newer repair for the very module in front of it.
        for probe in [old, current, None] {
            for invite in [State::NotAsked, State::Declined, State::Installed] {
                assert_eq!(
                    row_state(probe, invite, InstalledCopy::OlderBuild),
                    RowState::UpdateAvailable,
                    "what is on disk decides this one, not the probe and not the file: {probe:?} / {invite:?}"
                );
            }
        }
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
        assert!(
            remove_available(RowState::UpdateAvailable),
            "a module an older Folio wrote is still Folio's to take back"
        );
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
            !removed_ours(&documents),
            "there is nothing there to remove"
        );

        let root = install_ours(&documents);
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

        assert!(removed_ours(&documents));
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
        let root = install_ours(&documents);
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
        assert!(!removed_ours(&documents));
        assert!(psm1.exists(), "and nothing in it was deleted");
        assert_eq!(std::fs::read(&psm1).unwrap(), text);

        // A directory that is merely incomplete is equally not Folio's.
        std::fs::remove_file(root.join("License.txt")).unwrap();
        assert!(!is_folios_copy(&documents));
        assert!(!removed_ours(&documents));
        std::fs::remove_dir_all(&documents).unwrap();
    }

    /// Lay somebody else's PSReadLine 2.4.6 into the version leaf, the way
    /// `Install-Module PSReadLine -RequiredVersion 2.4.6 -Scope CurrentUser`
    /// leaves it: the module's own files under the names PowerShell resolves,
    /// **and PowerShellGet's sidecars beside them**.
    ///
    /// The sidecars are the half that makes this fixture faithful. They are not
    /// in [`BUNDLED_FILES`], so they survived Folio's write on a real machine
    /// and then went with the whole leaf on the next `Off` — files this product
    /// never wrote and could not hand back.
    ///
    /// Returns every path laid and its bytes, so the assertion afterwards is
    /// "all of it, byte for byte" rather than a sample.
    fn lay_a_foreign_module(documents: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let root = module_directory(documents);
        let mut laid = Vec::new();
        let sidecars = [
            (
                "PSGetModuleInfo.xml",
                "<Objs><Obj><Repository>PSGallery</Repository></Obj></Objs>",
            ),
            ("PSReadLine.cat", "catalog"),
            (
                "en-US/about_PSReadLine.help.txt",
                "TOPIC\r\n    about_PSReadLine\r\n",
            ),
        ];
        for (name, bytes) in BUNDLED_FILES
            .iter()
            .map(|(name, _)| (*name, format!("the gallery's {name}")))
            .chain(
                sidecars
                    .iter()
                    .map(|(name, body)| (*name, (*body).to_owned())),
            )
        {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, bytes.as_bytes()).unwrap();
            laid.push((path, bytes.into_bytes()));
        }
        laid
    }

    /// RED GATE (audit 3, E-1) — **a module this build did not write survives
    /// an install, from every state that used to permit the write.**
    ///
    /// The counterpart of `a_module_this_build_did_not_write_survives_a_removal`,
    /// which has guarded the delete since the feature was written while the
    /// write beside it had no occupancy check at all. Three ways onto that
    /// write, all of them ordinary:
    ///
    /// 1. `Outdated` with a **stale** probe — the reader ran `Install-Module`
    ///    in a pane after Folio read the machine, and the probe is a one-shot.
    /// 2. `Outdated` with a **blind** probe — `powershell.exe` was not found or
    ///    its output did not parse, so `Probe::default()` reports no version at
    ///    all and `already_current` is false.
    /// 3. `RemovedElsewhere` with a **fresh** probe — the stored invitation says
    ///    Folio installed the module and the directory no longer holds Folio's
    ///    copy. No race at all: it survives restarts.
    ///
    /// What must be true afterwards is the same in all three: every byte the
    /// gallery put there is still there, none of Folio's nine files is, and the
    /// press said why.
    ///
    /// MUTATION: take the occupancy check out of the writer and all three arms
    /// fail on the first file compared.
    #[test]
    fn a_module_this_build_did_not_write_survives_an_install() {
        for (tag, state, probe) in [
            (
                "stale",
                RowState::Outdated,
                Some(probe_at("2.0.0", ExecutionPolicy::RemoteSigned)),
            ),
            ("blind", RowState::Outdated, Some(Probe::default())),
            (
                "removed-elsewhere",
                RowState::RemovedElsewhere,
                Some(probe_at("2.4.6", ExecutionPolicy::RemoteSigned)),
            ),
        ] {
            let documents = temp_dir(&format!("foreign-install-{tag}"));
            let laid = lay_a_foreign_module(&documents);
            let root = module_directory(&documents);

            // **The state is handed in, not computed**, which is the whole
            // claim: even a caller holding a state that permits the write — and
            // all three of these did, on a machine in exactly this condition —
            // cannot get past the check inside the writer.
            let outcome = apply(true, Some(&documents), state, probe);
            let Outcome::Refused(refusal) = outcome else {
                panic!("{tag}: the write went ahead over somebody else's module: {outcome:?}");
            };
            assert_eq!(refusal.tag(), "occupied");
            assert!(
                refusal.sentence().contains(&root.display().to_string()),
                "{tag}: the card must name the directory: {:?}",
                refusal.sentence()
            );

            for (path, bytes) in &laid {
                assert_eq!(
                    std::fs::read(path).unwrap().as_slice(),
                    bytes.as_slice(),
                    "{tag}: {} was overwritten",
                    path.display()
                );
            }
            assert!(
                !is_folios_copy(&documents),
                "{tag}: Folio's own bytes are now in a directory it does not own"
            );
            // Nothing arrived either: the leaf holds exactly what was laid in
            // it, counted at the top level where a stray file would land.
            let mut left: Vec<String> = std::fs::read_dir(&root)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            let mut expected: Vec<String> = laid
                .iter()
                .map(|(path, _)| {
                    path.strip_prefix(&root)
                        .unwrap()
                        .components()
                        .next()
                        .unwrap()
                        .as_os_str()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            left.sort();
            expected.sort();
            expected.dedup();
            left.dedup();
            assert_eq!(left, expected, "{tag}: the leaf gained or lost an entry");

            // And the row says so, from every stored state — `RemovedElsewhere`
            // included, which used to be answered before the disk was read at
            // all.
            let installed = installed_copy(&documents);
            assert_eq!(installed, InstalledCopy::Foreign, "{tag}");
            for invite in [State::NotAsked, State::Declined, State::Installed] {
                let told = row_state(probe, invite, installed);
                assert_eq!(told, RowState::NotOurs, "{tag} / {invite:?}");
                assert!(!install_available(probe, told), "{tag} / {invite:?}");
                assert!(!remove_available(told), "{tag} / {invite:?}");
            }
            // `Off` keeps its hands off it too.
            assert_eq!(remove_from(&documents).unwrap(), Removed::NotOurs);
            for (path, bytes) in &laid {
                assert_eq!(std::fs::read(path).unwrap().as_slice(), bytes.as_slice());
            }
            std::fs::remove_dir_all(&documents).unwrap();
        }
    }

    /// PIN (audit 3, E-1) — **the four shapes the place may be in and still be
    /// Folio's to write into.**
    ///
    /// The other half of the gate above: a rule that refuses everything is not
    /// the rule this ticket asked for. Absent, empty, this build's own copy, and
    /// a copy of ours that was interrupted — all four still take the install,
    /// and the row offers it.
    ///
    /// MUTATION: make the check "the leaf is absent" and the last two fail;
    /// make it "byte identity" and the fourth fails.
    #[test]
    fn an_absent_an_empty_and_folios_own_leaf_all_take_the_install() {
        // Absent — the ordinary machine, and the only shape the feature was
        // ever measured on.
        let documents = temp_dir("free-absent");
        assert_eq!(installed_copy(&documents), InstalledCopy::None);
        let root = install_ours(&documents);
        assert!(is_folios_copy(&documents));

        // This build's own copy: the same verb again, which is what the row's
        // `Update` performs.
        assert_eq!(installed_copy(&documents), InstalledCopy::ThisBuild);
        assert_eq!(
            install_into(&documents).unwrap(),
            Wrote::Module(root.clone())
        );

        // Interrupted, in the two places it can be interrupted: after the
        // assembly landed, and before it.
        std::fs::write(root.join("PSReadLine.psd1"), b"truncated").unwrap();
        assert_eq!(
            installed_copy(&documents),
            InstalledCopy::None,
            "our own assembly with a half-written file beside it is still ours"
        );
        assert!(matches!(install_into(&documents), Ok(Wrote::Module(_))));

        std::fs::remove_dir_all(&root).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(
            installed_copy(&documents),
            InstalledCopy::None,
            "an empty leaf is an empty leaf"
        );
        std::fs::write(root.join("Changes.txt"), bundled("Changes.txt")).unwrap();
        assert_eq!(
            installed_copy(&documents),
            InstalledCopy::None,
            "and so is one holding nothing but a file this build writes"
        );
        assert!(matches!(install_into(&documents), Ok(Wrote::Module(_))));
        assert!(is_folios_copy(&documents));
        std::fs::remove_dir_all(&documents).unwrap();
    }

    /// PIN (audit 3, E-1) — **the leaf a Folio before this fix left behind: our
    /// module, the gallery's sidecars still beside it.**
    ///
    /// This is the damaged state on real machines, and it is the one the rule
    /// has to answer in two directions at once. For the **update** it is ours —
    /// the assembly PowerShell loads is Folio's, and a reader whose module is
    /// out of date must not be stranded by a guard that reads the litter beside
    /// it as somebody else's ownership. For the **removal** it is not: the nine
    /// names go and every file Folio never wrote stays, together with the
    /// directory holding them.
    ///
    /// MUTATION: `remove_dir_all` the leaf again, as the removal did until
    /// today, and the three sidecar assertions fail together.
    #[test]
    fn a_leaf_of_ours_with_somebody_elses_sidecars_updates_and_is_removed_file_by_file() {
        let documents = temp_dir("mixed-leaf");
        let root = install_ours(&documents);
        let sidecars = [
            (
                "PSGetModuleInfo.xml",
                "<Objs><Repository>PSGallery</Repository></Objs>",
            ),
            ("PSReadLine.cat", "catalog"),
            ("en-US/about_PSReadLine.help.txt", "TOPIC"),
        ];
        for (name, body) in sidecars {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, body.as_bytes()).unwrap();
        }

        assert_eq!(
            installed_copy(&documents),
            InstalledCopy::ThisBuild,
            "the module here is Folio's; what is beside it is bookkeeping"
        );
        assert!(matches!(install_into(&documents), Ok(Wrote::Module(_))));

        let Removed::Took { left } = remove_from(&documents).unwrap() else {
            panic!("a module of Folio's must come off");
        };
        for (name, _) in BUNDLED_FILES {
            assert!(
                !root.join(name).exists(),
                "{name} is Folio's and should have gone"
            );
        }
        for (name, body) in sidecars {
            assert_eq!(
                std::fs::read(root.join(name)).unwrap(),
                body.as_bytes(),
                "{name} is not Folio's to delete"
            );
        }
        assert!(
            root.is_dir(),
            "and the directory holding them stays with them"
        );
        assert!(!root.join("net6plus").exists(), "our own subdirectories go");
        assert!(!root.join("netstd").exists());

        let named: Vec<String> = left.iter().map(|path| path.display().to_string()).collect();
        for (name, _) in sidecars {
            let head = Path::new(name).components().next().unwrap();
            let expected = root.join(head.as_os_str()).display().to_string();
            assert!(
                named.contains(&expected),
                "the door must be able to name what it left: {named:?}"
            );
        }
        // **And what is left is not Folio's either.** With the module gone the
        // leaf still holds PowerShellGet's claim on that version — a second
        // `Off` says so rather than reporting a removal of nothing, and `On`
        // will not write back into it. That is the rule holding in the
        // direction nobody enjoys: `Get-InstalledModule` still reports 2.4.6
        // installed from this directory, so it is still somebody's, and the way
        // out is `Uninstall-Module` rather than Folio writing over the claim a
        // second time.
        assert_eq!(remove_from(&documents).unwrap(), Removed::NotOurs);
        assert_eq!(installed_copy(&documents), InstalledCopy::Foreign);
        assert_eq!(install_into(&documents).unwrap(), Wrote::NotOurs);
        std::fs::remove_dir_all(&documents).unwrap();
    }

    /// Rewrite the installed module's `ProductVersion` string **in place**, so a
    /// test can stand a module some other build wrote in front of the row.
    ///
    /// The version resource stores its strings with a length in the header, so
    /// the replacement is padded with NULs to exactly the units the original
    /// occupied and the block stays walkable — which is what makes this a
    /// faithful fixture rather than a corrupted file. What comes out the other
    /// end is byte for byte what an older release's DLL is, as far as the only
    /// thing that reads it is concerned.
    ///
    /// Returns how many occurrences were rewritten, which is asserted rather
    /// than assumed: a fixture that silently changed nothing would make every
    /// claim below vacuously true.
    #[cfg(windows)]
    fn stamp_installed_build(root: &Path, build: &str) -> usize {
        let dll = root.join(BUILD_STAMP_FILE);
        let mut bytes = std::fs::read(&dll).unwrap();
        let units = PATCHED_BUILD.encode_utf16().count();
        assert!(
            build.encode_utf16().count() <= units,
            "a longer stamp would not fit the resource's own length"
        );
        let needle: Vec<u8> = PATCHED_BUILD
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let mut padded: Vec<u16> = build.encode_utf16().collect();
        padded.resize(units, 0);
        let replacement: Vec<u8> = padded.into_iter().flat_map(u16::to_le_bytes).collect();

        let mut rewritten = 0;
        let mut at = 0;
        while at + needle.len() <= bytes.len() {
            if bytes[at..at + needle.len()] == needle[..] {
                bytes[at..at + needle.len()].copy_from_slice(&replacement);
                rewritten += 1;
                at += needle.len();
            } else {
                at += 1;
            }
        }
        std::fs::write(&dll, &bytes).unwrap();
        rewritten
    }

    /// PIN (2026-08-18) — **the build stamp this file names is the one the
    /// shipped bytes carry.**
    ///
    /// `PATCHED_VERSION`'s own pin, one level down. That one binds this file to
    /// `folio.ps1`; this one binds it to the DLL, because the upgrade door is
    /// decided entirely by a string comparison and a literal that drifts from
    /// the bundle turns every installed copy into "an older build" — the row
    /// would offer an update that installs the same bytes, for ever.
    ///
    /// Read out of an install rather than out of the source tree, so that what
    /// is measured is what `install_into` actually writes.
    ///
    /// MUTATION: bump `PATCHED_BUILD` alone and this fails naming both strings.
    #[cfg(windows)]
    #[test]
    fn the_bundled_module_carries_the_build_this_file_names() {
        let documents = temp_dir("build-stamp");
        install_ours(&documents);
        assert_eq!(
            installed_build(&documents).as_deref(),
            Some(PATCHED_BUILD),
            "the module this build installs stamps a different build than \
             PATCHED_BUILD says"
        );
        assert!(
            PATCHED_BUILD.starts_with(&family_prefix()),
            "and the stamp is inside the family the row recognises: \
             {PATCHED_BUILD} against {}",
            family_prefix()
        );
        std::fs::remove_dir_all(&documents).unwrap();
    }

    /// PIN (user ruling 2026-08-18) — **a module an older Folio installed is
    /// offered an update, and both verbs on the row work on it.**
    ///
    /// The hole: `is_folios_copy` compares against *this* build's bytes, so an
    /// older Folio's module answered "not mine" — and the probe, which reads
    /// `ModuleVersion` and gets `2.4.6` from every `-bt` build alike, answered
    /// "already current". The row therefore greyed both items over a module this
    /// product had put there itself, and the only way out was `Documents`.
    ///
    /// What the row does about it is the ruling's own shape: the verbs stay `On`
    /// and `Off`, the value reads `Update`, and turning it on writes this build's
    /// files over the older ones in the same directory.
    ///
    /// MUTATIONS:
    /// (1) drop the family arm from `installed_copy` — the state goes back to
    ///     `AlreadyCurrent` and both `_available` assertions go red;
    /// (2) leave `remove_from` guarded on byte identity — the removal at the end
    ///     returns `false` and the directory survives, which is the bug reported.
    #[cfg(windows)]
    #[test]
    fn a_module_an_older_folio_wrote_is_offered_an_update_and_answers_both_verbs() {
        let documents = temp_dir("older-build");
        let root = install_ours(&documents);
        assert!(stamp_installed_build(&root, "2.4.6-bt.1") > 0);

        assert_eq!(installed_build(&documents).as_deref(), Some("2.4.6-bt.1"));
        assert!(
            !is_folios_copy(&documents),
            "byte identity still answers the question it was asked: these are \
             not the bytes this build ships"
        );
        assert_eq!(installed_copy(&documents), InstalledCopy::OlderBuild);

        // The probe reports the *manifest's* version, which every -bt build says
        // is 2.4.6 — which is exactly why the disk has to be believed over it.
        let machine = Some(Probe {
            version: Version::parse(PATCHED_VERSION),
            policy: ExecutionPolicy::RemoteSigned,
        });
        let state = row_state(machine, State::Installed, installed_copy(&documents));
        assert_eq!(state, RowState::UpdateAvailable);
        assert!(
            install_available(machine, state),
            "On is what performs the update"
        );
        assert!(
            remove_available(state),
            "and Off takes an older Folio's module off the machine, which is \
             the other half of the report"
        );
        // The sentence, asserted through the words rather than through
        // `row_description_in`: that function reads *this machine's* Documents
        // (and caches per language, as the three states beside it do), so a
        // claim about a temporary directory made through it would be a claim
        // about the developer's own module.
        for lang in i18n::Lang::ALL {
            let line = i18n::psreadline_row_update_in(lang, "2.4.6-bt.1", PATCHED_BUILD);
            assert!(
                line.contains("2.4.6-bt.1") && line.contains(PATCHED_BUILD),
                "{lang:?}: the row names what is installed and what is available: {line:?}"
            );
        }

        // On, over the older build.
        install_ours(&documents);
        assert!(is_folios_copy(&documents));
        assert_eq!(installed_copy(&documents), InstalledCopy::ThisBuild);
        assert_eq!(
            row_state(machine, State::Installed, installed_copy(&documents)),
            RowState::InstalledByFolio,
            "and once replaced there is nothing left to offer"
        );

        // Off, over an older build again.
        assert!(stamp_installed_build(&root, "2.4.6-bt.1") > 0);
        assert!(removed_ours(&documents));
        assert!(!root.exists());
        std::fs::remove_dir_all(&documents).unwrap();
    }

    /// PIN (user ruling 2026-08-18) — **somebody's own 2.4.6 is not Folio's
    /// family, is offered no update, and is never deleted.**
    ///
    /// The other side of widening the guard, and the side that has to be
    /// airtight: what may be removed is a module carrying Folio's own `-bt`
    /// stamp — a string only this project's builds put in that file — and never
    /// a module that merely reaches the same version number. A stock 2.4.6 from
    /// the gallery says `2.4.6` in the same field.
    ///
    /// MUTATION: match the family on `PATCHED_VERSION` instead of on
    /// `family_prefix()` and this deletes a stranger's module.
    ///
    /// **Rewritten by audit 3 (E-1) where it used to say `AlreadyCurrent`.**
    /// The old row reached that state through the *probe*, which reported
    /// `2.4.6` because the machine's own module said so — an accidental
    /// protection that held only while the probe agreed with the disk, and the
    /// probe is read once per process. What answers now is the disk: the module
    /// is somebody's, so the state is [`RowState::NotOurs`] whatever the probe
    /// says. Both verbs stay dark, which is what the ruling asked for and what
    /// the old row happened to do.
    #[cfg(windows)]
    #[test]
    fn a_stock_module_at_the_same_version_is_not_folios_and_is_left_alone() {
        let documents = temp_dir("stock-current");
        let root = install_ours(&documents);
        assert!(stamp_installed_build(&root, PATCHED_VERSION) > 0);

        assert_eq!(
            installed_build(&documents),
            None,
            "no -bt, no family: {:?}",
            installed_build(&documents)
        );
        assert_eq!(installed_copy(&documents), InstalledCopy::Foreign);
        assert!(
            !removed_ours(&documents),
            "and the guard refuses to delete it"
        );
        assert!(root.exists());

        // **Every probe this machine could have answered with**, because the
        // point of the state is that none of them decides it: the stale one that
        // made this write reachable, the blind one, and the fresh one that used
        // to be the only thing standing in front of the directory.
        for machine in [
            Some(probe_at(PATCHED_VERSION, ExecutionPolicy::RemoteSigned)),
            Some(probe_at("2.0.0", ExecutionPolicy::RemoteSigned)),
            Some(Probe::default()),
            None,
        ] {
            for invite in [State::NotAsked, State::Declined, State::Installed] {
                let state = row_state(machine, invite, installed_copy(&documents));
                assert_eq!(
                    state,
                    RowState::NotOurs,
                    "{machine:?} / {invite:?}: the disk decides this one"
                );
                assert!(!install_available(machine, state));
                assert!(!remove_available(state));
            }
        }
        std::fs::remove_dir_all(&documents).unwrap();
    }

    /// PIN — installing over an interrupted install repairs it.
    #[test]
    fn a_second_install_repairs_a_half_written_one() {
        let documents = temp_dir("repair");
        install_ours(&documents);
        let root = module_directory(&documents);
        std::fs::write(root.join("PSReadLine.psd1"), b"truncated").unwrap();
        std::fs::remove_file(root.join("netstd/Microsoft.PowerShell.PSReadLine.Polyfiller.dll"))
            .unwrap();
        assert!(!is_folios_copy(&documents));

        install_ours(&documents);
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
    ///
    #[test]
    fn no_line_this_row_has_cached_survives_a_language_switch() {
        for state in [
            RowState::Probing,
            RowState::RemovedElsewhere,
            RowState::Outdated,
            RowState::InstalledByFolio,
            RowState::AlreadyCurrent,
            RowState::NotOurs,
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

    // ── §7.47: a refused press is never a silent one ────────────────────────

    fn probe_at(version: &str, policy: ExecutionPolicy) -> Probe {
        Probe {
            version: Version::parse(version),
            policy,
        }
    }

    /// RED GATE (§7.47) — **a refused install says why instead of just staying
    /// off.**
    ///
    /// The write is made to fail the way a real one fails: `WindowsPowerShell`
    /// is a *file* where the module path wants a directory, so `create_dir_all`
    /// cannot get past it. What must come back is a refusal carrying the path
    /// and Windows' own words — not `Ok(false)`, which is what the runtime used
    /// to turn into nothing at all.
    ///
    /// MUTATION: make `apply` return anything without a sentence on a failed
    /// write — an early `return`, a swallowed `Err`, an `Outcome::Installed`
    /// on a path nothing was written to — and this fails.
    #[test]
    fn a_refused_install_says_why_instead_of_staying_off() {
        let documents = temp_dir("refused-write");
        // The one level the module path needs, occupied by a file.
        std::fs::write(documents.join("WindowsPowerShell"), b"not a directory").unwrap();

        let outcome = apply(
            true,
            Some(&documents),
            RowState::Outdated,
            Some(probe_at("2.0.0", ExecutionPolicy::RemoteSigned)),
        );
        let Outcome::Refused(refusal) = outcome else {
            panic!("a write that cannot happen must refuse, not report success: {outcome:?}");
        };
        assert_eq!(refusal.tag(), "write-failed");
        let sentence = refusal.sentence();
        assert!(
            sentence.contains(&module_directory(&documents).display().to_string()),
            "the card must name the path the module would have gone to: {sentence:?}"
        );
        assert!(
            sentence.len() > module_directory(&documents).display().to_string().len(),
            "and Windows' own words beside it: {sentence:?}"
        );
        // The row did not move: nothing of ours is on disk.
        assert_eq!(installed_copy(&documents), InstalledCopy::None);
        assert!(!remove_available(row_state(
            Some(probe_at("2.0.0", ExecutionPolicy::RemoteSigned)),
            State::NotAsked,
            installed_copy(&documents),
        )));
        let _ = std::fs::remove_dir_all(&documents);
    }

    /// RED GATE (§7.47) — **the module directory is created a level at a time.**
    ///
    /// The root handed in does not exist, and neither does one level under it:
    /// `Documents` itself, `WindowsPowerShell`, `Modules`, `PSReadLine`, the
    /// version leaf, and the two polyfiller subdirectories are all made by the
    /// install. This is the shape a brand-new Windows account is in — measured
    /// on a clean Windows 10 on 2026-08-29, where `PSModulePath` already named
    /// `…\Documents\WindowsPowerShell\Modules` and neither directory was there.
    ///
    /// MUTATION: `create_dir` in place of `create_dir_all` and this fails on
    /// the first file.
    #[test]
    fn the_module_directory_is_created_level_by_level() {
        let parent = temp_dir("levels");
        // Nothing below this exists — not even the `Documents` folder itself.
        let documents = parent.join("Documents");
        assert!(!documents.exists());

        let root = install_ours(&documents);
        assert_eq!(root, module_directory(&documents));
        for level in [
            documents.clone(),
            documents.join("WindowsPowerShell"),
            documents.join(r"WindowsPowerShell\Modules"),
            documents.join(MODULE_RELATIVE_PATH),
            root.clone(),
            root.join("net6plus"),
            root.join("netstd"),
        ] {
            assert!(level.is_dir(), "{} was not created", level.display());
        }
        for (name, bytes) in BUNDLED_FILES {
            assert_eq!(
                std::fs::read(root.join(name)).unwrap().as_slice(),
                bytes,
                "{name} did not land"
            );
        }
        assert_eq!(installed_copy(&documents), InstalledCopy::ThisBuild);
        let _ = std::fs::remove_dir_all(&parent);
    }

    /// RED GATE (§7.47) — **the root cause: a policy that refuses the module
    /// says which policy, on the row and in the card.**
    ///
    /// This is the machine the 2026-08-29 report came from and the one this VM
    /// run reproduced: `Get-ExecutionPolicy` answers `Restricted` — Windows'
    /// own default on a client — the `On` item goes dark, and before this gate
    /// nothing on the Terminal page said so. The refusal is *correct*: measured
    /// on that machine, the nine files write and `Import-Module PSReadLine`
    /// then loads nothing, because `PSReadLine.format.ps1xml` is a script and
    /// the policy refuses it. What was wrong was that it was mute.
    ///
    /// MUTATION: drop the policy branch from `outdated_line` and the row goes
    /// back to `2.0.0 on this machine · resizing still misplaces the input
    /// line`; drop it from `apply` and the card stops naming `Restricted`.
    #[test]
    fn a_policy_that_refuses_the_module_says_which_policy_and_where() {
        for policy in [ExecutionPolicy::Restricted, ExecutionPolicy::AllSigned] {
            let probe = probe_at("2.0.0", policy);
            for lang in [i18n::Lang::English, i18n::Lang::Chinese] {
                let line = outdated_line(lang, probe);
                assert!(
                    line.contains(policy.name()),
                    "the row must name the policy that dims the switch: {line:?}"
                );
                assert!(
                    line.contains("2.0.0"),
                    "and what is on the machine: {line:?}"
                );
            }

            let documents = temp_dir("policy");
            let outcome = apply(true, Some(&documents), RowState::Outdated, Some(probe));
            let Outcome::Refused(refusal) = outcome else {
                panic!("a policy that refuses the module must refuse the write: {outcome:?}");
            };
            assert_eq!(refusal.tag(), "policy");
            let sentence = refusal.sentence();
            assert!(
                sentence.contains(policy.name()),
                "the card must name the policy: {sentence:?}"
            );
            assert!(
                sentence.contains(&module_directory(&documents).display().to_string()),
                "and the path it would have written to: {sentence:?}"
            );
            // **And a way out.** A card that names an obstacle and stops leaves
            // its reader where they were standing; the copy rule is that a
            // sentence on screen says what can be done now, and here that is one
            // command in the shell already in front of them.
            assert!(
                sentence.contains(i18n::POLICY_REMEDY_COMMAND),
                "and the command that lifts the refusal: {sentence:?}"
            );
            assert!(
                i18n::POLICY_REMEDY_COMMAND.contains("-Scope CurrentUser"),
                "in the scope an unelevated shell can actually set"
            );
            assert!(
                !module_directory(&documents).exists(),
                "and it must not have written anything"
            );
            let _ = std::fs::remove_dir_all(&documents);
        }
    }

    /// PIN (§7.47) — **the greyed item and the refusal are two readings of one
    /// table.**
    ///
    /// `install_available` decides what the picker draws and `apply` decides
    /// what a press does. They were allowed to drift for as long as the press
    /// could not reach a greyed item at all; now that it can, a state where one
    /// says yes and the other refuses would be a switch that moves under a dark
    /// item, or a lit item that does nothing.
    #[test]
    fn the_greyed_item_and_the_refusal_agree_on_every_state() {
        let states = [
            RowState::Probing,
            RowState::Outdated,
            RowState::InstalledByFolio,
            RowState::UpdateAvailable,
            RowState::AlreadyCurrent,
            RowState::RemovedElsewhere,
            RowState::NotOurs,
        ];
        for policy in [
            ExecutionPolicy::Restricted,
            ExecutionPolicy::AllSigned,
            ExecutionPolicy::RemoteSigned,
            ExecutionPolicy::Bypass,
        ] {
            for state in states {
                // `Probing` is the state with no probe, and vice versa: the two
                // are one fact read twice.
                let probe = (state != RowState::Probing).then(|| probe_at("2.0.0", policy));
                let documents = temp_dir("agree");
                let outcome = apply(true, Some(&documents), state, probe);
                let refused = matches!(outcome, Outcome::Refused(_));
                assert_eq!(
                    install_available(probe, state),
                    !refused,
                    "{state:?} under {policy:?}: the drawing says \
                     {}, the press says {}",
                    install_available(probe, state),
                    if refused { "no" } else { "yes" }
                );
                let _ = std::fs::remove_dir_all(&documents);
            }
        }
    }

    /// PIN (§7.47) — **a `Documents` Windows would not name is a sentence, not
    /// a shrug.**
    ///
    /// The runtime used to `return Ok(false)` here, which reached no card, no
    /// row and no log line. It is the one refusal with no path in it, because
    /// there is no path — which is what it says.
    #[test]
    fn a_documents_folder_windows_will_not_name_still_says_something() {
        let outcome = apply(
            true,
            None,
            RowState::Outdated,
            Some(probe_at("2.0.0", ExecutionPolicy::RemoteSigned)),
        );
        assert_eq!(outcome, Outcome::Refused(Refusal::NoDocuments));
        let Outcome::Refused(refusal) = outcome else {
            unreachable!()
        };
        assert_eq!(refusal.tag(), "no-documents");
        assert!(refusal.sentence().contains(PATCHED_VERSION));
    }

    /// PIN (§7.47) — **every refusal has words, they are all different, and
    /// every one that has a path in hand puts it on screen.**
    ///
    /// The language in force is not touched: it is a process-wide fact and the
    /// suite runs in parallel, so a test that moved it would answer for its
    /// neighbours. What it checks is the shape a card must have in whatever
    /// language is up — a refusal with an empty sentence, or two refusals
    /// sharing one, is a switch that has gone quiet again by a different road.
    #[test]
    fn every_refusal_has_its_own_sentence_and_names_its_path() {
        let path = PathBuf::from(r"C:\Users\somebody\Documents\WindowsPowerShell\Modules");
        let shown = path.display().to_string();
        let refusals = [
            Refusal::StillReading,
            Refusal::Policy {
                policy: ExecutionPolicy::Restricted,
                path: path.clone(),
            },
            Refusal::NoDocuments,
            Refusal::AlreadyCurrent {
                found: "2.4.6".to_owned(),
                path: path.clone(),
            },
            Refusal::AlreadyThere { path: path.clone() },
            Refusal::Occupied { path: path.clone() },
            Refusal::Write {
                path: path.clone(),
                message: "Access is denied. (os error 5)".to_owned(),
            },
            Refusal::NotOurs { path: path.clone() },
            Refusal::Remove {
                path: path.clone(),
                message: "Access is denied. (os error 5)".to_owned(),
            },
        ];
        let mut seen: Vec<String> = Vec::new();
        for refusal in refusals {
            let sentence = refusal.sentence();
            assert!(
                !sentence.trim().is_empty(),
                "{} says nothing",
                refusal.tag()
            );
            assert!(
                !seen.contains(&sentence),
                "{} says what another refusal already said: {sentence:?}",
                refusal.tag()
            );
            // The two with nothing to point at are the two that say so.
            if !matches!(refusal, Refusal::StillReading | Refusal::NoDocuments) {
                assert!(
                    sentence.contains(&shown),
                    "{} does not name the path: {sentence:?}",
                    refusal.tag()
                );
            }
            seen.push(sentence);
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

    // ── ticket 56: Folio's own older copy is replaced at launch ─────────────

    fn on_disk(stamp: &str, recorded: bool) -> InstalledBuild {
        InstalledBuild {
            build: Build::parse(stamp),
            recorded,
        }
    }

    /// RED (56) — **Folio's own older PSReadLine build is replaced by the
    /// bundled one without an invitation.**
    ///
    /// Owner's ruling 2026-09-21, option A. On BASE nothing decided this at
    /// all: an older Folio copy silenced the invitation and waited on the
    /// Settings row for a press of `On`. The answer here is `Replace` — not
    /// `Invite`, because the invitation is for a machine Folio has never put
    /// its module on, and not `Keep`, which is what BASE did.
    ///
    /// MUTATION: return `Upgrade::Invite` for any installed copy in
    /// `upgrade_decision`.
    #[test]
    fn folios_own_older_psreadline_build_is_replaced_by_the_bundled_one_without_an_invitation() {
        let bundled = Build::parse("2.4.6-bt.2").unwrap();
        for older in ["2.4.6-bt.1", "2.4.6-bt.anchorfix"] {
            assert_eq!(
                upgrade_decision(Some(&on_disk(older, true)), &bundled),
                Upgrade::Replace,
                "{older} recorded by Folio, bundled 2.4.6-bt.2"
            );
        }
        assert_eq!(
            upgrade_decision(None, &bundled),
            Upgrade::Invite,
            "and a machine with nothing of Folio's on it is still only invited"
        );
    }

    /// RED (56) — **A copy Folio did not install is never replaced.**
    ///
    /// The stamp alone says a Folio build wrote the DLL; what says *this
    /// account's Folio put it here* is the install record. A Folio-stamped
    /// copy with no record — carried over by hand, restored from somebody's
    /// backup — is the reader's, and so are a stock `2.4.6` and an upstream
    /// `2.5.0`, which carry no Folio stamp at all (`PATCHED_BUILD`'s rule about
    /// `2.5.0` stands).
    ///
    /// MUTATION: drop `installed.recorded &&` from `upgrade_decision`.
    #[test]
    fn a_copy_folio_did_not_install_is_never_replaced() {
        let bundled = Build::bundled();
        assert_eq!(
            upgrade_decision(Some(&on_disk("2.4.6-bt.1", false)), &bundled),
            Upgrade::Keep,
            "a Folio-stamped 2.4.6 with no record of Folio installing it"
        );
        for stamp in ["2.4.6", "2.5.0", "2.5.0-bt.1"] {
            for recorded in [false, true] {
                assert_eq!(
                    upgrade_decision(Some(&on_disk(stamp, recorded)), &bundled),
                    Upgrade::Keep,
                    "{stamp}, recorded={recorded}"
                );
            }
        }
    }

    /// RED (56) — **The same build is kept, and a newer own build is kept.**
    ///
    /// A downgrade of Folio must not downgrade the module it installed, and
    /// the launch after an upgrade must not write the same bytes again. Also
    /// pins the order in `Build`'s doc: numbers compare as numbers, a named
    /// build predates every numbered one, and two named builds are unordered.
    ///
    /// MUTATION: `this < that` → `this <= that` in `Build::predates` (replace
    /// on equal).
    #[test]
    fn the_same_build_is_kept_and_a_newer_own_build_is_kept() {
        let bundled = Build::parse("2.4.6-bt.2").unwrap();
        for stamp in ["2.4.6-bt.2", "2.4.6-bt.3", "2.4.6-bt.10"] {
            assert_eq!(
                upgrade_decision(Some(&on_disk(stamp, true)), &bundled),
                Upgrade::Keep,
                "{stamp} against bundled 2.4.6-bt.2"
            );
        }
        let build = |stamp| Build::parse(stamp).unwrap();
        assert!(build("2.4.6-bt.9").predates(&build("2.4.6-bt.10")));
        assert!(!build("2.4.6-bt.10").predates(&build("2.4.6-bt.9")));
        assert!(build("2.4.6-bt.anchorfix").predates(&build("2.4.6-bt.1")));
        assert!(!build("2.4.6-bt.1").predates(&build("2.4.6-bt.anchorfix")));
        assert!(!build("2.4.6-bt.anchorfix").predates(&build("2.4.6-bt.other")));
        assert!(!build("2.4.5-bt.1").predates(&build("2.4.6-bt.2")));
        assert_eq!(build("2.4.6-bt.anchorfix").text(), "2.4.6-bt.anchorfix");
        assert_eq!(Build::parse("2.4.6-bt."), None);
        // The bundled build is numbered, so every named build predates it and
        // the launch can place every stamp this product has ever shipped.
        assert_eq!(Build::bundled().text(), PATCHED_BUILD);
        assert!(build("2.4.6-bt.anchorfix").predates(&Build::bundled()));
    }

    /// Stand an older Folio build in `documents`: the bundled files with the
    /// DLL's own `ProductVersion` rewritten to `stamp`, read back by the real
    /// version-resource reader.
    #[cfg(windows)]
    fn older_build_in(documents: &Path, stamp: &str) -> PathBuf {
        let root = install_ours(documents);
        assert!(stamp_installed_build(&root, stamp) > 0);
        assert_eq!(installed_build(documents).as_deref(), Some(stamp));
        assert_eq!(installed_copy(documents), InstalledCopy::OlderBuild);
        root
    }

    #[cfg(windows)]
    fn marks_roots(data: &Path) -> Vec<PathBuf> {
        crate::shell_integration::profile_marks::Marks::read(data)
            .unwrap()
            .psreadline_module_roots
    }

    /// RED (56) — **The road performs the replacement and records the new
    /// build (headless, the recorded file writes).**
    ///
    /// Real files in a temporary `Documents`, a real DLL whose version
    /// resource says `2.4.6-bt.1`, a real marks record in a temporary data
    /// root. Folio's record here is the one every install pressed before 0.4.3
    /// left — `settings.json` saying `Installed`, and no marks root — so the
    /// replacement's own record write is what puts the root in the marks, where
    /// the uninstall door looks. Then the launch after: nothing to do. And the
    /// two other cases on the same disk: the marks record alone is a record,
    /// and no record at all leaves the older copy exactly as it was.
    ///
    /// MUTATION: in `upgrade_recorded`, write through
    /// `install_checked(documents, |_| Ok(()))` instead of `install_recorded`
    /// (skip the record write).
    #[cfg(windows)]
    #[test]
    fn the_road_performs_the_replacement_and_records_the_new_build() {
        let documents = temp_dir("upgrade-road");
        let data = temp_dir("upgrade-road-data");
        let root = older_build_in(&documents, "2.4.6-bt.1");
        assert!(marks_roots(&data).is_empty());

        let replacement = upgrade_recorded(&documents, &data, State::Installed)
            .expect("Folio's own older build, recorded, is replaced");
        assert!(
            matches!(&replacement.wrote, Ok(Wrote::Module(wrote)) if *wrote == root),
            "{replacement:?}"
        );
        assert_eq!(replacement.from, "2.4.6-bt.1");
        assert!(
            replacement
                .log_line()
                .contains("upgraded 2.4.6-bt.1 to 2.4.6-bt.2"),
            "{}",
            replacement.log_line()
        );
        assert!(is_folios_copy(&documents), "the bundled bytes, all nine");
        assert_eq!(installed_build(&documents).as_deref(), Some(PATCHED_BUILD));
        assert_eq!(installed_copy(&documents), InstalledCopy::ThisBuild);
        assert_eq!(
            marks_roots(&data),
            vec![root.clone()],
            "and the install is recorded"
        );
        assert!(
            upgrade_recorded(&documents, &data, State::Installed).is_none(),
            "the launch after has nothing to do"
        );

        // The marks record alone is Folio's record.
        assert!(stamp_installed_build(&root, "2.4.6-bt.1") > 0);
        let replaced = upgrade_recorded(&documents, &data, State::NotAsked).unwrap();
        assert!(matches!(replaced.wrote, Ok(Wrote::Module(_))));
        assert_eq!(installed_copy(&documents), InstalledCopy::ThisBuild);

        // No record at all: the older copy is left exactly as it was.
        let unrecorded = temp_dir("upgrade-road-unrecorded");
        assert!(stamp_installed_build(&root, "2.4.6-bt.1") > 0);
        let before = std::fs::read(root.join(BUILD_STAMP_FILE)).unwrap();
        for invite in [State::NotAsked, State::Declined, State::Dismissed] {
            assert!(upgrade_recorded(&documents, &unrecorded, invite).is_none());
        }
        assert_eq!(std::fs::read(root.join(BUILD_STAMP_FILE)).unwrap(), before);
        assert!(marks_roots(&unrecorded).is_empty());

        for dir in [documents, data, unrecorded] {
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    /// RED (56) — **A replacement that stops part-way keeps the older build's
    /// stamp, so the next launch tries again.**
    ///
    /// The failure the ruling expects is a PowerShell somewhere holding one of
    /// the module's DLLs, which Windows will not let anybody overwrite. Stood
    /// in here, portably and without a PowerShell, by a directory where
    /// `Microsoft.PowerShell.Pager.dll` goes: the write of that one file fails
    /// as a locked one does. What must survive is the stamp — with it the copy
    /// still reads as the older build, and the launch after replaces it once the
    /// file is free.
    ///
    /// MUTATION: write the files in `BUNDLED_FILES` order in `install_checked`
    /// (the stamp DLL third, before the Pager) — the copy then carries this
    /// build's stamp over two builds' bytes, reads as an edit (`None`), and is
    /// never replaced again.
    #[cfg(windows)]
    #[test]
    fn a_replacement_that_stops_part_way_keeps_the_older_stamp_and_is_tried_again() {
        let documents = temp_dir("upgrade-part-way");
        let data = temp_dir("upgrade-part-way-data");
        let root = older_build_in(&documents, "2.4.6-bt.1");
        let blocked = root.join("Microsoft.PowerShell.Pager.dll");
        std::fs::remove_file(&blocked).unwrap();
        std::fs::create_dir(&blocked).unwrap();

        let failed = upgrade_recorded(&documents, &data, State::Installed).unwrap();
        assert!(failed.wrote.is_err(), "{failed:?}");
        assert!(
            failed.log_line().contains("kept until the next launch"),
            "{}",
            failed.log_line()
        );
        assert_eq!(installed_build(&documents).as_deref(), Some("2.4.6-bt.1"));
        assert_eq!(installed_copy(&documents), InstalledCopy::OlderBuild);
        assert_eq!(
            upgrade_decision(
                InstalledBuild::found(&documents, &data, State::Installed).as_ref(),
                &Build::bundled()
            ),
            Upgrade::Replace,
            "the next launch decides the same"
        );

        std::fs::remove_dir(&blocked).unwrap();
        let retried = upgrade_recorded(&documents, &data, State::Installed).unwrap();
        assert!(matches!(retried.wrote, Ok(Wrote::Module(_))), "{retried:?}");
        assert_eq!(installed_copy(&documents), InstalledCopy::ThisBuild);

        for dir in [documents, data] {
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}
