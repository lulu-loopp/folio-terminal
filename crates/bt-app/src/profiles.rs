//! The profile picker — the menu the tab strip's `˅` opens.
//!
//! Spec authority is `design/ui-mockup.html`: the `.profile-menu` / `.profile-item`
//! block (lines 1006-1030) for the surface and its rows, and `openProfileMenu`
//! (line 7409) for where the menu lands and what a click on a row does. Every
//! number below is that stylesheet's own.
//!
//! Those two line numbers were written as 976-1002 and 7296 and had drifted about
//! thirty lines as the mock-up grew above them. They are re-anchored here, and the
//! individual constants below carry their own — a reference that names a *number*
//! rots silently, so the ones that matter are stated beside the value they
//! justify where a wrong line is caught by the value not matching.
//!
//! Two facts decide the shape of this module:
//!
//! * **It is a popup, not a modal.** There is no scrim, so unlike [`crate::settings`]
//!   its [`hit`] returns `None` for a point that is not on the menu, and a press
//!   there closes the menu and then goes on about its business — which is exactly
//!   what the mock-up's `document.addEventListener("click", closeProfileMenu)`
//!   does.
//! * **It floats, so it blends.** Its lift, its hairline and its face are the
//!   same three planes every floating surface in this product is made of, built
//!   through the same [`crate::settings::push_float_window`] — a popup drawn out
//!   of opaque chrome quads would have to know what is under it, and nothing is
//!   under a popup but whatever the terminal happens to be showing.
//! * **It shows two lists, so a row is not a number.** Under the profiles sits
//!   `Recently opened` (mock-up 7424-7433), and its rows index the seed vault
//!   rather than [`PROFILES`]. Both [`hit`] and the hover therefore speak in
//!   [`MenuRow`], because the one thing a bare index cannot say is which list it
//!   came from — and the answer it gets wrong is silent.

use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    time::SystemTime,
};

use bt_pty::{ShellEnvironment, resolve_default_shell};
use bt_render::{
    ChromeLabel, ChromeLabelWeight, ChromePalette, FLOAT_WINDOW_BORDER_LOGICAL_PX,
    FLOAT_WINDOW_SHADOW_LOGICAL_PX, OverlayQuad, chrome_palette, rounded_overlay_fill,
};

use crate::{
    marks::{ChromeMark, ChromeSprite, OverlayLayer},
    seed::{RECENT_CAPACITY, RecentEntry, Seed, ago_label},
    settings::push_float_window,
};

// ── `.profile-menu` ────────────────────────────────────────────────────────
/// `min-width: 180px`. It is the only width the menu has: every row is one mark
/// and one short name, so nothing here ever asks for more than the minimum.
const MENU_MIN_WIDTH_LOGICAL_PX: f32 = 180.0;
/// `border-radius: 8px` — a popup menu's own round, the same one the theme
/// picker's menu wears, and deliberately not the 10px a floating *window* gets.
const MENU_RADIUS_LOGICAL_PX: f32 = 8.0;
const MENU_PADDING_LOGICAL_PX: f32 = 4.0;
/// `menu.style.top = a.bottom + 4` — the gap between the button and its menu.
const MENU_OFFSET_LOGICAL_PX: f32 = 4.0;
/// `Math.min(a.left, win.width - mw - 8)` — the menu never touches the window's
/// right edge, however near the edge the button that opened it sits.
const MENU_EDGE_MARGIN_LOGICAL_PX: f32 = 8.0;

// ── `.profile-item` ────────────────────────────────────────────────────────
/// `padding: 7px 10px` around a 13px line box, which measures 15.5px in the
/// mock-up's own renderer: 7 + 15.5 + 7.
const ITEM_HEIGHT_LOGICAL_PX: f32 = 29.5;
const ITEM_RADIUS_LOGICAL_PX: f32 = 5.0;
const ITEM_PADDING_X_LOGICAL_PX: f32 = 10.0;
/// `.profile-item { gap: 10px }`.
const ITEM_GAP_LOGICAL_PX: f32 = 10.0;
const ITEM_FONT_LOGICAL_PX: f32 = 13.0;
/// `.profile-item .ticon { width: 14px }` — the column. The mark inside it is
/// the strip's own 15px `.pmark`, centred, exactly as the flex box centres it.
const ITEM_ICON_COLUMN_LOGICAL_PX: f32 = 14.0;
const ITEM_MARK_LOGICAL_PX: f32 = 15.0;
/// `.default-hint { margin-left: auto; font-size: 11px; color: var(--ink3) }`.
///
/// Two annotations ride in this one slot: the profile list's `default`, and a
/// recent row's `agoLabel` (mock-up 7428/7432). They are the same declaration in the
/// same place, so they are the same number here.
const HINT_FONT_LOGICAL_PX: f32 = 11.0;
const HINT_TEXT: &str = "default";
/// What a row says instead of `default` when this machine cannot start it.
///
/// The same slot, because it is the same sentence in the same place: one short
/// annotation about the profile rather than about the pointer. `default` and
/// `not installed` are the two things a row can have to add, and no row has both
/// — the default profile is [`ProgramSource::DefaultShell`], which always
/// resolves.
///
/// The words are chosen against the alternative of showing nothing. A greyed row
/// with no caption asks the user to work out *why* it is grey, and the two
/// available guesses — "not on this machine" and "BetterTerminal is broken" —
/// are not equally actionable.
const UNAVAILABLE_HINT_TEXT: &str = "not installed";

// ── the greyed row ─────────────────────────────────────────────────────────
/// `.ticon-wrap.dead .ticon { opacity: .35; filter: grayscale(1) }` (mock-up
/// line 314) — the mock-up's own register for a mark that names something not
/// running, borrowed here for a mark that names something not installed.
///
/// Both fields, and neither alone: grayscale without the fade leaves a mark at
/// full strength that merely lost its colour, which reads as a *rendering* fault
/// rather than as a state; the fade without the grayscale leaves Ubuntu's orange
/// still the loudest thing in a menu of rows you cannot click. It is the one
/// place a profile mark is allowed to lose its own colours, and it is allowed
/// because the sentence being spoken is precisely "this is not one of your
/// shells".
const UNAVAILABLE_MARK_OPACITY: f32 = 0.35;

// ── `.menu-sep` (mock-up line 1025) ─────────────────────────────────────────
/// `height: 1px`, taken to whole device pixels and never below one.
///
/// Rounded rather than left fractional, which is where the floating window's own
/// border differs: a border is four edges around a rounded box that the coverage
/// pass is already antialiasing, while this is a single horizontal line, and a
/// horizontal line 1.25px tall is drawn as two rows of partial ink — a blurred
/// grey band instead of a rule. The `max` keeps it from rounding away entirely
/// at the scales where the ink is thinnest.
const SEPARATOR_THICKNESS_LOGICAL_PX: f32 = 1.0;
/// `margin: 5px 0`.
const SEPARATOR_MARGIN_Y_LOGICAL_PX: f32 = 5.0;
/// `background: var(--border-soft)` — `rgba(255,255,255,.06)` on dark,
/// `rgba(0,0,0,.055)` on light (mock-up lines 20 and 50).
///
/// The ink is the one `ChromePalette::menu_border` already carries (both tokens
/// are the theme's own black or white); only this softer alpha is missing from
/// the palette, so the pair is stated here and chosen **off the ink the palette
/// handed us** rather than off [`bt_render::current_theme`]. That is not a
/// detour: the palette is picked by background luma and the theme by the user's
/// setting, and under a `BT_BG` override those two answers differ — asking the
/// palette keeps the hairline in the same theme as the surface under it.
///
/// Its proper home is a pre-composited `--border-soft` over `--menu` in
/// [`ChromePalette`], which is a bt-render change this work item may not make.
const SEPARATOR_ALPHA_ON_DARK: f32 = 0.06;
/// The light theme's half of [`SEPARATOR_ALPHA_ON_DARK`].
const SEPARATOR_ALPHA_ON_LIGHT: f32 = 0.055;

// ── `.menu-label` (mock-up lines 1026-1029) ─────────────────────────────────
const SECTION_LABEL_FONT_LOGICAL_PX: f32 = 10.5;
/// The 10.5px line box, measured in the mock-up's own renderer (Inter at
/// `line-height: normal`) — 12.5px, the same ladder its 11px group label climbs
/// at 13px and its 13px row at 15.5px.
const SECTION_LABEL_LINE_LOGICAL_PX: f32 = 12.5;
/// `letter-spacing: .05em` at `font-weight: 600` — the settings dialog's
/// `.group-label` craft, which is the same heading in a different surface.
const SECTION_LABEL_TRACKING_EM: f32 = 0.05;
/// `padding: 3px 10px 5px` — top, both sides, bottom.
const SECTION_LABEL_PADDING_TOP_LOGICAL_PX: f32 = 3.0;
const SECTION_LABEL_PADDING_X_LOGICAL_PX: f32 = 10.0;
const SECTION_LABEL_PADDING_BOTTOM_LOGICAL_PX: f32 = 5.0;
/// `Recently opened` under `text-transform: uppercase`.
///
/// The transform is a *rendering* of the heading, and this pipeline has no
/// transform: a chrome label draws the string it is given. So the string it is
/// given is the drawn one, and the mock-up's own casing lives in the doc line
/// above rather than in a lowercase constant nothing would uppercase.
const RECENT_SECTION_LABEL: &str = "RECENTLY OPENED";

// ── `.recent-item` (mock-up lines 1030-1031) ───────────────────────────────
/// `max-width: 260px`.
///
/// It is a real clamp on the row's box and it cannot bind today: the menu is
/// [`MENU_MIN_WIDTH_LOGICAL_PX`] wide and nothing here measures text, so every
/// row is already 170px of content. In the mock-up the menu is content-sized
/// (`min-width: 180px` over `white-space: nowrap` rows) and this is what stops
/// one long path from stretching the popup across the window — the day this
/// module can measure a string, that growth and the ellipsis at mock-up 1031
/// arrive together, and the clamp is already where it belongs.
const RECENT_ITEM_MAX_WIDTH_LOGICAL_PX: f32 = 260.0;

/// A profile the picker can start a tab from.
///
/// The mock-up's own four (`const PROFILES`, line 2598): PowerShell, WSL, Git
/// Bash, Command Prompt, in that order, because the order **is** the index and
/// `state.defaultProfile` is a number into it.
///
/// Four fixed entries rather than a discovery pass over the machine (user ruling
/// 2026-08-10, Q1). The alternative — Windows Terminal's dynamic profiles, where
/// a profile exists only if its shell is installed — answers a different
/// question than the one this list is asked: the list is the *product's* offer,
/// and which of them this machine can honour is a fact about the machine.
/// Discovery is not skipped, it is **separated**: [`ProfilePrograms`] probes for
/// each one's executable and a profile it cannot find is drawn greyed rather
/// than dropped. That is the honest form of "you do not have this", and it is
/// the one a hidden row cannot say — a row that is missing looks exactly like a
/// row that was never designed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    /// The name a seed keeps this profile by — `docs/DESIGN.md` §7.1.4 requires a
    /// "**稳定 profile_id**（不是标题、不是展示对象）".
    ///
    /// It is deliberately not [`Self::title`]: a title is a display object, and
    /// display objects get renamed, localised and reworded. A seed keyed on one
    /// would stop matching its own profile the day the strip's wording changed,
    /// and the tab would come back as somebody else. It is not the executable
    /// path either — that is what the shell *is*, not which profile chose it, and
    /// two profiles can legitimately launch the same binary.
    ///
    /// **Ruling 2026-08-10 (Q3), and it overturns a written spec.**
    /// `docs/M2-persistence-schema-v1.md` §3.3 says the v1 transitional value is
    /// "启动该 pane 时实际使用的 shell 可执行路径" — a normalized executable path
    /// — while this build has always written `"pwsh"`, so both spellings exist on
    /// real disks. The slug wins: a path is not stable (`pwsh.exe` moves between
    /// `%ProgramFiles%` and the Store alias without the user changing anything,
    /// and `BT_SHELL` moves it anywhere), and it is not an identity (two profiles
    /// may legitimately run one binary — which is precisely what the pwsh profile
    /// and a future "pwsh with different arguments" profile would do). The paths
    /// already on disk are therefore *historical values to be migrated*, which is
    /// `migrate_session_v5_to_v6`'s whole job.
    pub id: &'static str,
    pub title: &'static str,
    /// A profile's icon is its mark, not a letter that happens to be in its
    /// prompt — the mock-up says so in as many words at `const mark`.
    pub mark: ChromeMark,
    /// How this profile's program is found on the machine it is running on.
    pub program: ProgramSource,
    /// The arguments the profile always passes, ahead of nothing else — there is
    /// no user-supplied argument list yet (that is the profile editor's, K86).
    ///
    /// `-NoLogo` lives here now, and that is the point of the field. It used to
    /// be welded into `PtyCommand::interactive_shell` as "the one argument, hard
    /// coded", which was true only while every shell this terminal could start
    /// was a PowerShell. It is a PowerShell flag: `cmd.exe` would take it as the
    /// name of a batch file to run, and `bash` as a filename to open.
    pub args: &'static [&'static str],
    /// Where a leaf of this profile opens when nothing else says.
    ///
    /// The mock-up has no such field: it has one `HOME` constant (line 2632) that
    /// every profile shares, because every one of its profiles is a fiction that
    /// never starts a process. A real one has to say *whose* home, and the answer
    /// is not the same kind of thing for all four — see [`StartingDir`].
    ///
    /// It is a fixed property of the profile rather than something the user can
    /// edit, and that is this ticket's boundary, not an opinion about the feature:
    /// editing it belongs to the profile editor (K86, → the Settings extension
    /// block) along with the program, the arguments and the environment. What is
    /// owed now is that the slot exists and is *read*, so that the editor is a
    /// screen over a working mechanism rather than a screen and a mechanism.
    pub starting_dir: StartingDir,
    /// Which shell-integration script this profile is served by, if any.
    pub integration: Integration,
}

/// Where a profile's shell stands when it is not told.
///
/// Two shapes, because "home" is not one fact here. Three of these profiles run
/// as Windows processes and take their starting directory the way every Windows
/// process does — as a working directory handed to `CreateProcess`. WSL's shell
/// does not: `wsl.exe` is a *launcher*, its working directory is a Windows path
/// that the distribution sees through `/mnt`, and the Linux home it should open
/// in has no Windows spelling at all. Handing `C:\Users\Weiyi` to a WSL tab lands
/// it in `/mnt/c/Users/Weiyi` — a real directory, and not the one a shell opens
/// in when you start it yourself.
///
/// So the enum carries the *form* the answer takes rather than a path. That is
/// what keeps this from being a special case bolted onto the spawn path: a
/// profile states how it is told where to start, and the spawn reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartingDir {
    /// `%USERPROFILE%` — the Windows home, handed over as a working directory.
    ///
    /// The variable rather than a composed `C:\Users\<name>`: a roaming or
    /// redirected profile lives elsewhere and the variable is the only thing that
    /// knows it.
    WindowsHome,
    /// The shell's own `$HOME`, reached by *asking the launcher for it* — these
    /// arguments, appended when there is no directory to hand over instead.
    ///
    /// `wsl.exe --cd ~` is the whole of it, and it is the documented flag rather
    /// than a trick: verified on this machine to answer `/home/weiyi` from a
    /// process standing in `D:\`, where the same launcher with no flag answers
    /// `/mnt/d`.
    ///
    /// Appended *instead of* a working directory and never beside one, because
    /// `--cd` overrides the inherited directory: a profile that always passed it
    /// could never be started anywhere else, which is exactly the inheritance P5
    /// is going to want back once WSL's two path namespaces have a translation.
    LauncherFlag(&'static [&'static str]),
}

