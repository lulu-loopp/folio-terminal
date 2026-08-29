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
        || {
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
    let output = bt_platform::quiet_command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", PROBE_COMMAND])
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
}

/// Which Folio-written copy, if any, is on disk under a Documents root.
///
/// Three answers and not a `bool`, because the middle one is the whole of the
/// 2026-08-18 ruling: "there is a module here and Folio's family wrote it, but
/// not this build" is a different sentence from either "this build wrote it" or
/// "nothing of ours is here", and a caller handed a `bool` has to pick one of
/// the two to lie with.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InstalledCopy {
    /// Nothing of Folio's family is in the module directory.
    #[default]
    None,
    /// Byte for byte, or build for build, what this executable carries.
    ThisBuild,
    /// A `2.4.6-bt.*` that is not this one — an older Folio's work.
    OlderBuild,
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
    let build = file_product_version(&stamp)?;
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
    if is_folios_copy(documents) {
        return InstalledCopy::ThisBuild;
    }
    match installed_build(documents) {
        // Our own family, our own build number, and bytes that are not ours:
        // this directory has been edited since Folio wrote it. See above.
        Some(build) if build == PATCHED_BUILD => InstalledCopy::None,
        Some(_) => InstalledCopy::OlderBuild,
        None => InstalledCopy::None,
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
pub fn remove_from(documents: &Path) -> io::Result<bool> {
    if installed_copy(documents) == InstalledCopy::None {
        return Ok(false);
    }
    std::fs::remove_dir_all(module_directory(documents))?;
    Ok(true)
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
pub fn apply(
    install: bool,
    documents: Option<&Path>,
    state: RowState,
    probe: Option<Probe>,
) -> Outcome {
    let Some(documents) = documents else {
        return Outcome::Refused(Refusal::NoDocuments);
    };
    let path = module_directory(documents);
    if !install {
        // The guard lives inside `remove_from`, so this is a report of what it
        // did rather than a second copy of its rule.
        return match remove_from(documents) {
            Ok(true) => Outcome::Removed(path),
            Ok(false) => Outcome::Refused(Refusal::NotOurs { path }),
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
    match install_into(documents) {
        Ok(root) => Outcome::Installed(root),
        Err(error) => Outcome::Refused(Refusal::Write {
            path,
            message: error.to_string(),
        }),
    }
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
        install_into(&documents).unwrap();
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
        let root = install_into(&documents).unwrap();
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
        install_into(&documents).unwrap();
        assert!(is_folios_copy(&documents));
        assert_eq!(installed_copy(&documents), InstalledCopy::ThisBuild);
        assert_eq!(
            row_state(machine, State::Installed, installed_copy(&documents)),
            RowState::InstalledByFolio,
            "and once replaced there is nothing left to offer"
        );

        // Off, over an older build again.
        assert!(stamp_installed_build(&root, "2.4.6-bt.1") > 0);
        assert!(remove_from(&documents).unwrap());
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
    #[cfg(windows)]
    #[test]
    fn a_stock_module_at_the_same_version_is_not_folios_and_is_left_alone() {
        let documents = temp_dir("stock-current");
        let root = install_into(&documents).unwrap();
        assert!(stamp_installed_build(&root, PATCHED_VERSION) > 0);

        assert_eq!(
            installed_build(&documents),
            None,
            "no -bt, no family: {:?}",
            installed_build(&documents)
        );
        assert_eq!(installed_copy(&documents), InstalledCopy::None);
        assert!(
            !remove_from(&documents).unwrap(),
            "and the guard refuses to delete it"
        );
        assert!(root.exists());

        let machine = Some(Probe {
            version: Version::parse(PATCHED_VERSION),
            policy: ExecutionPolicy::RemoteSigned,
        });
        let state = row_state(machine, State::NotAsked, installed_copy(&documents));
        assert_eq!(
            state,
            RowState::AlreadyCurrent,
            "a machine whose own module is new enough is told so, and offered \
             nothing"
        );
        assert!(!install_available(machine, state));
        assert!(!remove_available(state));
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

        let root = install_into(&documents).expect("an absent root is made, not refused");
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
}