/// How a profile's executable is located on the machine.
///
/// Two shapes rather than one because they answer to different authorities.
/// [`Self::DefaultShell`] defers to a resolution *order* that is already ruled
/// and already tested (`bt_pty::resolve_default_shell`: `BT_SHELL`, then a
/// `pwsh` probe, then `powershell.exe`); [`Self::FirstOf`] is a list of places
/// to look, in order, for a program that either is on this machine or is not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramSource {
    /// `bt_pty::resolve_default_shell`'s answer — `BT_SHELL` first (ruling
    /// 2026-08-10, Q4: the override is **kept**, as a development back door, and
    /// it covers this profile alone rather than becoming a fifth profile's
    /// worth of configuration), then PowerShell 7, then Windows PowerShell.
    ///
    /// Never unavailable, and that is a fact about Windows rather than an
    /// optimism: `powershell.exe` is part of the OS, so the last step of that
    /// chain always answers. It is what makes [`FALLBACK_PROFILE`] safe to fall
    /// back to.
    DefaultShell,
    /// The first of these that is a real file, in order.
    FirstOf(&'static [ProgramCandidate]),
}

/// One place to look for a profile's executable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramCandidate {
    /// `%VARIABLE%\tail` — an environment variable and a path under it.
    ///
    /// Never a bare relative path: "wherever this process happens to be
    /// standing" is not a place a shell lives.
    Under {
        variable: &'static str,
        tail: &'static str,
    },
    /// Find `anchor` on `PATH`, climb out of the directory it was found in, and
    /// take `tail` from there.
    ///
    /// The general answer to "installed somewhere we did not think of", and it
    /// is not a guess: it reads the install the user is *already using*. A
    /// well-known-paths list can only ever enumerate the installers' defaults,
    /// and the one thing a person who changed the install directory has
    /// certainly done is put the tool on their `PATH` — so the tool itself is
    /// the most reliable landmark its siblings have.
    ///
    /// Concretely, for Git for Windows: `git.exe` lives at `<root>\cmd\git.exe`
    /// and `bash.exe` at `<root>\bin\bash.exe`, so the anchor's *parent's*
    /// parent is the root both hang off. Climbing one directory rather than
    /// joining onto the anchor's own is what makes this work for a layout where
    /// the two are siblings rather than nested.
    BesideOnPath {
        anchor: &'static str,
        tail: &'static str,
    },
}

/// Which shell-integration script a profile is served by.
///
/// A slot rather than a mechanism. `scripts/shell-integration/betterterminal.ps1`
/// is **opt-in and manual** — the user dot-sources it into their own `$PROFILE`
/// and this product never injects it (`docs/shell-integration.md` §83-96) — so
/// there is nothing here for spawn to do today, and this ticket adds nothing.
/// What the field records is which profiles *have* a script at all, which is the
/// question the honest-capability matrix (P7) and the bash/cmd injection tickets
/// (P5/P6) both start from.
///
/// The three profiles without one are not degraded by a special case: a shell
/// that never emits OSC 133 keeps the cursor/WRAPLINE heuristics, and one that
/// never emits OSC 7 leaves the relative path undetected rather than guessing a
/// directory. Both are the existing, already-implemented conventions
/// (`docs/shell-integration.md` §34-35 and §111-115) — this ticket confirms they
/// hold for the new shells rather than inventing a second set for them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Integration {
    /// `betterterminal.ps1`, dot-sourced by the user into `$PROFILE`.
    PowerShellOptIn,
    /// No script exists for this profile yet.
    None,
}

pub const PROFILES: [Profile; 4] = [
    Profile {
        id: "pwsh",
        title: "PowerShell",
        mark: ChromeMark::ProfilePowerShell,
        program: ProgramSource::DefaultShell,
        // The flag this terminal has always passed, now said by the profile that
        // means it rather than by the spawn path every profile goes through.
        args: &["-NoLogo"],
        starting_dir: StartingDir::WindowsHome,
        integration: Integration::PowerShellOptIn,
    },
    Profile {
        id: "wsl",
        // The mock-up writes `WSL · Ubuntu`, and this build writes `WSL`.
        //
        // The qualifier after the `·` is a **discovery claim**, and this ticket
        // does not discover: `wsl.exe` with no arguments starts whatever the
        // user's *default* distribution is, which on this machine may be Ubuntu
        // and on the next is Debian or Alpine. Printing "Ubuntu" over a command
        // that will start Debian is the same failure as a greyed row that was
        // hidden instead — chrome saying something it did not check. Naming the
        // distribution needs `wsl.exe -l -q`'s answer, which is P5's, and the
        // `·` qualifier arrives with it (and with it the mock-up's own rule at
        // line 4013 that a session's name drops everything from the `·` on).
        title: "WSL",
        mark: ChromeMark::ProfileUbuntu,
        program: ProgramSource::FirstOf(&[ProgramCandidate::Under {
            variable: "SystemRoot",
            tail: r"System32\wsl.exe",
        }]),
        args: &[],
        // The one profile whose home is not a Windows directory.
        starting_dir: StartingDir::LauncherFlag(&["--cd", "~"]),
        integration: Integration::None,
    },
    Profile {
        id: "gitbash",
        title: "Git Bash",
        mark: ChromeMark::ProfileGit,
        // Git for Windows lands in more places than a list can enumerate — the
        // same shape of problem `find_pwsh` already solves for PowerShell 7,
        // and the same answer: probe rather than assume.
        //
        // `git.exe` on `PATH` is tried **first**, and it is the only candidate
        // that generalises. The three paths under it are the system-wide, the
        // 32-bit and the per-user installers' *defaults*, which between them
        // still miss everyone who chose their own install directory — a case
        // this project met on the very first machine it was tested on, where
        // Git sits on another drive entirely. Somebody who moved the install has
        // certainly put `git` on their path, so the tool is the landmark its own
        // shell is found by.
        program: ProgramSource::FirstOf(&[
            ProgramCandidate::BesideOnPath {
                anchor: "git.exe",
                tail: r"bin\bash.exe",
            },
            ProgramCandidate::Under {
                variable: "ProgramFiles",
                tail: r"Git\bin\bash.exe",
            },
            ProgramCandidate::Under {
                variable: "ProgramFiles(x86)",
                tail: r"Git\bin\bash.exe",
            },
            ProgramCandidate::Under {
                variable: "LocalAppData",
                tail: r"Programs\Git\bin\bash.exe",
            },
        ]),
        // `bin\bash.exe` is the MSYS wrapper the Git Bash shortcut itself runs,
        // and `--login -i` is that shortcut's own argument list: `--login` is
        // what sources `/etc/profile` and puts `git` on the path, and without it
        // this would be a bash that cannot find the tool it is named after.
        args: &["--login", "-i"],
        // Git for Windows' MSYS layer maps `$HOME` onto `%USERPROFILE%` by
        // default, so the Windows home *is* this shell's home — one directory
        // under two spellings, unlike WSL's two directories.
        starting_dir: StartingDir::WindowsHome,
        integration: Integration::None,
    },
    Profile {
        id: "cmd",
        title: "Command Prompt",
        mark: ChromeMark::ProfileCmd,
        program: ProgramSource::FirstOf(&[ProgramCandidate::Under {
            variable: "SystemRoot",
            tail: r"System32\cmd.exe",
        }]),
        // None. `cmd.exe` has no logo to suppress, and every switch it does take
        // (`/c`, `/k`) would end the session rather than start one.
        args: &[],
        starting_dir: StartingDir::WindowsHome,
        integration: Integration::None,
    },
];

/// The profile everything falls back **to**, and the one thing in this module
/// that is not a choice.
///
/// It used to be called `DEFAULT_PROFILE` and it used to be both things at once:
/// the floor a broken `profile_id` lands on, *and* the profile the `+` starts.
/// P3 makes the second one a setting, and the two have to come apart before that
/// setting exists — a name that means "what the user picked" and "what we do when
/// nothing was picked" is a name that will be read as the wrong one of the two by
/// whoever touches it next.
///
/// What it must satisfy is one property: it is [`ProgramSource::DefaultShell`],
/// whose last resolution step is `powershell.exe`, which is part of Windows. So
/// it always starts, which is what makes it safe to be the end of every fallback
/// chain — see [`the_fallback_profile_can_always_be_started`]. The *user's*
/// default carries no such guarantee (they can uninstall Git after choosing Git
/// Bash), which is precisely why [`default_profile`] resolves through here.
pub const FALLBACK_PROFILE: usize = 0;

/// Which profile the `+`, `Ctrl+Shift+N` and the opening window start from —
/// `state.defaultProfile` (mock-up 3217), resolved for this machine.
///
/// `stored` is `settings.json`'s `default_profile`, an id and not an index
/// (`bt_persist::SettingsV1::default_profile` says why). Three inputs collapse to
/// one answer here rather than at each of the four call sites, because four
/// readings of "the default" is how three of them end up meaning something
/// slightly different:
///
/// * an id naming no profile in this build — including the empty id a user who
///   has never opened the setting has — is [`FALLBACK_PROFILE`], which is
///   [`index_of_id`]'s rule and not a second one;
/// * an id naming a profile this machine cannot start is **also**
///   [`FALLBACK_PROFILE`], and this is the part `index_of_id` cannot do because
///   it is a fact about the machine rather than about the file. Someone who chose
///   Git Bash and then uninstalled Git must still get a window;
/// * anything else is what they chose.
///
/// The stored id is *not* rewritten when it degrades. Uninstalling Git must not
/// quietly consume the answer "Git Bash", or reinstalling it would leave the
/// user's own choice erased with nothing to say so — the degradation lives for
/// exactly as long as its cause.
#[must_use]
pub fn default_profile(stored: &str, programs: &ProfilePrograms) -> usize {
    PROFILES
        .iter()
        .position(|profile| profile.id == stored)
        .filter(|index| programs.is_available(*index))
        .unwrap_or(FALLBACK_PROFILE)
}

/// Which profile a seed's `profile_id` names, or [`FALLBACK_PROFILE`] when the
/// file names one this build does not have.
///
/// Falling back rather than refusing is the schema's own rule — `§5.4` 逐叶降级,
/// "未知 profile→默认": a profile that was removed (or that a newer build wrote)
/// must cost you that tab's *shell choice*, never the tab. The place you were
/// standing is the part worth keeping, and it survives this.
///
/// It falls to the *fallback* and deliberately not to the user's configured
/// default, which is the one place those two answers visibly differ. The setting
/// says what it is for in the dialog's own words — "What opens on a new tab, and
/// when BetterTerminal starts" — and a leaf coming back off disk is neither. A
/// user who set their default to `cmd` and restores a session written by a build
/// that spelled a profile differently is owed the pane back, not every such pane
/// silently converted to their current preference; and the conversion would be
/// written to disk on the next save, so the original spelling could never be
/// recovered by a build that understood it again.
#[must_use]
pub fn index_of_id(id: &str) -> usize {
    PROFILES
        .iter()
        .position(|profile| profile.id == id)
        .unwrap_or(FALLBACK_PROFILE)
}

/// What a greyed row says when the pointer rests on it — the *why* behind the
/// grey, which the row itself has no room for.
///
/// The mock-up has no tooltip here to quote: its four profiles are always
/// startable, so it never had a greyed row to explain (user ruling: where the
/// mock-up is silent, rule and report). Its own convention decides the shape
/// anyway — a `title` on a menu row is the fact the row could not fit (7426/7430
/// put the full path on a Recent row captioned with only its leaf), so this is
/// the fact `not installed` could not fit.
///
/// The wording names **the profile and the machine**, in that order, and it is
/// chosen against two alternatives that read as bug reports. "Not found" alone
/// invites "where did you look?"; "BetterTerminal could not find Git Bash" makes
/// the terminal the subject of a sentence whose subject is the machine. `— not
/// found on this machine` says the search happened, that it was for a real thing,
/// and that the answer is about this computer rather than about the product.
#[must_use]
pub fn unavailable_tip(profile: usize) -> String {
    format!("{} — not found on this machine", PROFILES[profile].title)
}

/// How a profile is told where to start, once the machine has been asked.
///
/// The two shapes of [`StartingDir`] resolved into the two things a spawn can
/// actually do with them, plus the honest third answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartingPlace {
    /// Hand this over as the process's working directory.
    Directory(PathBuf),
    /// Append these to the profile's own arguments, and hand over no directory.
    Arguments(&'static [&'static str]),
    /// This machine cannot name the place. The shell starts where a process
    /// started by this one starts, which is what every leaf did before this
    /// field existed — an unchanged answer rather than a guessed one.
    Unstated,
}

/// Where a leaf of `profile` opens when nothing else has said — the resolution
/// of [`Profile::starting_dir`] against this machine.
///
/// Read at spawn rather than probed once like [`ProfilePrograms`], and the
/// difference is the rate: availability is asked by the *paint*, four times a
/// frame for as long as a menu is open, while this is asked once per shell
/// started. A value cached for the life of the process would be trading nothing
/// for a home directory that cannot then follow a `%USERPROFILE%` the user
/// changed under us.
#[must_use]
pub fn starting_place(profile: usize, environment: &dyn ShellEnvironment) -> StartingPlace {
    match PROFILES[profile].starting_dir {
        StartingDir::WindowsHome => environment
            .var_os("USERPROFILE")
            .map(PathBuf::from)
            .map_or(StartingPlace::Unstated, StartingPlace::Directory),
        StartingDir::LauncherFlag(arguments) => StartingPlace::Arguments(arguments),
    }
}

/// The first directory of `PATH` holding `file_name`, joined.
///
/// `std::env::split_paths` only parses an already-fetched `PATH` string — it
/// touches neither the real environment nor the real filesystem — so this stays
/// a pure function of whatever `environment` reports, which is what lets a test
/// hand it an imaginary machine.
fn search_path(environment: &dyn ShellEnvironment, file_name: &str) -> Option<PathBuf> {
    let path = environment.var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(file_name))
        .find(|candidate| environment.is_file(candidate))
}

/// Which executable each profile resolves to **on this machine**, probed once.
///
/// Once, and that is the whole reason this is a value rather than a function.
/// Availability is a filesystem question, the picker asks it of every row it
/// draws, and the picker is redrawn on every frame it is open — a probe called
/// from the paint would put four `is_file` calls on the pointer's path at
/// whatever rate the screen refreshes. It is also a question whose answer must
/// not change *while the menu is open*: a row that greys out between the frame
/// you read it on and the click you aimed at it is a worse answer than a stale
/// one.
///
/// The environment is injected for the reason `bt_pty::shell`'s already is:
/// otherwise every test of this module would be a test of what happens to be
/// installed on the machine running it, and "Git Bash is greyed" would pass on
/// the build server and fail on the developer's laptop for the same code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfilePrograms {
    resolved: [Option<OsString>; PROFILES.len()],
}

impl ProfilePrograms {
    /// Ask the machine, once, what each profile would start.
    #[must_use]
    pub fn probe(environment: &dyn ShellEnvironment) -> Self {
        Self {
            resolved: std::array::from_fn(|index| match PROFILES[index].program {
                // Always answers: the last step of that chain is
                // `powershell.exe`, which is part of the OS.
                ProgramSource::DefaultShell => Some(resolve_default_shell(environment).program),
                ProgramSource::FirstOf(candidates) => candidates
                    .iter()
                    .filter_map(|candidate| Self::candidate_path(*candidate, environment))
                    .find(|candidate| environment.is_file(candidate))
                    .map(PathBuf::into_os_string),
            }),
        }
    }

    /// The program this profile would start, or `None` when this machine has
    /// nowhere to start it from.
    #[must_use]
    pub fn program(&self, profile: usize) -> Option<&OsStr> {
        self.resolved.get(profile)?.as_deref()
    }

    /// Where one candidate says to look, or `None` when the machine cannot even
    /// name the place — an environment variable that is unset, or an anchor that
    /// is nowhere on `PATH`.
    ///
    /// Naming a place is not finding a file there; the caller still probes.
    fn candidate_path(
        candidate: ProgramCandidate,
        environment: &dyn ShellEnvironment,
    ) -> Option<PathBuf> {
        match candidate {
            ProgramCandidate::Under { variable, tail } => {
                Some(Path::new(&environment.var_os(variable)?).join(tail))
            }
            ProgramCandidate::BesideOnPath { anchor, tail } => {
                let found = search_path(environment, anchor)?;
                // `<root>\cmd\git.exe` -> `<root>\cmd` -> `<root>` -> `<root>\bin\bash.exe`.
                Some(found.parent()?.parent()?.join(tail))
            }
        }
    }

    /// Whether this profile can do what its row says it does.
    ///
    /// The picker draws a profile it cannot start greyed rather than hiding it
    /// (user ruling 2026-08-10): the row is the product saying "this is a thing
    /// BetterTerminal opens", and the grey is it saying "not on this machine".
    /// Dropping the row conflates "you have not installed Git" with "we never
    /// thought of Git", and only one of those is something the user can act on.
    #[must_use]
    pub fn is_available(&self, profile: usize) -> bool {
        self.program(profile).is_some()
    }
}

/// Which row of the menu, and **what kind of row** — the two lists the picker
/// shows are indexed separately and a bare number cannot say which one it is
/// counting.
///
/// The tag is load-bearing rather than tidy. The menu used to be [`PROFILES`]
/// and nothing else, so a row index *was* a profile index and the two could be
/// the same integer; the moment a Recent section sits under the profiles, that
/// same integer names two different things, and the failure it produces is not
/// a panic but a silent one — clicking `~/repo · 3m ago` launching a plain
/// PowerShell in the wrong place, which looks like the menu working.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuRow {
    /// An index into [`PROFILES`]: start a new tab from this profile.
    Profile(usize),
    /// An index into the vault slice the menu was laid out from: revive this
    /// seed. It is the vault's own index, so [`crate::seed::SeedVault::take`]
    /// consumes it directly.
    Recent(usize),
}

/// Whether the picker is up, and which row the pointer is on.
///
/// App state and nothing else: not a seat, so the solver never sees it; not an
/// intent, so the session file never sees it. A menu that survived a restart
/// would be a window that opens mid-gesture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProfileMenu {
    open: bool,
    hover: Option<MenuRow>,
}

impl ProfileMenu {
    pub fn is_open(self) -> bool {
        self.open
    }

    /// The chevron: open when shut, shut when open. A control that opens
    /// something must also put it away — the mock-up learned that one the hard
    /// way, and its comment says so.
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.hover = None;
    }

    /// Shut it, and report whether there was anything to shut — which is what
    /// tells Esc and a press elsewhere whether they consumed anything.
    pub fn close(&mut self) -> bool {
        let was_open = self.open;
        self.open = false;
        self.hover = None;
        was_open
    }

    /// Returns whether the hover changed, so a caller can skip a repaint.
    pub fn set_hover(&mut self, hover: Option<MenuRow>) -> bool {
        let hover = if self.open { hover } else { None };
        let changed = self.hover != hover;
        self.hover = hover;
        changed
    }

    pub fn hover(self) -> Option<MenuRow> {
        self.hover
    }
}

/// Every rectangle the menu draws and hit-tests, in physical pixels of the whole
/// surface.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileMenuLayout {
    scale: f32,
    /// The menu's border box.
    frame: [f32; 4],
    /// One row per entry of [`PROFILES`], top to bottom.
    items: Vec<[f32; 4]>,
    /// `.menu-sep`'s 1px rule, or `None` when there is nothing to separate.
    ///
    /// The three Recent boxes are `Option`/empty together and never singly:
    /// mock-up 7424 is one ternary over `state.recent.length`, and a heading
    /// over an empty list is a promise the menu cannot keep.
    separator: Option<[f32; 4]>,
    /// `.menu-label`'s band, padding included.
    section_label: Option<[f32; 4]>,
    /// One row per vault entry the menu shows, newest first.
    recent: Vec<[f32; 4]>,
}

impl ProfileMenuLayout {
    /// Every row that has something to say beyond its own caption, paired with
    /// the box it says it over.
    ///
    /// The mock-up puts a `title` on a menu row exactly when the row is showing
    /// less than it knows: 7426 and 7430 caption a Recent row with the last
    /// segment of a path and hang the whole path off it. This is that rule, plus
    /// the one row the mock-up never had — a profile this machine cannot start,
    /// captioned `not installed`, which is a state without its reason.
    ///
    /// It is one iterator rather than a tooltip block beside the draw, because
    /// the rectangles are the *laid-out* ones: a tip registered against a box
    /// computed a second way is a tip that appears where the row is not. An
    /// available profile row yields nothing — its caption already is everything
    /// the row knows, and a tip that restates the label under the pointer is the
    /// noise `hideTip` exists to prevent.
    pub fn tips<'a>(
        &'a self,
        programs: &'a ProfilePrograms,
        recent: &'a [RecentEntry],
    ) -> impl Iterator<Item = (MenuRow, [f32; 4], String)> + 'a {
        let profiles = self
            .items
            .iter()
            .enumerate()
            .filter(|(index, _)| !programs.is_available(*index))
            .map(|(index, rect)| (MenuRow::Profile(index), *rect, unavailable_tip(index)));
        let recents =
            self.recent
                .iter()
                .zip(menu_rows(recent))
                .enumerate()
                .map(|(index, (rect, entry))| {
                    (
                        MenuRow::Recent(index),
                        *rect,
                        match &entry.seed {
                            Seed::Term { cwd, .. } => cwd.clone(),
                            Seed::Files { root } => root.clone(),
                        },
                    )
                });
        profiles.chain(recents)
    }
}

/// What the menu shows of a vault: its first [`RECENT_CAPACITY`] entries.
///
/// The cap is the vault's own (`docs/DESIGN.md` §7.1.4, mock-up 4106) and not a
/// second policy invented here — but it is applied here too, because a menu is
/// a surface with a window edge under it and "however many the caller passed"
/// is not a height. Both [`layout`] and [`build`] read the slice through this,
/// so the rectangles and the rows drawn into them cannot disagree.
fn menu_rows(recent: &[RecentEntry]) -> &[RecentEntry] {
    &recent[..recent.len().min(RECENT_CAPACITY)]
}

/// Which way the menu hangs off the button that opened it.
///
/// `openProfileMenu` (mock-up 7409-7457) needs no such choice: it writes `top:
/// a.bottom + 4; left: a.left` off whatever element was clicked, and in a
/// document that is right for both layouts for free, because both chevrons are
/// real boxes and a menu below either one has the whole page to fall into.
///
/// This window is not a page. Below-and-left of a *rail* button is the rail's
/// own column — 46px of it while the rail is parked — so the menu would be laid
/// down the sidebar it was opened from. A vertical strip keeps its free space to
/// the side, which is the same reason [`crate::peek_strip::PeekSide`] exists and
/// the same answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuSide {
    /// Under the button, sharing its left edge. The horizontal strip.
    Below,
    /// To the right of the button, aligned with its top. The vertical rail.
    Beside,
}

/// The menu hung off `anchor` — the `˅`'s own box, in physical pixels — inside
/// a surface this big, showing `recent` under the profiles.
///
/// No clock is read here and none is passed: how long ago a seed was closed is
/// a fact about the moment it is *drawn*, so it belongs to [`build`], and a
/// layout that took the time would change shape between two frames of one open
/// menu.
#[must_use]
pub fn layout(
    anchor: [f32; 4],
    side: MenuSide,
    surface: (f32, f32),
    scale: f32,
    recent: &[RecentEntry],
) -> ProfileMenuLayout {
    let px = |value: f32| value * scale;
    let recent = menu_rows(recent);
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let padding = px(MENU_PADDING_LOGICAL_PX);
    let item_height = px(ITEM_HEIGHT_LOGICAL_PX).round();
    let separator_thickness = (SEPARATOR_THICKNESS_LOGICAL_PX * scale).round().max(1.0);
    let separator_margin = px(SEPARATOR_MARGIN_Y_LOGICAL_PX).round();
    // `margin: 5px 0` above and below the rule, and nothing to collapse against:
    // a row carries no vertical margin of its own.
    //
    // Every term here is a whole number of device pixels, and that is what makes
    // the section *additive*: the menu's height is the rounded sum, so a section
    // measured in whole pixels adds exactly its own height to it rather than a
    // pixel more or less depending on where the fraction under it happened to
    // sit.
    let separator_block = 2.0 * separator_margin + separator_thickness;
    let section_block = px(SECTION_LABEL_PADDING_TOP_LOGICAL_PX
        + SECTION_LABEL_LINE_LOGICAL_PX
        + SECTION_LABEL_PADDING_BOTTOM_LOGICAL_PX)
    .round();
    let recent_block = if recent.is_empty() {
        0.0
    } else {
        separator_block + section_block + item_height * recent.len() as f32
    };

    let width = px(MENU_MIN_WIDTH_LOGICAL_PX).round();
    let height =
        (2.0 * (border + padding) + item_height * PROFILES.len() as f32 + recent_block).round();
    let (surface_width, surface_height) = surface;
    let edge = px(MENU_EDGE_MARGIN_LOGICAL_PX);
    let (left, top) = match side {
        // `menu.style.top = a.bottom + 4; menu.style.left = Math.min(a.left,
        // win.width - mw - 8)` — the mock-up's own two lines.
        MenuSide::Below => (
            anchor[0].min(surface_width - width - edge).max(0.0).round(),
            (anchor[3] + px(MENU_OFFSET_LOGICAL_PX)).round(),
        ),
        // The same four pixels turned through a right angle. The rail's `˅`
        // stands beside its `+` when the panel is open and collapses to nothing
        // when it is parked (Q181), so the box handed in here is the chevron's
        // in one state and the `+`'s in the other — and "clear of its right
        // edge, level with its top" is the one placement that reads the same for
        // both, because the two share that edge and that top by construction.
        MenuSide::Beside => (
            (anchor[2] + px(MENU_OFFSET_LOGICAL_PX))
                .min(surface_width - width - edge)
                .max(0.0)
                .round(),
            anchor[1]
                .min(surface_height - height - edge)
                .max(0.0)
                .round(),
        ),
    };
    let frame = [left, top, left + width, top + height];

    let content_left = frame[0] + border + padding;
    let content_right = frame[2] - border - padding;
    let mut cursor = frame[1] + border + padding;
    let mut items = Vec::with_capacity(PROFILES.len());
    for _ in 0..PROFILES.len() {
        items.push([content_left, cursor, content_right, cursor + item_height]);
        cursor += item_height;
    }
    let (separator, section_label, recent_rows) = if recent.is_empty() {
        (None, None, Vec::new())
    } else {
        let separator = [
            content_left,
            cursor + separator_margin,
            content_right,
            cursor + separator_margin + separator_thickness,
        ];
        cursor += separator_block;
        let section_label = [content_left, cursor, content_right, cursor + section_block];
        cursor += section_block;
        // `.recent-item { max-width: 260px }` — see the constant: a clamp that
        // cannot bind while the menu keeps its min-width, and the right place
        // for it the day the menu is content-sized.
        let recent_right = content_right.min(content_left + px(RECENT_ITEM_MAX_WIDTH_LOGICAL_PX));
        let mut rows = Vec::with_capacity(recent.len());
        for _ in recent {
            rows.push([content_left, cursor, recent_right, cursor + item_height]);
            cursor += item_height;
        }
        (Some(separator), Some(section_label), rows)
    };

    ProfileMenuLayout {
        scale,
        frame,
        items,
        separator,
        section_label,
        recent: recent_rows,
    }
}

/// What a point is over: a row and which list it belongs to, `Some(None)` for
/// the menu's own body between and around its rows, and `None` for anywhere else
/// in the window.
///
/// The two negatives are different answers and the difference is the whole of
/// what "popup" means here: a press on the body is the menu's and does nothing,
/// a press outside it belongs to whatever is there and merely closes the menu on
/// its way past.
///
/// The separator and the heading are body, not rows — they are the two things in
/// the menu that name nothing you can open.
///
/// **A row this machine cannot start is body too**, and that is the whole
/// enforcement of the greying: it is answered here, at the one place both the
/// hover and the click read, rather than at each of them. A rule spelled at the
/// click would light the row under the pointer and then do nothing when pressed,
/// which is a menu lying about what it is about to do; a rule spelled at the
/// hover would leave the row dark and still open a tab. Neither half is
/// separately correct, so neither half is separately written.
#[must_use]
pub fn hit(
    layout: &ProfileMenuLayout,
    programs: &ProfilePrograms,
    recent: &[RecentEntry],
    x: f64,
    y: f64,
) -> Option<Option<MenuRow>> {
    let (x, y) = (x as f32, y as f32);
    for (index, item) in layout.items.iter().enumerate() {
        if contains(*item, x, y) {
            return Some(
                programs
                    .is_available(index)
                    .then_some(MenuRow::Profile(index)),
            );
        }
    }
    for (index, (row, entry)) in layout.recent.iter().zip(menu_rows(recent)).enumerate() {
        if contains(*row, x, y) {
            return Some(
                recent_is_available(&entry.seed, programs).then_some(MenuRow::Recent(index)),
            );
        }
    }
    contains(layout.frame, x, y).then_some(None)
}

/// Whether the shell a Recent row would revive can be started on this machine.
///
/// Asked of Recent rows and not only of profile rows, because the row above and
/// the row below are the same offer: `~/repo · 3m ago` under a Git mark is
/// "start Git Bash here", and if the profile row that says `Git Bash` is greyed
/// then this one has to be too. Greying one and not the other would put, in one
/// menu, both answers to the same question.
///
/// A files locus has no shell, so nothing about it can be missing.
fn recent_is_available(seed: &Seed, programs: &ProfilePrograms) -> bool {
    match seed {
        Seed::Term { profile_id, .. } => programs.is_available(index_of_id(profile_id)),
        Seed::Files { .. } => true,
    }
}

fn contains(rect: [f32; 4], x: f32, y: f32) -> bool {
    x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]
}

/// The menu's three planes and its rows, as one overlay layer.
///
/// One layer and not more: a popup with nothing of its own inside it has nothing
/// to cover but the window, and the window is not the overlay's to draw. The
/// stack exists so a surface can cover another surface the overlay drew — see
/// [`crate::settings::build`], where the picker is a second layer over the dialog
/// it hangs off.
#[must_use]
pub fn build(
    layout: &ProfileMenuLayout,
    programs: &ProfilePrograms,
    default: usize,
    hover: Option<MenuRow>,
    recent: &[RecentEntry],
    now: SystemTime,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> Vec<OverlayLayer> {
    let palette = chrome_palette();
    let scale = layout.scale;
    let px = |value: f32| value * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
    // `.default-hint { margin-left: auto }` is a flex item, so in the mock-up it
    // takes its own width out of the row before the name gets any — and the name
    // is what shrinks (line 1031 puts `text-overflow: ellipsis` on the name span
    // and not on the hint). Measuring it is how that becomes true here: the row
    // is 180px, `Command Prompt` and `default` do not both fit in it, and until
    // this was measured the two were drawn into overlapping boxes and printed on
    // top of each other. Caller-measured for the reason every other surface's
    // text is (`restore`, `peek_strip`, `settings`): the font is the renderer's.
    let hint_font = px(HINT_FONT_LOGICAL_PX);
    let mut hint = |text: String| {
        let width = measure(&text, hint_font);
        (text, width)
    };
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let mut quads = Vec::new();
    let mut labels = Vec::new();
    let mut sprites = Vec::new();

    push_float_window(
        &mut quads,
        layout.frame,
        px(MENU_RADIUS_LOGICAL_PX),
        border,
        px(FLOAT_WINDOW_SHADOW_LOGICAL_PX),
        palette.menu_surface,
        palette.menu_shadow,
        alpha(palette.menu_popup_shadow_inner_alpha),
        alpha(palette.menu_popup_shadow_outer_alpha),
        palette.menu_border,
        alpha(palette.menu_border_alpha),
    );

    for (index, item) in layout.items.iter().enumerate() {
        let profile = PROFILES[index];
        let available = programs.is_available(index);
        push_row(
            &Row {
                rect: *item,
                mark: profile.mark,
                name: profile.title,
                // `margin-left: auto` puts the hint hard against the row's
                // trailing padding, and it names a fact about the profile rather
                // than the row's state — so it does not answer to hover.
                //
                // The two annotations are exclusive by construction rather than
                // by an `if/else` that could one day pick wrong: `default` is
                // resolved through [`default_profile`], which refuses to answer
                // with a profile this machine cannot start.
                hint: if available {
                    (index == default).then(|| hint(HINT_TEXT.to_owned()))
                } else {
                    Some(hint(UNAVAILABLE_HINT_TEXT.to_owned()))
                },
                hovered: hover == Some(MenuRow::Profile(index)),
                available,
            },
            scale,
            palette,
            &mut quads,
            &mut labels,
            &mut sprites,
        );
    }

    if let Some(rule) = layout.separator {
        quads.push(OverlayQuad {
            rect: rule,
            color: palette.menu_border,
            alpha: separator_alpha(palette.menu_border),
        });
    }

    if let Some(band) = layout.section_label {
        labels.push(ChromeLabel {
            text: RECENT_SECTION_LABEL.to_owned(),
            // The band's content box: padding stripped, so the 12.5px line box
            // is centred in exactly its own height and the 3px above it and 5px
            // below it stay the stylesheet's rather than the renderer's.
            rect: [
                band[0] + px(SECTION_LABEL_PADDING_X_LOGICAL_PX),
                band[1] + px(SECTION_LABEL_PADDING_TOP_LOGICAL_PX),
                band[2] - px(SECTION_LABEL_PADDING_X_LOGICAL_PX),
                band[3] - px(SECTION_LABEL_PADDING_BOTTOM_LOGICAL_PX),
            ],
            font_size_px: px(SECTION_LABEL_FONT_LOGICAL_PX),
            // `--ink3` over `--menu` — the same ink the row hints wear, because
            // it is the same declaration on the same surface.
            color: palette.menu_item_hint_text,
            align_right: false,
            align_center: false,
            letter_spacing_em: SECTION_LABEL_TRACKING_EM,
            weight: ChromeLabelWeight::SemiBold,
            tabular_numerals: false,
            clip: None,
        });
    }

    for (index, (row, entry)) in layout.recent.iter().zip(menu_rows(recent)).enumerate() {
        push_row(
            &Row {
                rect: *row,
                mark: recent_mark(&entry.seed),
                name: recent_label(&entry.seed),
                // Still the age, and deliberately not `not installed`: a Recent
                // row's one annotation answers "when", the grey already answers
                // "can you", and losing the timestamp would cost the row the
                // only thing that orders it against its neighbours.
                hint: Some(hint(ago_label(entry.at, now))),
                hovered: hover == Some(MenuRow::Recent(index)),
                available: recent_is_available(&entry.seed, programs),
            },
            scale,
            palette,
            &mut quads,
            &mut labels,
            &mut sprites,
        );
    }

    vec![OverlayLayer {
        quads,
        labels,
        sprites,
        ..Default::default()
    }]
}

/// One `.profile-item`, whichever list it belongs to.
///
/// The two lists are the same row — mock-up 7426/7430 is `class="profile-item
/// recent-item"`, and `.recent-item` adds a width and nothing else. So they are
/// drawn by one function rather than two that look alike, because the way two
/// menu rows drift apart is that somebody fixes the ink on one of them.
struct Row<'a> {
    rect: [f32; 4],
    mark: ChromeMark,
    name: &'a str,
    /// The `.default-hint` slot and **its measured width**: `default` on the
    /// default profile, `3m ago` on a recent row, `not installed` on one this
    /// machine cannot start.
    ///
    /// The width travels with the words because the two are used together and
    /// once: the hint is right-aligned into it and the name's box ends where it
    /// begins.
    hint: Option<(String, f32)>,
    hovered: bool,
    /// Whether this row can do what it says. A row that cannot is drawn and not
    /// offered — see [`hit`], which is where "not offered" is actually enforced.
    available: bool,
}

fn push_row(
    row: &Row<'_>,
    scale: f32,
    palette: ChromePalette,
    quads: &mut Vec<OverlayQuad>,
    labels: &mut Vec<ChromeLabel>,
    sprites: &mut Vec<ChromeSprite>,
) {
    let px = |value: f32| value * scale;
    let item = row.rect;
    if row.hovered {
        quads.extend(rounded_overlay_fill(
            item,
            px(ITEM_RADIUS_LOGICAL_PX),
            palette.menu_item_hover,
            1.0,
        ));
    }
    // The 15px mark centred on its own 14px column, which is what a flex box
    // does with a child one pixel wider than the box it is in.
    let column_left = item[0] + px(ITEM_PADDING_X_LOGICAL_PX);
    let column_right = column_left + px(ITEM_ICON_COLUMN_LOGICAL_PX);
    let mark = px(ITEM_MARK_LOGICAL_PX).round();
    let mark_left = ((column_left + column_right - mark) / 2.0).round();
    let mark_top = ((item[1] + item[3] - mark) / 2.0).round();
    let mut sprite = ChromeSprite::new(
        row.mark,
        [mark_left, mark_top, mark_left + mark, mark_top + mark],
        palette.accent,
    );
    if !row.available {
        sprite.opacity = UNAVAILABLE_MARK_OPACITY;
        sprite.grayscale = true;
    }
    sprites.push(sprite);
    // What the hint has already claimed, out of the row's trailing padding: its
    // own measured width, and the `gap: 10px` between two flex items. A row with
    // nothing to add gives the name the whole span, which is what every row did
    // before any of them had a hint long enough to collide.
    let hint_claim = row
        .hint
        .as_ref()
        .map_or(0.0, |(_, width)| width + px(ITEM_GAP_LOGICAL_PX));
    labels.push(ChromeLabel {
        text: row.name.to_owned(),
        // The name's box ends at the row's trailing padding, and the row's own
        // right edge is where `.recent-item`'s `max-width` already landed. A
        // `ChromeLabel` clips per glyph and per pixel, so a name too long for
        // that box is cropped exactly as CSS `overflow: hidden` crops it —
        // mock-up 1031 asks for `text-overflow: ellipsis` instead, and the `…`
        // needs a measured string this module is not given.
        rect: [
            column_right + px(ITEM_GAP_LOGICAL_PX),
            item[1],
            item[2] - px(ITEM_PADDING_X_LOGICAL_PX) - hint_claim,
            item[3],
        ],
        font_size_px: px(ITEM_FONT_LOGICAL_PX),
        // Three inks and one order of precedence. An unavailable row drops to
        // the hint's own `--ink3` — the menu's quietest ink, and already the one
        // this surface uses for text that reports rather than offers — and it
        // wins over hover because an unavailable row is never hovered anyway
        // (see [`hit`]); stating it first means the two cannot disagree if that
        // ever stops being true.
        color: if !row.available {
            palette.menu_item_hint_text
        } else if row.hovered {
            palette.menu_item_text_selected
        } else {
            palette.menu_item_text
        },
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: None,
    });
    if let Some((hint, _)) = &row.hint {
        labels.push(ChromeLabel {
            text: hint.clone(),
            rect: [
                item[0],
                item[1],
                item[2] - px(ITEM_PADDING_X_LOGICAL_PX),
                item[3],
            ],
            font_size_px: px(HINT_FONT_LOGICAL_PX),
            // `--ink3` over `--menu`. It used to be `dialog_muted_text`,
            // which is the same ink over `--win` — the settings dialog's
            // surface, not this one. Identical in the light theme, six levels
            // adrift in the dark.
            color: palette.menu_item_hint_text,
            align_right: true,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        });
    }
}

/// `--border-soft`'s alpha for the theme whose `--border` is drawn in `ink`.
///
/// White is the dark theme's hairline and black is the light theme's — the
/// palette's own convention, documented at `ChromePalette::menu_border`.
fn separator_alpha(ink: [u8; 3]) -> f32 {
    if ink == [0xff, 0xff, 0xff] {
        SEPARATOR_ALPHA_ON_DARK
    } else {
        SEPARATOR_ALPHA_ON_LIGHT
    }
}

/// The mark a recent row wears — mock-up 7427/7431.
///
/// A terminal seed wears **its own profile's** mark rather than a generic one:
/// the row is offering to reopen that shell, and the picker's rows one section
/// up are already teaching what the mark means. A files locus has no profile,
/// so it wears the folder the pane is (`#i-folder` in `--accent`, mock-up 7427).
fn recent_mark(seed: &Seed) -> ChromeMark {
    match seed {
        Seed::Term { profile_id, .. } => PROFILES[index_of_id(profile_id)].mark,
        Seed::Files { .. } => ChromeMark::Folder,
    }
}

/// What a recent row calls itself — mock-up 7431: `r.seed.name || cwdLeaf(r.seed)`.
///
/// Your own name for the tab wins, and the folder it stood in answers when you
/// never gave it one. An empty manual name is not a name: `||` in the mock-up
/// falls through an empty string, and a row captioned with nothing would be a
/// row you cannot tell from the one above it.
fn recent_label(seed: &Seed) -> &str {
    match seed {
        Seed::Term {
            cwd, manual_name, ..
        } => manual_name
            .as_deref()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| cwd_leaf(cwd)),
        // A files locus has no name of its own; the mock-up captions it with the
        // same leaf rule applied to its root.
        Seed::Files { root } => cwd_leaf(root),
    }
}

/// The last segment of a path, drive-root aware: `C:\` is `C:` and not the empty
/// string a naive split leaves behind the trailing separator.
///
/// **Duplicated** from `main.rs`'s `cwd_leaf`, deliberately and temporarily: that
/// one is the tab-title layer's, it takes a `&Path`, and `main.rs` is a binary
/// crate root that nothing can import from. The two must stay the same rule —
/// a Recent row that names a folder differently from the tab it reopens is the
/// same place under two names — so the day either moves, both move together.
fn cwd_leaf(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['\\', '/']);
    let leaf = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed);
    if leaf.is_empty() { trimmed } else { leaf }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    /// The one layer a popup with nothing inside it draws.
    fn one_layer(layers: Vec<OverlayLayer>) -> OverlayLayer {
        let [layer]: [OverlayLayer; 1] = layers
            .try_into()
            .expect("a popup with no popup of its own is one layer");
        layer
    }

    /// The `˅`'s box in a 960x600 window at 1x, taken from the strip's own
    /// geometry rather than restated here — one ordinary unpinned tab.
    fn anchor(scale: f32) -> [f32; 4] {
        let strip = [crate::seats::TabTrailer {
            pinned: false,
            reveal: 0.0,
        }];
        crate::seats::tab_strip_geometry(960.0 * scale, scale, &strip, 0, 0.0).new_tab_menu
    }

    /// A vault with nothing in it: the menu every test that predates Recent was
    /// written against.
    const NO_RECENT: &[RecentEntry] = &[];

    /// A stand-in for the renderer's own text measurement.
    ///
    /// A fixed advance per character rather than a real font: these tests are
    /// about *where the boxes end up given a width*, and a real font would make
    /// every one of them a claim about the machine's font stack. The advance is
    /// deliberately generous (0.6em is about right for Inter's digits and rather
    /// wider than its lowercase) so that a row's hint claims a realistic slice of
    /// a 180px menu.
    fn fake_measure(text: &str, font_px: f32) -> f32 {
        text.chars().count() as f32 * font_px * 0.6
    }

    /// An in-memory machine: what is on the `PATH`, and which files exist.
    ///
    /// The whole reason [`ProfilePrograms::probe`] takes a trait rather than
    /// reading `std::env` is here. "Git Bash is greyed out" is a claim about a
    /// machine, and a test that asked the *host* would pass on the build server
    /// and fail on a developer's laptop for identical code — the two would even
    /// disagree about which assertion was the bug.
    #[derive(Default)]
    struct FakeMachine {
        vars: std::collections::HashMap<String, OsString>,
        files: std::collections::HashSet<PathBuf>,
    }

    impl FakeMachine {
        fn with_var(mut self, key: &str, value: &str) -> Self {
            self.vars.insert(key.to_owned(), value.into());
            self
        }

        fn with_file(mut self, path: &str) -> Self {
            self.files.insert(PathBuf::from(path));
            self
        }

        /// A machine with all four shells on it, spelled the way a real Windows
        /// install spells them.
        fn fully_equipped() -> Self {
            Self::default()
                .with_var("SystemRoot", r"C:\WINDOWS")
                .with_var("ProgramFiles", r"C:\Program Files")
                .with_file(r"C:\WINDOWS\System32\wsl.exe")
                .with_file(r"C:\WINDOWS\System32\cmd.exe")
                .with_file(r"C:\Program Files\Git\bin\bash.exe")
        }
    }

    impl ShellEnvironment for FakeMachine {
        fn var_os(&self, key: &str) -> Option<OsString> {
            self.vars.get(key).cloned()
        }

        fn is_file(&self, path: &Path) -> bool {
            self.files.contains(path)
        }
    }

    /// The four profiles all startable — the machine most of these tests are
    /// about the *menu* on rather than about availability.
    fn equipped() -> ProfilePrograms {
        ProfilePrograms::probe(&FakeMachine::fully_equipped())
    }

    /// A bare Windows box: PowerShell and nothing else this product can start.
    fn bare() -> ProfilePrograms {
        ProfilePrograms::probe(&FakeMachine::default())
    }

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    /// The moment the menu is drawn in these tests. A fixed one: the ago labels
    /// are a function of two instants and neither of them is the wall clock.
    fn now() -> SystemTime {
        at(100_000)
    }

    fn term(cwd: &str, manual_name: Option<&str>, secs_ago: u64) -> RecentEntry {
        RecentEntry {
            seed: Seed::Term {
                profile_id: PROFILES[FALLBACK_PROFILE].id.to_owned(),
                cwd: cwd.to_owned(),
                manual_name: manual_name.map(str::to_owned),
            },
            at: at(100_000 - secs_ago),
        }
    }

    fn files(root: &str, secs_ago: u64) -> RecentEntry {
        RecentEntry {
            seed: Seed::Files {
                root: root.to_owned(),
            },
            at: at(100_000 - secs_ago),
        }
    }

    /// The height the Recent section adds at `scale`: `.menu-sep` with its two
    /// margins, `.menu-label` with its padding, and one row per seed.
    fn recent_block(scale: f32, rows: usize) -> f32 {
        let separator = 2.0 * (SEPARATOR_MARGIN_Y_LOGICAL_PX * scale).round()
            + (SEPARATOR_THICKNESS_LOGICAL_PX * scale).round().max(1.0);
        let heading = ((SECTION_LABEL_PADDING_TOP_LOGICAL_PX
            + SECTION_LABEL_LINE_LOGICAL_PX
            + SECTION_LABEL_PADDING_BOTTOM_LOGICAL_PX)
            * scale)
            .round();
        separator + heading + (ITEM_HEIGHT_LOGICAL_PX * scale).round() * rows as f32
    }

    /// PIN — the menu hangs 4px under the button that opened it, at the button's
    /// own left edge, and it is the mock-up's 180px wide.
    #[test]
    fn the_menu_hangs_under_its_button_at_the_mockup_s_own_width() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let button = anchor(scale);
            let layout = layout(
                button,
                MenuSide::Below,
                (960.0 * scale, 600.0),
                scale,
                NO_RECENT,
            );
            let frame = layout.frame;
            assert_eq!(
                frame[1],
                (button[3] + 4.0 * scale).round(),
                "scale {scale}: the menu sits 4px under the button"
            );
            assert_eq!(
                frame[0],
                button[0].round(),
                "scale {scale}: the menu's left edge is the button's, on a whole pixel"
            );
            assert_eq!(
                (frame[2] - frame[0]).round(),
                (180.0 * scale).round(),
                "scale {scale}: `.profile-menu` is 180px wide"
            );
            assert_eq!(layout.items.len(), PROFILES.len());
        }
    }

    /// PIN — beside the rail's button, not under it, and the bug that asked.
    ///
    /// A real window in rail mode opened the picker adrift in the middle of the
    /// terminal, because the anchor was still read out of the *horizontal*
    /// strip's geometry — a pure function of a width and a trailer list, which
    /// goes on answering with a box in the title bar long after the tabs have
    /// moved down the side.
    ///
    /// With the rail's own box, "under and left" is still wrong: that is the
    /// rail's own column, 46px of it while parked. Beside, then, and level with
    /// the button's top — and, because Q181 collapses the `˅` while the rail is
    /// parked so the `+` is the anchor there instead, the placement is written
    /// so those two boxes give the same answer. They share a right edge and a
    /// top by construction, so the menu does not jump when the panel slides open
    /// and the chevron comes back.
    #[test]
    fn beside_the_rail_the_menu_clears_its_button_rather_than_hanging_down_it() {
        let scale = 1.0;
        // A 220px rail's `+` row, and the `˅` that stands at its right end:
        // `new_tab` is 173 wide, a 2px gap, then a 28px chevron (Q181).
        let plus = [8.0_f32, 400.0, 181.0, 430.0];
        let chevron = [183.0_f32, 400.0, 211.0, 430.0];

        let open = layout(chevron, MenuSide::Beside, (1400.0, 900.0), scale, NO_RECENT);
        assert_eq!(
            open.frame[0],
            (chevron[2] + 4.0 * scale).round(),
            "the menu stands clear of the chevron's right edge, not under it"
        );
        assert_eq!(
            open.frame[1], chevron[1],
            "and level with its top rather than below its bottom"
        );

        let parked = layout(plus, MenuSide::Beside, (1400.0, 900.0), scale, NO_RECENT);
        assert_eq!(
            parked.frame[1], open.frame[1],
            "a parked rail anchors on the `+` instead, and the two share a top, \
             so the menu does not jump as the panel opens"
        );

        // The `Below` placement is still the strip's own, and still different.
        let strip = layout(chevron, MenuSide::Below, (1400.0, 900.0), scale, NO_RECENT);
        assert_eq!(strip.frame[0], chevron[0].round());
        assert_eq!(strip.frame[1], (chevron[3] + 4.0 * scale).round());
    }

    /// PIN — a menu beside a button near the window's foot is pushed back up
    /// rather than hanging out of it. The `Below` placement never needed this —
    /// it only ever hangs off the title bar — which is why the clamp arrived
    /// with the rail.
    #[test]
    fn a_menu_beside_a_low_button_stays_inside_the_window() {
        let scale = 1.0;
        let surface = (1400.0_f32, 500.0_f32);
        let low = layout(
            [8.0, 470.0, 211.0, 500.0],
            MenuSide::Beside,
            surface,
            scale,
            NO_RECENT,
        );
        assert!(
            low.frame[3] <= surface.1 - 8.0 + 0.001,
            "the menu ran past the window's foot: {:?}",
            low.frame
        );
        assert!(low.frame[1] >= 0.0, "{:?}", low.frame);
    }

    /// PIN — the mock-up's own four, in the mock-up's own order, each with a
    /// stable slug, its own mark and a real program to start.
    ///
    /// The order is load-bearing rather than tidy: `state.defaultProfile` is an
    /// *index* into this list (mock-up 3217), and until P3 makes it a setting,
    /// [`FALLBACK_PROFILE`] is a constant index too. Reordering this array
    /// silently re-points it.
    ///
    /// This test replaces one that asserted `PROFILES.len() == 1` and was right
    /// to: a list of four rows that all started PowerShell would have been three
    /// rows that cannot do what they say. What makes four honest now is the rest
    /// of this file — a program per profile, and a greyed row where the machine
    /// has none.
    #[test]
    fn the_picker_offers_exactly_the_profiles_this_build_has() {
        assert_eq!(PROFILES.len(), 4);
        let listed: Vec<_> = PROFILES.iter().map(|profile| profile.id).collect();
        assert_eq!(listed, ["pwsh", "wsl", "gitbash", "cmd"]);
        assert_eq!(PROFILES[FALLBACK_PROFILE].title, "PowerShell");

        // Four profiles, four marks: the icon column carries information only
        // while no two rows carry the same thing.
        for (index, left) in PROFILES.iter().enumerate() {
            for right in &PROFILES[index + 1..] {
                assert_ne!(
                    left.mark, right.mark,
                    "{} and {} would be one row twice — a profile's icon is its mark",
                    left.id, right.id
                );
            }
        }

        // And four ids, because an id is what a seed is keyed on: two profiles
        // sharing one would be two tabs that cannot be told apart on disk.
        let ids: std::collections::HashSet<_> = listed.iter().collect();
        assert_eq!(ids.len(), PROFILES.len());
        for profile in PROFILES {
            assert_eq!(
                index_of_id(profile.id),
                PROFILES.iter().position(|p| p.id == profile.id).unwrap(),
                "{} must resolve to its own row",
                profile.id
            );
        }
    }

    /// PIN — **the arguments are the profile's, not the spawn path's.**
    ///
    /// `-NoLogo` used to be welded into `PtyCommand::interactive_shell` as "the
    /// one argument", which was true only while every shell this terminal could
    /// start was a PowerShell. It is a PowerShell flag: `cmd.exe` reads it as
    /// the name of a batch file to run and `bash` as a file to open, so passing
    /// it to the other three would produce three shells that start wrong — the
    /// exact failure that cannot be seen from a screenshot of the menu.
    #[test]
    fn only_the_powershell_profile_asks_for_nologo() {
        for profile in PROFILES {
            let has_nologo = profile.args.contains(&"-NoLogo");
            assert_eq!(
                has_nologo,
                profile.id == "pwsh",
                "{} must {} pass -NoLogo",
                profile.id,
                if profile.id == "pwsh" { "" } else { "not" }
            );
        }
        assert_eq!(PROFILES[index_of_id("cmd")].args, &[] as &[&str]);
        assert_eq!(
            PROFILES[index_of_id("gitbash")].args,
            &["--login", "-i"],
            "without --login this is a bash that cannot find git"
        );
    }

    /// PIN — the fallback profile is the one profile that cannot be unavailable.
    ///
    /// Everything else leans on it: `index_of_id` falls back to it for an id this
    /// build does not have, `default_profile` falls back to it for a *setting*
    /// this machine cannot honour, `create_leaf_session` falls back to it when a
    /// profile's own program will not start, and the picker's `default` hint is
    /// drawn on the assumption that no row is ever both the default and greyed. A
    /// fallback that could be missing would turn each of those into a window with
    /// no shell in it.
    #[test]
    fn the_fallback_profile_can_always_be_started() {
        assert_eq!(
            PROFILES[FALLBACK_PROFILE].program,
            ProgramSource::DefaultShell,
            "the fallback profile resolves through the OS's own PowerShell chain"
        );
        // Even on a machine with nothing else on it.
        assert!(bare().is_available(FALLBACK_PROFILE));
        assert!(equipped().is_available(FALLBACK_PROFILE));
    }

    /// PIN — `default` is a caption on the *chosen* row, not on the first one.
    ///
    /// Red gate: the hint used to be `index == DEFAULT_PROFILE`, a constant, so
    /// every reading of the menu said PowerShell however the setting was set. The
    /// second half — exactly one row wears it — is what keeps a fix that ORs a
    /// new condition onto the old one from passing.
    #[test]
    fn the_default_caption_follows_the_setting_and_lands_on_exactly_one_row() {
        let scale = 1.0;
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            NO_RECENT,
        );
        for (chosen, profile) in PROFILES.iter().enumerate() {
            let layers = build(
                &layout,
                &equipped(),
                chosen,
                None,
                NO_RECENT,
                now(),
                &mut fake_measure,
            );
            let captioned: Vec<usize> = layout
                .items
                .iter()
                .enumerate()
                .filter(|(_, row)| {
                    layers.iter().flat_map(|layer| &layer.labels).any(|label| {
                        label.text == HINT_TEXT
                            && label.rect[1] == row[1]
                            && label.rect[3] == row[3]
                    })
                })
                .map(|(index, _)| index)
                .collect();
            assert_eq!(
                captioned,
                vec![chosen],
                "the `default` hint belongs to {} and to nothing else",
                profile.id
            );
        }
    }

    /// PIN — a row's annotation takes its width out of the row before the name
    /// gets any, so the two are never drawn on top of each other.
    ///
    /// **Found on the real machine** (2026-08-11), the frame after "Command
    /// Prompt" became the default: the menu is 180px, `Command Prompt` alone very
    /// nearly fills it, and `default` was right-aligned into a span the name's
    /// own box also ran to the end of — so the window printed `Command Promptdefault`,
    /// two labels in one place. It was latent before this ticket and reachable
    /// only through `not installed` on a long-named profile; making the default
    /// configurable put it on the common path.
    ///
    /// The mock-up never has it because `.default-hint { margin-left: auto }` is a
    /// flex item and the *name* is the one carrying `text-overflow: ellipsis`
    /// (line 1031). Reserving the measured hint is that layout; the name then
    /// clips, which is what `ChromeLabel` already does per glyph.
    #[test]
    fn a_rows_annotation_reserves_its_own_width_and_the_name_stops_short_of_it() {
        let scale = 1.0;
        let vault = [term(r"C:\some\very\long\path\indeed", None, 3_600)];
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            &vault,
        );
        // Every profile as the default in turn, so the longest name carrying the
        // hint is covered rather than only the first row's short one.
        for chosen in 0..PROFILES.len() {
            for programs in [equipped(), bare()] {
                let layer = one_layer(build(
                    &layout,
                    &programs,
                    chosen,
                    None,
                    &vault,
                    now(),
                    &mut fake_measure,
                ));
                // Rows are one line box tall, so two labels sharing a row are the
                // two whose vertical spans agree.
                for name in &layer.labels {
                    if name.align_right {
                        continue;
                    }
                    for annotation in layer.labels.iter().filter(|other| {
                        other.align_right
                            && other.rect[1] == name.rect[1]
                            && other.rect[3] == name.rect[3]
                    }) {
                        // Where the hint's glyphs actually start: it is right
                        // aligned, so its ink begins one measured width back from
                        // the right edge of its box.
                        let ink_left = annotation.rect[2]
                            - fake_measure(&annotation.text, annotation.font_size_px);
                        assert!(
                            name.rect[2] <= ink_left,
                            "default={chosen}: {:?} runs to {} and {:?} starts at {ink_left}",
                            name.text,
                            name.rect[2],
                            annotation.text,
                        );
                    }
                }
            }
        }
    }

    /// PIN — a row says what its caption could not fit, and only then.
    ///
    /// Two rules in one list because they are one rule: the mock-up hangs a
    /// `title` on a menu row exactly when the row is showing less than it knows
    /// (7426/7430 caption a Recent row with a path's last segment and tip the
    /// whole path). A greyed profile row is the case the mock-up never had — it
    /// is captioned `not installed`, which is a state without its reason — and an
    /// *available* profile row is showing everything it knows, so it says nothing
    /// rather than restating the label under the pointer.
    #[test]
    fn a_row_is_tipped_with_what_its_caption_left_out_and_nothing_else() {
        let scale = 1.0;
        let vault = [term(r"D:\Developer\BetterTerminal\crates", None, 30)];
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            &vault,
        );

        let bare_tips: Vec<_> = layout.tips(&bare(), &vault).collect();
        let profiles_tipped: Vec<_> = bare_tips
            .iter()
            .filter_map(|(row, _, text)| match row {
                MenuRow::Profile(index) => Some((*index, text.clone())),
                MenuRow::Recent(_) => None,
            })
            .collect();
        assert_eq!(
            profiles_tipped,
            vec![
                (
                    index_of_id("wsl"),
                    "WSL — not found on this machine".to_owned()
                ),
                (
                    index_of_id("gitbash"),
                    "Git Bash — not found on this machine".to_owned()
                ),
                (
                    index_of_id("cmd"),
                    "Command Prompt — not found on this machine".to_owned()
                ),
            ],
            "every greyed row says why, in its own name, and the startable one \
             says nothing"
        );

        // The rectangles are the laid-out rows themselves — a tip registered
        // against a box computed a second way is a tip that appears where the
        // row is not.
        for (row, rect, _) in &bare_tips {
            let expected = match row {
                MenuRow::Profile(index) => layout.items[*index],
                MenuRow::Recent(index) => layout.recent[*index],
            };
            assert_eq!(*rect, expected);
        }

        // And on a machine with all four, no profile row has anything to add —
        // but the Recent row still carries the path its caption cropped.
        let equipped_tips: Vec<_> = layout.tips(&equipped(), &vault).collect();
        assert_eq!(
            equipped_tips,
            vec![(
                MenuRow::Recent(0),
                layout.recent[0],
                r"D:\Developer\BetterTerminal\crates".to_owned()
            )],
        );
        assert_eq!(
            recent_label(&vault[0].seed),
            "crates",
            "the caption really is only the leaf, which is what the tip is for"
        );
    }

    /// PIN — the setting decides, the machine vetoes, and neither is the other.
    ///
    /// Red gate for the whole of P3's data half. Four inputs and four different
    /// answers, and the two failure modes this exists to stop are opposites: a
    /// resolver that trusted the file hands a window a shell that is not
    /// installed, and one that only ever answered `FALLBACK_PROFILE` makes the
    /// setting a control that does nothing.
    #[test]
    fn the_default_profile_is_the_stored_choice_unless_this_machine_cannot_honour_it() {
        let all = equipped();

        assert_eq!(
            default_profile("cmd", &all),
            index_of_id("cmd"),
            "a stored id this machine can start is the answer, whatever index it is"
        );
        assert_eq!(
            default_profile(bt_persist::DEFAULT_PROFILE_UNSET, &all),
            FALLBACK_PROFILE,
            "nobody has ever opened the setting: the floor, not an error"
        );
        assert_eq!(
            default_profile("a-profile-from-a-newer-build", &all),
            FALLBACK_PROFILE,
            "an id this build does not have degrades exactly as a leaf's does"
        );
        assert_eq!(
            default_profile("gitbash", &bare()),
            FALLBACK_PROFILE,
            "chosen, installed once, uninstalled since — the window still opens"
        );
        // And the resolved answer is always startable, which is the property
        // `create_leaf_session`'s `expect` is standing on.
        for stored in ["cmd", "gitbash", "wsl", "pwsh", "", "nonsense"] {
            for machine in [&all, &bare()] {
                assert!(
                    machine.is_available(default_profile(stored, machine)),
                    "the default resolved for {stored:?} must be startable"
                );
            }
        }
    }

    /// PIN — every profile can say where it starts, and WSL says it differently.
    ///
    /// The trap is a `starting_dir` that resolves to a Windows path for all four:
    /// it would compile, it would look right in the table, and a WSL tab would
    /// open in `/mnt/c/Users/…` — a real directory, silently not the one the same
    /// shell opens in when started any other way.
    #[test]
    fn a_profile_states_its_starting_place_in_the_form_its_launcher_can_take() {
        let machine = FakeMachine::default().with_var("USERPROFILE", r"C:\Users\dev");
        for profile in ["pwsh", "gitbash", "cmd"] {
            assert_eq!(
                starting_place(index_of_id(profile), &machine),
                StartingPlace::Directory(PathBuf::from(r"C:\Users\dev")),
                "{profile} is a Windows process and takes a working directory"
            );
        }
        assert_eq!(
            starting_place(index_of_id("wsl"), &machine),
            StartingPlace::Arguments(&["--cd", "~"]),
            "WSL's home has no Windows spelling, so it is asked for rather than handed over"
        );
        // The home is read from the variable and never composed, so a redirected
        // profile is followed rather than guessed at.
        assert_eq!(
            starting_place(
                FALLBACK_PROFILE,
                &FakeMachine::default().with_var("USERPROFILE", r"\\server\redirected\dev")
            ),
            StartingPlace::Directory(PathBuf::from(r"\\server\redirected\dev")),
        );
        assert_eq!(
            starting_place(FALLBACK_PROFILE, &FakeMachine::default()),
            StartingPlace::Unstated,
            "a machine that cannot name its own home is told nothing, not a guess"
        );
    }

    /// PIN — a profile is available exactly when its program is on the machine,
    /// and a machine that has none of them still has PowerShell.
    ///
    /// Red gate: probing only `%ProgramFiles%` for Git — which is where it lands
    /// from the ordinary installer and nowhere else. A per-user install
    /// (`%LocalAppData%\Programs\Git`) is the default for anybody without
    /// administrator rights, so "Git Bash is greyed on a machine that has Git
    /// Bash" is not a corner case, it is a whole class of user.
    #[test]
    fn a_profile_is_offered_when_this_machine_has_its_program_and_greyed_when_it_does_not() {
        let none = bare();
        assert_eq!(
            (0..PROFILES.len())
                .filter(|index| none.is_available(*index))
                .collect::<Vec<_>>(),
            vec![FALLBACK_PROFILE],
            "a bare Windows box offers PowerShell and says the truth about the rest"
        );

        let all = equipped();
        for (index, profile) in PROFILES.iter().enumerate() {
            assert!(
                all.is_available(index),
                "{} is installed here and must be offered",
                profile.id
            );
        }
        assert_eq!(
            all.program(index_of_id("cmd")),
            Some(OsStr::new(r"C:\WINDOWS\System32\cmd.exe")),
            "the resolved program is the probed path, not the profile's id"
        );

        // Git through the per-user installer, which is not under %ProgramFiles%.
        let per_user = ProfilePrograms::probe(
            &FakeMachine::default()
                .with_var("LocalAppData", r"C:\Users\dev\AppData\Local")
                .with_file(r"C:\Users\dev\AppData\Local\Programs\Git\bin\bash.exe"),
        );
        assert_eq!(
            per_user.program(index_of_id("gitbash")),
            Some(OsStr::new(
                r"C:\Users\dev\AppData\Local\Programs\Git\bin\bash.exe"
            ))
        );
        assert!(!per_user.is_available(index_of_id("wsl")));

        // The candidate list is an *order*: the first well-known path that
        // exists wins, so a machine carrying both installs starts the
        // system-wide one.
        let both = ProfilePrograms::probe(
            &FakeMachine::default()
                .with_var("ProgramFiles", r"C:\Program Files")
                .with_var("LocalAppData", r"C:\Users\dev\AppData\Local")
                .with_file(r"C:\Program Files\Git\bin\bash.exe")
                .with_file(r"C:\Users\dev\AppData\Local\Programs\Git\bin\bash.exe"),
        );
        assert_eq!(
            both.program(index_of_id("gitbash")),
            Some(OsStr::new(r"C:\Program Files\Git\bin\bash.exe"))
        );
    }

    /// PIN — **Git installed anywhere at all is found, through `git.exe` on
    /// `PATH`.**
    ///
    /// Red gate, and it is not hypothetical: this ticket's own first real-machine
    /// check ran on a box with Git at `D:\App\Tool\Git`, which is under none of
    /// the three installers' default roots. A well-known-paths list can only
    /// enumerate defaults, so on that machine — and on everyone else's who chose
    /// their own directory — the row would have been greyed `not installed` over
    /// a working Git Bash. That is not honest degradation, it is a wrong answer
    /// delivered in the tone of an honest one, which is worse than no row at all.
    ///
    /// The anchor is climbed two levels because Git for Windows puts the tool at
    /// `<root>\cmd\git.exe` and the shell at `<root>\bin\bash.exe` — siblings
    /// under one root, not nested — so joining onto `git.exe`'s own directory
    /// would look in `<root>\cmd\bin` and find nothing.
    #[test]
    fn git_installed_outside_the_well_known_roots_is_found_through_the_tool_on_path() {
        let custom = ProfilePrograms::probe(
            &FakeMachine::default()
                .with_var(
                    "PATH",
                    std::env::join_paths([r"C:\Other", r"D:\App\Tool\Git\cmd"])
                        .expect("test PATH joins cleanly")
                        .to_str()
                        .expect("ASCII test paths"),
                )
                .with_file(r"D:\App\Tool\Git\cmd\git.exe")
                .with_file(r"D:\App\Tool\Git\bin\bash.exe"),
        );
        assert_eq!(
            custom.program(index_of_id("gitbash")),
            Some(OsStr::new(r"D:\App\Tool\Git\bin\bash.exe")),
            "a Git that is on the path is a Git we can find, wherever it was put"
        );

        // The anchor is a landmark, not a promise: `git.exe` on the path with no
        // `bash.exe` beside it is still an unavailable profile, because what the
        // row offers is bash and bash is what has to exist.
        let tool_only = ProfilePrograms::probe(
            &FakeMachine::default()
                .with_var(
                    "PATH",
                    std::env::join_paths([r"D:\App\Tool\Git\cmd"])
                        .expect("test PATH joins cleanly")
                        .to_str()
                        .expect("ASCII test paths"),
                )
                .with_file(r"D:\App\Tool\Git\cmd\git.exe"),
        );
        assert!(!tool_only.is_available(index_of_id("gitbash")));

        // And the anchor is tried first, so the install the user actually works
        // with wins over a stale copy in `%ProgramFiles%`.
        let both = ProfilePrograms::probe(
            &FakeMachine::default()
                .with_var(
                    "PATH",
                    std::env::join_paths([r"D:\App\Tool\Git\cmd"])
                        .expect("test PATH joins cleanly")
                        .to_str()
                        .expect("ASCII test paths"),
                )
                .with_var("ProgramFiles", r"C:\Program Files")
                .with_file(r"D:\App\Tool\Git\cmd\git.exe")
                .with_file(r"D:\App\Tool\Git\bin\bash.exe")
                .with_file(r"C:\Program Files\Git\bin\bash.exe"),
        );
        assert_eq!(
            both.program(index_of_id("gitbash")),
            Some(OsStr::new(r"D:\App\Tool\Git\bin\bash.exe"))
        );
    }

    /// PIN — **a greyed row is drawn and is not offered**, and both halves are
    /// answered by [`hit`] so they cannot disagree.
    ///
    /// Red gate, and it is the difference between this and hiding the row. A
    /// missing row and a greyed row look nothing alike to a user — one says "you
    /// do not have this", the other says "we never thought of this" — but they
    /// look identical to a test that only counts what the menu can launch. So
    /// what is asserted here is that the row is *still on screen*, still named,
    /// still carrying its own artwork, and still costs the same 29.5px of menu:
    /// the layout does not move when a machine lacks a shell.
    #[test]
    fn a_profile_this_machine_cannot_start_is_drawn_greyed_and_refuses_the_press() {
        let scale = 1.0;
        let programs = bare();
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            NO_RECENT,
        );
        assert_eq!(
            layout.items.len(),
            PROFILES.len(),
            "greying is not hiding: every profile still has a row"
        );

        let git = index_of_id("gitbash");
        let row = layout.items[git];
        let (x, y) = (
            f64::from((row[0] + row[2]) / 2.0),
            f64::from((row[1] + row[3]) / 2.0),
        );
        assert_eq!(
            hit(&layout, &programs, NO_RECENT, x, y),
            Some(None),
            "a press on a row this machine cannot start is the menu's own body: \
             it neither opens a tab nor escapes to whatever is under the menu"
        );
        assert_eq!(
            hit(&layout, &equipped(), NO_RECENT, x, y),
            Some(Some(MenuRow::Profile(git))),
            "and the very same pixel does open it where Git is installed"
        );

        let palette = chrome_palette();
        let layer = one_layer(build(
            &layout,
            &programs,
            FALLBACK_PROFILE,
            None,
            NO_RECENT,
            now(),
            &mut fake_measure,
        ));
        let name = layer
            .labels
            .iter()
            .find(|label| label.text == "Git Bash")
            .expect("the row is still named — a hidden row would say nothing at all");
        assert_eq!(
            name.color, palette.menu_item_hint_text,
            "and named in the menu's quietest ink"
        );
        assert!(
            layer
                .labels
                .iter()
                .any(|label| label.text == UNAVAILABLE_HINT_TEXT),
            "with the reason in the hint slot, so the grey does not have to be guessed at"
        );

        let mark = layer
            .sprites
            .iter()
            .find(|sprite| sprite.mark == ChromeMark::ProfileGit)
            .expect("the row still wears its own artwork");
        assert_eq!(mark.opacity, UNAVAILABLE_MARK_OPACITY);
        assert!(
            mark.grayscale,
            "a profile mark carries its own colours, so only desaturation can quiet it"
        );

        // The default profile is on the same machine and is untouched: greying
        // is per row, and the row that always works still looks like it works.
        let pwsh = layer
            .sprites
            .iter()
            .find(|sprite| sprite.mark == ChromeMark::ProfilePowerShell)
            .expect("PowerShell is always startable");
        assert_eq!(pwsh.opacity, 1.0);
        assert!(!pwsh.grayscale);
        assert!(
            layer.labels.iter().any(|label| label.text == HINT_TEXT),
            "and still says it is the default"
        );
    }

    /// PIN — a Recent row is greyed by the same rule its profile row is.
    ///
    /// Red gate: greying the profile list and not the Recent list would put both
    /// answers to one question in a single menu — `Git Bash` greyed at the top,
    /// and three rows below it a live `~/repo · 3m ago` under the same Git mark,
    /// offering to start the shell the row above just said you do not have.
    #[test]
    fn a_recent_row_whose_shell_is_missing_is_greyed_with_its_profile() {
        let scale = 1.0;
        let vault = [
            RecentEntry {
                seed: Seed::Term {
                    profile_id: "gitbash".to_owned(),
                    cwd: "C:\\repo".to_owned(),
                    manual_name: None,
                },
                at: at(100_000),
            },
            term("C:\\work", None, 60),
            files("C:\\notes", 120),
        ];
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            &vault,
        );
        let programs = bare();
        let centre = |rect: [f32; 4]| {
            (
                f64::from((rect[0] + rect[2]) / 2.0),
                f64::from((rect[1] + rect[3]) / 2.0),
            )
        };

        let (x, y) = centre(layout.recent[0]);
        assert_eq!(
            hit(&layout, &programs, &vault, x, y),
            Some(None),
            "the seed names a shell this machine has not got"
        );
        assert_eq!(
            hit(&layout, &equipped(), &vault, x, y),
            Some(Some(MenuRow::Recent(0))),
            "and opens where it has"
        );

        // Its neighbours are untouched: the PowerShell seed still opens, and so
        // does a files locus, which has no shell to be missing in the first
        // place.
        for index in [1, 2] {
            let (x, y) = centre(layout.recent[index]);
            assert_eq!(
                hit(&layout, &programs, &vault, x, y),
                Some(Some(MenuRow::Recent(index))),
                "recent row {index} is startable on any machine"
            );
        }

        let layer = one_layer(build(
            &layout,
            &programs,
            FALLBACK_PROFILE,
            None,
            &vault,
            now(),
            &mut fake_measure,
        ));
        let git = layer
            .sprites
            .iter()
            .find(|sprite| sprite.mark == ChromeMark::ProfileGit)
            .expect("the recent row wears its own profile's mark");
        assert!(git.grayscale && git.opacity == UNAVAILABLE_MARK_OPACITY);
        assert!(
            layer
                .sprites
                .iter()
                .any(|sprite| sprite.mark == ChromeMark::Folder && !sprite.grayscale),
            "a files locus is never greyed: it has no shell that could be missing"
        );
    }

    /// PIN — a popup is not a modal: a point off the menu belongs to nobody
    /// here, and a point on the menu's own body is not a row.
    ///
    /// Red gate: returning `Some(None)` for everything (the modal shape) would
    /// swallow every press in the window while the picker is up.
    #[test]
    fn the_menu_claims_its_own_box_and_nothing_else() {
        let scale = 1.0;
        let button = anchor(scale);
        let layout = layout(button, MenuSide::Below, (960.0, 600.0), scale, NO_RECENT);
        let frame = layout.frame;
        let item = layout.items[0];
        assert_eq!(
            hit(
                &layout,
                &equipped(),
                NO_RECENT,
                f64::from((item[0] + item[2]) / 2.0),
                f64::from((item[1] + item[3]) / 2.0)
            ),
            Some(Some(MenuRow::Profile(0)))
        );
        assert_eq!(
            hit(
                &layout,
                &equipped(),
                NO_RECENT,
                f64::from(frame[0] + 1.0),
                f64::from(frame[3] - 1.0)
            ),
            Some(None),
            "the menu's own padding is the menu's, not a row's"
        );
        assert_eq!(
            hit(
                &layout,
                &equipped(),
                NO_RECENT,
                f64::from(frame[0] - 4.0),
                f64::from(frame[1] + 4.0)
            ),
            None,
            "beside the menu belongs to whatever is there"
        );
        assert_eq!(hit(&layout, &equipped(), NO_RECENT, 400.0, 500.0), None);
    }

    /// PIN — the menu is pushed off the window's right edge by no more than the
    /// mock-up's own 8px margin, however near that edge the button sits.
    #[test]
    fn a_menu_opened_near_the_right_edge_stays_inside_the_window() {
        let scale = 1.0;
        let surface = 300.0;
        let layout = layout(
            [260.0, 9.0, 288.0, 37.0],
            MenuSide::Below,
            (surface, 600.0),
            scale,
            NO_RECENT,
        );
        let frame = layout.frame;
        assert!(
            frame[2] <= surface - 8.0,
            "the menu ran past the window edge: {frame:?}"
        );
        assert!(frame[0] >= 0.0);
    }

    /// PIN — hover is a fact about an open menu. A stale row cannot stay lit
    /// under a menu that is no longer there.
    #[test]
    fn hover_belongs_to_an_open_menu_only() {
        let mut menu = ProfileMenu::default();
        assert!(
            !menu.set_hover(Some(MenuRow::Profile(0))),
            "a shut menu has no hovered row"
        );
        assert_eq!(menu.hover(), None);
        menu.toggle();
        assert!(menu.is_open());
        assert!(menu.set_hover(Some(MenuRow::Profile(0))));
        assert_eq!(menu.hover(), Some(MenuRow::Profile(0)));
        assert!(
            menu.set_hover(Some(MenuRow::Recent(0))),
            "row 0 of the other list is a different row"
        );
        assert_eq!(menu.hover(), Some(MenuRow::Recent(0)));
        assert!(menu.close());
        assert_eq!(menu.hover(), None);
        assert!(!menu.close(), "closing a shut menu consumes nothing");
    }

    /// PIN — the hovered row wears `--ink` on `--hover`, and the resting one
    /// `--ink2` on nothing; the row also carries its profile's own mark.
    #[test]
    fn a_hovered_row_lights_up_and_every_row_wears_its_profile_s_mark() {
        let scale = 1.0;
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            NO_RECENT,
        );
        let palette = chrome_palette();
        let rest = one_layer(build(
            &layout,
            &equipped(),
            FALLBACK_PROFILE,
            None,
            NO_RECENT,
            now(),
            &mut fake_measure,
        ));
        let hover = one_layer(build(
            &layout,
            &equipped(),
            FALLBACK_PROFILE,
            Some(MenuRow::Profile(0)),
            NO_RECENT,
            now(),
            &mut fake_measure,
        ));
        let (rest_quads, rest_labels, sprites) = (rest.quads, rest.labels, rest.sprites);
        let (hover_quads, hover_labels) = (hover.quads, hover.labels);
        assert!(
            sprites
                .iter()
                .any(|sprite| sprite.mark == ChromeMark::ProfilePowerShell)
        );
        assert!(
            rest_labels
                .iter()
                .any(|label| label.text == "PowerShell" && label.color == palette.menu_item_text)
        );
        assert!(
            hover_labels.iter().any(|label| label.text == "PowerShell"
                && label.color == palette.menu_item_text_selected)
        );
        assert!(
            hover_quads.len() > rest_quads.len(),
            "the hovered row must add a fill"
        );
        assert!(
            hover_quads
                .iter()
                .any(|quad| quad.color == palette.menu_item_hover),
            "and that fill is `--hover` over `--menu`"
        );
        assert!(
            rest_labels.iter().any(|label| label.text == HINT_TEXT),
            "the default profile says so"
        );
    }

    /// PIN — I89/I90/I93/I95: every measured value of `.profile-menu` and
    /// `.profile-item` (mock-up lines 1006-1031), nailed to the stylesheet.
    ///
    /// The surface, its rows and its ink are checked elsewhere in this module;
    /// what this pins is the ruler — the numbers a redesign would have to change
    /// deliberately rather than drift past.
    #[test]
    fn the_menu_measures_what_the_stylesheet_says_it_measures() {
        assert_eq!(MENU_MIN_WIDTH_LOGICAL_PX, 180.0, "min-width: 180px");
        assert_eq!(MENU_RADIUS_LOGICAL_PX, 8.0, "border-radius: 8px");
        assert_eq!(MENU_PADDING_LOGICAL_PX, 4.0, "padding: 4px");
        assert_eq!(MENU_OFFSET_LOGICAL_PX, 4.0, "top = anchor.bottom + 4");
        assert_eq!(MENU_EDGE_MARGIN_LOGICAL_PX, 8.0, "win.width - mw - 8");
        assert_eq!(ITEM_RADIUS_LOGICAL_PX, 5.0, ".profile-item radius 5px");
        assert_eq!(ITEM_PADDING_X_LOGICAL_PX, 10.0, "padding: 7px 10px");
        assert_eq!(ITEM_GAP_LOGICAL_PX, 10.0, "gap: 10px");
        assert_eq!(ITEM_FONT_LOGICAL_PX, 13.0, "font-size: 13px");
        assert_eq!(
            ITEM_ICON_COLUMN_LOGICAL_PX, 14.0,
            ".ticon {{ width: 14px }}"
        );
        assert_eq!(HINT_FONT_LOGICAL_PX, 11.0, ".default-hint font-size 11px");
        // 7 + 15.5 + 7: the 13px line box the mock-up's own renderer produces,
        // inside the row's vertical padding.
        assert_eq!(ITEM_HEIGHT_LOGICAL_PX, 29.5);

        // I92, the Recent section (mock-up lines 996-1002).
        assert_eq!(SEPARATOR_THICKNESS_LOGICAL_PX, 1.0, ".menu-sep height 1px");
        assert_eq!(SEPARATOR_MARGIN_Y_LOGICAL_PX, 5.0, ".menu-sep margin 5px 0");
        assert_eq!(
            SEPARATOR_ALPHA_ON_DARK, 0.06,
            "--border-soft rgba(255,255,255,.06)"
        );
        assert_eq!(
            SEPARATOR_ALPHA_ON_LIGHT, 0.055,
            "--border-soft rgba(0,0,0,.055)"
        );
        assert_eq!(
            SECTION_LABEL_FONT_LOGICAL_PX, 10.5,
            ".menu-label font-size 10.5px"
        );
        assert_eq!(
            SECTION_LABEL_TRACKING_EM, 0.05,
            ".menu-label letter-spacing .05em"
        );
        // 3 + 12.5 + 5: the 10.5px line box the mock-up's own renderer produces,
        // inside `padding: 3px 10px 5px`.
        assert_eq!(SECTION_LABEL_PADDING_TOP_LOGICAL_PX, 3.0);
        assert_eq!(SECTION_LABEL_PADDING_X_LOGICAL_PX, 10.0);
        assert_eq!(SECTION_LABEL_PADDING_BOTTOM_LOGICAL_PX, 5.0);
        assert_eq!(SECTION_LABEL_LINE_LOGICAL_PX, 12.5);
        assert_eq!(
            RECENT_ITEM_MAX_WIDTH_LOGICAL_PX, 260.0,
            ".recent-item max-width 260px"
        );
        assert_eq!(
            RECENT_SECTION_LABEL, "RECENTLY OPENED",
            "`Recently opened` under `text-transform: uppercase`"
        );

        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let layout = layout(
                anchor(scale),
                MenuSide::Below,
                (960.0 * scale, 600.0),
                scale,
                NO_RECENT,
            );
            let item = layout.items[0];
            assert_eq!(
                (item[3] - item[1]).round(),
                (ITEM_HEIGHT_LOGICAL_PX * scale).round(),
                "scale {scale}: a row is its own height"
            );
            // `padding: 4px` inside a 1px border: the row is inset from the
            // menu's edge by both.
            let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
            assert_eq!(
                item[0] - layout.frame[0],
                border + MENU_PADDING_LOGICAL_PX * scale,
                "scale {scale}: the menu's own padding sits outside its rows"
            );
            assert_eq!(layout.frame[2] - item[2], item[0] - layout.frame[0]);
        }
    }

    /// PIN — I93: the `default` hint is `--ink3` over `--menu`, and the mark
    /// column is the mock-up's 14px with its 15px mark centred on it.
    ///
    /// Red gate: the hint used to wear `dialog_muted_text` — the same ink
    /// composited over `--win`, the settings dialog's surface. The two agree in
    /// the light theme and part by six levels in the dark, which is exactly the
    /// kind of error that survives a light-theme review.
    #[test]
    fn the_default_hint_is_inked_for_a_menu_and_not_for_a_dialog() {
        let scale = 1.0;
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            NO_RECENT,
        );
        let palette = chrome_palette();
        let layers = build(
            &layout,
            &equipped(),
            FALLBACK_PROFILE,
            None,
            NO_RECENT,
            now(),
            &mut fake_measure,
        );
        let labels: Vec<_> = layers.iter().flat_map(|layer| &layer.labels).collect();
        let sprites: Vec<_> = layers.iter().flat_map(|layer| &layer.sprites).collect();
        let hint = labels
            .iter()
            .find(|label| label.text == HINT_TEXT)
            .expect("the default profile says so");
        assert_eq!(hint.color, palette.menu_item_hint_text);
        assert_eq!(hint.font_size_px, HINT_FONT_LOGICAL_PX * scale);
        assert!(
            hint.align_right,
            "`margin-left: auto` puts it against the row's trailing padding"
        );
        assert_eq!(
            hint.rect[2],
            layout.items[0][2] - ITEM_PADDING_X_LOGICAL_PX * scale,
            "and that padding is the row's own 10px"
        );
        // The 15px mark, centred on its 14px column — what a flex box does with
        // a child one pixel wider than its box.
        let mark = sprites
            .iter()
            .find(|sprite| sprite.mark == ChromeMark::ProfilePowerShell)
            .expect("every row wears its profile's mark");
        assert_eq!(mark.rect[2] - mark.rect[0], ITEM_MARK_LOGICAL_PX * scale);
        let column_left = layout.items[0][0] + ITEM_PADDING_X_LOGICAL_PX * scale;
        let column_mid = column_left + ITEM_ICON_COLUMN_LOGICAL_PX * scale / 2.0;
        assert!(
            ((mark.rect[0] + mark.rect[2]) / 2.0 - column_mid).abs() <= 0.5,
            "the mark is centred on its column, not aligned to it"
        );
        // And the row's own label clears the column plus the row's 10px gap.
        let title = labels
            .iter()
            .find(|label| label.text == "PowerShell")
            .expect("the row is named");
        assert_eq!(
            title.rect[0],
            column_left + ITEM_ICON_COLUMN_LOGICAL_PX * scale + ITEM_GAP_LOGICAL_PX * scale
        );
        assert_eq!(title.font_size_px, ITEM_FONT_LOGICAL_PX * scale);
    }

    /// PIN — I92, mock-up 7424: `state.recent.length ? … : ""`. An empty vault
    /// adds no rule, no heading and no rows, and leaves the menu at exactly the
    /// height it had before Recent existed.
    ///
    /// Red gate: a section that draws itself unconditionally — a hairline and a
    /// heading reading "RECENTLY OPENED" over nothing at all, which is chrome
    /// making a promise the menu cannot keep.
    #[test]
    fn an_empty_vault_adds_no_rule_no_heading_and_no_rows() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let layout = layout(
                anchor(scale),
                MenuSide::Below,
                (960.0 * scale, 600.0),
                scale,
                NO_RECENT,
            );
            let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
            assert_eq!(
                layout.frame[3] - layout.frame[1],
                (2.0 * (border + MENU_PADDING_LOGICAL_PX * scale)
                    + (ITEM_HEIGHT_LOGICAL_PX * scale).round() * PROFILES.len() as f32)
                    .round(),
                "scale {scale}: the profiles and the menu's own padding, and nothing else"
            );
            assert_eq!(layout.separator, None);
            assert_eq!(layout.section_label, None);
            assert!(layout.recent.is_empty());

            let layer = one_layer(build(
                &layout,
                &equipped(),
                FALLBACK_PROFILE,
                None,
                NO_RECENT,
                now(),
                &mut fake_measure,
            ));
            assert!(
                !layer
                    .labels
                    .iter()
                    .any(|label| label.text == RECENT_SECTION_LABEL),
                "scale {scale}: no heading over an empty list"
            );
            assert_eq!(
                layer.sprites.len(),
                PROFILES.len(),
                "scale {scale}: one mark per profile row and no more"
            );
        }
    }

    /// PIN — the Recent section is `.menu-sep` (1px between two 5px margins),
    /// `.menu-label` (3 + the 10.5px line box + 5) and one 29.5px row per seed,
    /// in that order, inside the menu's own padding.
    #[test]
    fn the_recent_section_is_a_rule_a_heading_and_one_row_for_each_seed() {
        let vault = [term("C:\\repo", None, 0), files("D:\\notes", 600)];
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let empty = layout(
                anchor(scale),
                MenuSide::Below,
                (960.0 * scale, 600.0),
                scale,
                NO_RECENT,
            );
            let full = layout(
                anchor(scale),
                MenuSide::Below,
                (960.0 * scale, 600.0),
                scale,
                &vault,
            );
            assert_eq!(
                (full.frame[3] - full.frame[1]) - (empty.frame[3] - empty.frame[1]),
                recent_block(scale, vault.len()),
                "scale {scale}: the section's own three blocks and nothing more"
            );

            let rule = full.separator.expect("a filled vault is separated");
            let band = full.section_label.expect("and titled");
            let last_profile = *full.items.last().expect("the profile list");
            assert_eq!(
                rule[1] - last_profile[3],
                (SEPARATOR_MARGIN_Y_LOGICAL_PX * scale).round(),
                "scale {scale}: `margin: 5px 0` above the rule"
            );
            assert_eq!(
                rule[3] - rule[1],
                (SEPARATOR_THICKNESS_LOGICAL_PX * scale).round().max(1.0),
                "scale {scale}: a rule of whole pixels, never rounded away to nothing"
            );
            assert_eq!(
                band[1] - rule[3],
                (SEPARATOR_MARGIN_Y_LOGICAL_PX * scale).round(),
                "scale {scale}: and 5px below it"
            );
            assert_eq!(
                band[3] - band[1],
                ((SECTION_LABEL_PADDING_TOP_LOGICAL_PX
                    + SECTION_LABEL_LINE_LOGICAL_PX
                    + SECTION_LABEL_PADDING_BOTTOM_LOGICAL_PX)
                    * scale)
                    .round(),
                "scale {scale}: `padding: 3px 10px 5px` around a 12.5px line box"
            );
            assert_eq!(rule[0], last_profile[0], "the rule spans the row's own box");
            assert_eq!(rule[2], last_profile[2]);

            assert_eq!(full.recent.len(), vault.len());
            assert_eq!(
                full.recent[0][1], band[3],
                "the first row follows the heading"
            );
            for row in &full.recent {
                assert_eq!(
                    row[3] - row[1],
                    (ITEM_HEIGHT_LOGICAL_PX * scale).round(),
                    "scale {scale}: a recent row is a `.profile-item`"
                );
                assert_eq!(row[0], last_profile[0]);
                assert!(
                    row[2] - row[0] <= (RECENT_ITEM_MAX_WIDTH_LOGICAL_PX * scale).round(),
                    "scale {scale}: `.recent-item {{ max-width: 260px }}`"
                );
            }
        }
    }

    /// PIN — a press on a recent row is that recent row, by the vault's own
    /// index, and never a profile.
    ///
    /// Red gate: the menu's rows used to be one untagged `usize` indexed
    /// straight into [`PROFILES`]. With a Recent section under them that number
    /// names two different things, and the bug it produces is silent — clicking
    /// the third recent seed launches a bare PowerShell in the wrong folder and
    /// looks, from the outside, exactly like the menu working.
    #[test]
    fn a_press_on_a_recent_row_is_that_seed_and_never_a_profile() {
        let scale = 1.0;
        let vault = [
            term("C:\\a", None, 0),
            term("C:\\b", None, 60),
            files("C:\\c", 120),
        ];
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            &vault,
        );
        let centre = |rect: [f32; 4]| {
            (
                f64::from((rect[0] + rect[2]) / 2.0),
                f64::from((rect[1] + rect[3]) / 2.0),
            )
        };
        for index in 0..vault.len() {
            let (x, y) = centre(layout.recent[index]);
            assert_eq!(
                hit(&layout, &equipped(), &vault, x, y),
                Some(Some(MenuRow::Recent(index))),
                "recent row {index} must answer with its own index in its own list"
            );
        }
        let (x, y) = centre(layout.items[0]);
        assert_eq!(
            hit(&layout, &equipped(), &vault, x, y),
            Some(Some(MenuRow::Profile(0)))
        );

        // The rule and the heading name nothing you can open, so they are the
        // menu's body — a press there does nothing rather than something.
        let rule = layout.separator.expect("separated");
        let band = layout.section_label.expect("titled");
        for rect in [rule, band] {
            let (x, y) = centre(rect);
            assert_eq!(hit(&layout, &equipped(), &vault, x, y), Some(None));
        }
    }

    /// PIN — the menu shows at most the eight seeds the vault itself keeps
    /// (`docs/DESIGN.md` §7.1.4, mock-up 4106), whatever it is handed.
    ///
    /// Red gate: a menu whose height is "however many the caller passed" is a
    /// popup that grows off the bottom of the window, and every row past the
    /// edge is a row you can neither see nor click.
    #[test]
    fn the_menu_draws_at_most_the_eight_seeds_the_vault_keeps() {
        let scale = 1.0;
        let vault: Vec<RecentEntry> = (0..12)
            .map(|index| term(&format!("C:\\p{index}"), None, index * 60))
            .collect();
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            &vault,
        );
        assert_eq!(RECENT_CAPACITY, 8, "the vault's own cap, not a second one");
        assert_eq!(layout.recent.len(), RECENT_CAPACITY);
        assert_eq!(
            layout.frame[3] - layout.frame[1],
            (2.0 * ((FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0)
                + MENU_PADDING_LOGICAL_PX * scale)
                + (ITEM_HEIGHT_LOGICAL_PX * scale).round() * PROFILES.len() as f32
                + recent_block(scale, RECENT_CAPACITY))
            .round(),
            "and the menu is only as tall as the rows it draws"
        );

        let layer = one_layer(build(
            &layout,
            &equipped(),
            FALLBACK_PROFILE,
            None,
            &vault,
            now(),
            &mut fake_measure,
        ));
        assert!(
            layer.labels.iter().any(|label| label.text == "p7"),
            "the eighth seed is drawn"
        );
        assert!(
            !layer.labels.iter().any(|label| label.text == "p8"),
            "the ninth is not"
        );
        assert_eq!(
            layer.sprites.len(),
            PROFILES.len() + RECENT_CAPACITY,
            "one mark per drawn row"
        );
    }

    /// PIN — `.menu-label` is the settings dialog's group-heading craft on the
    /// menu's own surface: 10.5px, `600`, `.05em` tracked, `--ink3` over
    /// `--menu`, and drawn uppercase because `text-transform` has no renderer
    /// here.
    #[test]
    fn the_recent_heading_is_uppercase_tracked_and_inked_for_a_menu() {
        let scale = 1.0;
        let vault = [term("C:\\repo", None, 0)];
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            &vault,
        );
        let palette = chrome_palette();
        let layer = one_layer(build(
            &layout,
            &equipped(),
            FALLBACK_PROFILE,
            None,
            &vault,
            now(),
            &mut fake_measure,
        ));
        let heading = layer
            .labels
            .iter()
            .find(|label| label.text == RECENT_SECTION_LABEL)
            .expect("the section is titled");
        assert_eq!(heading.font_size_px, SECTION_LABEL_FONT_LOGICAL_PX * scale);
        assert_eq!(heading.letter_spacing_em, SECTION_LABEL_TRACKING_EM);
        assert_eq!(heading.weight, ChromeLabelWeight::SemiBold);
        assert_eq!(
            heading.color, palette.menu_item_hint_text,
            "`--ink3` over `--menu`, not the dialog's same-named ink"
        );
        assert!(!heading.align_right && !heading.align_center);

        let band = layout.section_label.expect("titled");
        assert_eq!(
            heading.rect[0],
            band[0] + SECTION_LABEL_PADDING_X_LOGICAL_PX * scale,
            "`padding: … 10px …`"
        );
        assert_eq!(
            heading.rect[1],
            band[1] + SECTION_LABEL_PADDING_TOP_LOGICAL_PX * scale,
            "3px above"
        );
        assert_eq!(
            heading.rect[3],
            band[3] - SECTION_LABEL_PADDING_BOTTOM_LOGICAL_PX * scale,
            "5px below, so the line box is centred in its own height"
        );

        // `--border-soft` is the same ink as the menu's own hairline at a
        // lighter alpha, and the two themes declare that alpha separately.
        let rule = layout.separator.expect("separated");
        let hairline = layer
            .quads
            .iter()
            .find(|quad| quad.rect == rule)
            .expect("the rule is drawn");
        assert_eq!(hairline.color, palette.menu_border);
        assert_eq!(hairline.alpha, separator_alpha(palette.menu_border));
        assert_eq!(separator_alpha([0xff, 0xff, 0xff]), SEPARATOR_ALPHA_ON_DARK);
        assert_eq!(
            separator_alpha([0x00, 0x00, 0x00]),
            SEPARATOR_ALPHA_ON_LIGHT
        );
    }

    /// PIN — mock-up 7431: a recent row is called by your own name for it, and
    /// by the folder it stood in when you never gave it one. The leaf rule is
    /// drive-root aware, so `C:\` is `C:` rather than the empty caption a naive
    /// split leaves behind a trailing separator.
    #[test]
    fn a_recent_row_wears_your_name_for_it_or_the_folder_it_stood_in() {
        assert_eq!(cwd_leaf("C:\\Users\\Weiyi\\repo"), "repo");
        assert_eq!(cwd_leaf("C:\\Users\\Weiyi\\repo\\"), "repo");
        assert_eq!(cwd_leaf("C:\\"), "C:", "a drive root names its drive");
        assert_eq!(cwd_leaf("C:"), "C:");
        assert_eq!(
            cwd_leaf("/home/weiyi/src"),
            "src",
            "and forward slashes too"
        );

        let vault = [
            term("C:\\Users\\Weiyi\\repo", Some("build"), 0),
            term("C:\\Users\\Weiyi\\notes", None, 60),
            // `||` in the mock-up falls through an empty string: a row captioned
            // with nothing is a row you cannot tell from the one above it.
            term("C:\\Users\\Weiyi\\empty", Some(""), 120),
            files("D:\\Developer\\BetterTerminal\\", 180),
        ];
        let layout = layout(anchor(1.0), MenuSide::Below, (960.0, 600.0), 1.0, &vault);
        let layer = one_layer(build(
            &layout,
            &equipped(),
            FALLBACK_PROFILE,
            None,
            &vault,
            now(),
            &mut fake_measure,
        ));
        let drawn: Vec<&str> = layer
            .labels
            .iter()
            .map(|label| label.text.as_str())
            .collect();
        for name in ["build", "notes", "empty", "BetterTerminal"] {
            assert!(drawn.contains(&name), "{name} is missing from {drawn:?}");
        }
    }

    /// PIN — mock-up 7427/7431: a terminal seed wears its own profile's mark,
    /// a files locus wears `#i-folder`, and both are `--accent`. The ago label
    /// rides in the `.default-hint` slot the `default` hint already owns.
    #[test]
    fn a_files_seed_wears_the_folder_and_a_terminal_seed_wears_its_profile_s_mark() {
        let scale = 1.0;
        let vault = [files("D:\\notes", 0), term("C:\\repo", None, 3 * 3600)];
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            &vault,
        );
        let palette = chrome_palette();
        let layer = one_layer(build(
            &layout,
            &equipped(),
            FALLBACK_PROFILE,
            None,
            &vault,
            now(),
            &mut fake_measure,
        ));

        let in_row = |row: [f32; 4], sprite: &ChromeSprite| {
            sprite.rect[1] >= row[1] && sprite.rect[3] <= row[3]
        };
        let folder = layer
            .sprites
            .iter()
            .find(|sprite| in_row(layout.recent[0], sprite))
            .expect("the files row wears a mark");
        assert_eq!(folder.mark, ChromeMark::Folder);
        assert_eq!(folder.color, palette.accent);
        let shell = layer
            .sprites
            .iter()
            .find(|sprite| in_row(layout.recent[1], sprite))
            .expect("the terminal row wears a mark");
        assert_eq!(shell.mark, PROFILES[FALLBACK_PROFILE].mark);
        assert_eq!(shell.color, palette.accent);
        // An id this build does not have costs the row its shell choice, never
        // its mark — `index_of_id` falls back rather than refusing.
        assert_eq!(
            recent_mark(&Seed::Term {
                profile_id: "a-shell-from-a-newer-build".to_owned(),
                cwd: "C:\\repo".to_owned(),
                manual_name: None,
            }),
            PROFILES[FALLBACK_PROFILE].mark
        );

        let hint = layer
            .labels
            .iter()
            .find(|label| label.text == "3h ago")
            .expect("a recent row says how long ago");
        assert_eq!(hint.font_size_px, HINT_FONT_LOGICAL_PX * scale);
        assert_eq!(hint.color, palette.menu_item_hint_text);
        assert!(hint.align_right, "`margin-left: auto`");
        assert_eq!(
            hint.rect[2],
            layout.recent[1][2] - ITEM_PADDING_X_LOGICAL_PX * scale,
            "against the row's own trailing padding"
        );
        assert!(
            layer.labels.iter().any(|label| label.text == "just now"),
            "and the newest one says so in the mock-up's own words"
        );
    }

    /// PIN — hovering a recent row lights that row and only that row.
    ///
    /// Red gate: the untagged index again, this time in ink — `Some(0)` used to
    /// mean "the first row", so pointing at the first recent seed lit the
    /// PowerShell row at the top of the menu.
    #[test]
    fn hovering_a_recent_row_lights_it_and_leaves_the_profile_above_it_dark() {
        let scale = 1.0;
        let vault = [term("C:\\repo", Some("build"), 0)];
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            &vault,
        );
        let palette = chrome_palette();
        let layer = one_layer(build(
            &layout,
            &equipped(),
            FALLBACK_PROFILE,
            Some(MenuRow::Recent(0)),
            &vault,
            now(),
            &mut fake_measure,
        ));
        let row = layout.recent[0];
        assert!(
            layer
                .quads
                .iter()
                .any(|quad| quad.color == palette.menu_item_hover
                    && quad.rect[1] >= row[1]
                    && quad.rect[3] <= row[3]),
            "the hovered recent row wears `--hover` over `--menu`"
        );
        assert!(
            layer.labels.iter().any(
                |label| label.text == "build" && label.color == palette.menu_item_text_selected
            ),
            "and steps to `--ink`"
        );
        assert!(
            layer
                .labels
                .iter()
                .any(|label| label.text == "PowerShell" && label.color == palette.menu_item_text),
            "while the profile row it is not stays `--ink2`"
        );
    }
}
