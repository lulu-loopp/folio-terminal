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
    path::{Component, Path, PathBuf, Prefix},
    sync::OnceLock,
    time::{Duration, Instant, SystemTime},
};

use bt_layout::Axis;
use bt_pty::{ShellEnvironment, resolve_powershell_seven};
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
pub const MENU_OFFSET_LOGICAL_PX: f32 = 4.0;
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
/// The box a `×` gets in that same column — **ten, not fifteen** (user ruling,
/// 2026-08-16), and a deliberate deviation from the mock-up.
///
/// `#i-close`'s artwork runs edge to edge of its own `viewBox`: it is a bare
/// cross with no margin, drawn that way because every other place it appears is
/// a *button* whose padding supplies the air. A menu row has no button around
/// it, so struck at the column's full fifteen the cross came out visibly heavier
/// than the folder and the copy glyph beside it — two marks whose artwork is a
/// shape inside a box with its own breathing room. Ten in a fifteen-pixel column
/// gives it the same optical weight as its neighbours, which is what "the same
/// size" actually means for glyphs that are not drawn to the same margins.
///
/// It applies to all three spellings of the cross ([`ChromeMark::WindowClose`],
/// [`ChromeMark::TabClose`], [`ChromeMark::PaneClose`]) rather than to the one a
/// menu happens to use today: they are one drawing under three names, and a rule
/// that named one of them would be a rule the next menu escapes by picking a
/// different alias. Every other mark keeps [`ITEM_MARK_LOGICAL_PX`], the
/// split glyphs included.
const ITEM_MARK_CLOSE_LOGICAL_PX: f32 = 10.0;

/// How big this mark is drawn inside a menu row's icon column.
fn item_mark_logical_px(mark: ChromeMark) -> f32 {
    match mark {
        ChromeMark::WindowClose | ChromeMark::TabClose | ChromeMark::PaneClose => {
            ITEM_MARK_CLOSE_LOGICAL_PX
        }
        _ => ITEM_MARK_LOGICAL_PX,
    }
}
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
/// available guesses — "not on this machine" and "Folio is broken" —
/// are not equally actionable.
const UNAVAILABLE_HINT_TEXT: &str = "not installed";

/// **The `˅` menu's second section: one row, and what it is for** (H113,
/// mock-up 7417-7423).
///
/// Every row above it makes a **tab**; this one adds a **pane** to the tab you
/// are already in, and that is a different enough verb that the mock-up puts a
/// rule between them rather than letting it read as a fifth profile.
const FILES_PANE_TEXT: &str = "Files pane";
/// The annotation that keeps the row honest about the difference.
///
/// It rides the same `.default-hint` slot as `default` and `3m ago`, and it is
/// doing the same job: saying the one thing about the row that its caption does
/// not. Without it "Files pane" sits under four rows that all open new tabs and
/// looks like the fifth.
const FILES_PANE_HINT_TEXT: &str = "this tab";

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
    /// Which spelling of a path this profile's shell speaks — see
    /// [`PathNamespace`]. What makes a directory inherited from another pane
    /// either translatable or honestly refused.
    pub paths: PathNamespace,
    /// What this profile's title has to name before it is unambiguous here.
    pub qualifier: Qualifier,
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
    /// The place is named to the *launcher*, as this flag and one argument,
    /// because the shell does not stand where the launcher does.
    ///
    /// `wsl.exe --cd <place>` is the whole of it, and it is the documented flag
    /// rather than a trick: verified on this machine to answer `/home/weiyi`
    /// from a process standing in `D:\` when handed `~`, and `/mnt/d/Developer`
    /// when handed `/mnt/d/Developer`, where the same launcher with no flag
    /// answers `/mnt/d`.
    ///
    /// Passed *instead of* a working directory and never beside one, because
    /// `--cd` overrides the inherited directory anyway — and because the
    /// directory this profile is given is written in its own namespace
    /// ([`PathNamespace::Wsl`]), which is not a string `CreateProcess` could be
    /// handed. That is the same fact stated twice: the launcher is the only
    /// thing in the chain that speaks both.
    LauncherFlag {
        flag: &'static str,
        /// What the flag is given when nothing has been inherited — the shell's
        /// own `$HOME`, which has no Windows spelling to hand over instead.
        home: &'static str,
    },
}

/// How a profile's executable is located on the machine.
///
/// Two shapes rather than one because they answer to different authorities.
/// [`Self::PowerShellSeven`] defers to a resolution *order* that is already ruled
/// and already tested (`bt_pty::resolve_powershell_seven`: `BT_SHELL`, then a
/// `pwsh` probe); [`Self::FirstOf`] is a list of places to look, in order, for a
/// program that either is on this machine or is not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramSource {
    /// `bt_pty::resolve_powershell_seven`'s answer — `BT_SHELL` first (ruling
    /// 2026-08-10, Q4: the override is **kept**, as a development back door, and
    /// it covers this profile alone rather than becoming another profile's worth
    /// of configuration), then a `pwsh.exe` probe.
    ///
    /// **It stops there, and the missing third step is the point** (user ruling
    /// 2026-08-11). It used to end at `powershell.exe`, back when one row called
    /// `PowerShell` stood for the whole family and the row could not afford to
    /// answer "no". Now that Windows PowerShell has a row of its own, a machine
    /// without PowerShell 7 makes *this* row greyed and truthful instead of
    /// startable and wrong: a user with both installed picks between them, and a
    /// `PowerShell` row that quietly started 5.1 would be the picker lying about
    /// which of the two it ran — on exactly the machines where the difference is
    /// visible.
    PowerShellSeven,
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

/// Which shell-integration script a profile is served by, and **how it gets
/// there** — the two answers are not the same, and the difference is what the
/// honest-capability matrix is made of.
///
/// There is no `None`, and that is the P6 result rather than a tidy-up: every
/// profile this build ships now has a way in. What differs is how far it
/// reaches, and the variants say so — which is exactly the distinction a `None`
/// would have flattened, by spelling "we found no door" and "the door is only
/// wide enough for one marker" the same way.
///
/// The profile with no script is not degraded by a special case: a shell that
/// never emits OSC 133 keeps the cursor/WRAPLINE heuristics, and one that never
/// emits OSC 7 leaves the relative path undetected rather than guessing a
/// directory. Both are the existing, already-implemented conventions
/// (`docs/shell-integration.md` §34-35 and §111-115) — this table confirms they
/// hold for the new shells rather than inventing a second set for them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Integration {
    /// `folio.ps1`, dot-sourced by the user into `$PROFILE`.
    ///
    /// **Opt-in and manual**: this product never injects it
    /// (`docs/shell-integration.md` §83-96), because PowerShell's own startup
    /// file is a single well-known path the user already owns and edits, and a
    /// terminal that rewrote `$PROFILE` behind them would be editing their
    /// shell. There is no argument to hand `pwsh` that would source a second
    /// file *after* theirs, which is the position this integration must occupy.
    PowerShellOptIn,
    /// `folio.bash`, handed to the shell as its init file at spawn.
    ///
    /// Automatic, and the asymmetry with PowerShell is bash's own: `--init-file`
    /// is a documented argument that names the startup file for this one
    /// interactive shell, so the integration can be installed for a session
    /// without touching anything on disk that belongs to the user. What that
    /// argument costs is the startup chain it replaces, which the script itself
    /// puts back — see `scripts/shell-integration/folio.bash`.
    BashInitFile,
    /// No script at all — the whole integration is the `PROMPT` variable
    /// `cmd.exe` prints its prompt from, and what fits in there is **one**
    /// marker: `OSC 7`.
    ///
    /// `PROMPT` is not a hook, it is a format string, and `cmd.exe` expands it
    /// at exactly one moment: just before it reads a line. There is no
    /// pre-execution and no post-execution moment to be called at, so
    /// `OSC 133;C` (a command was submitted) and `OSC 133;D;<code>` (it ended,
    /// with this status) have nowhere to be emitted from. That much was known
    /// (ruling 2026-08-11, Q5).
    ///
    /// **What that ruling assumed and this build disproves is that `A` and `B`
    /// are free.** They are not two more facts, they are a claim of *authority*,
    /// and the machine they claim it from is built on `C` closing what `B`
    /// opened:
    ///
    /// * `133;A` alone turns `shell_integration_is_authoritative` on, and that
    ///   flag's job is to **retire the cursor-line heuristic** — the rule that
    ///   the line under the cursor is probably still being typed and must not be
    ///   decorated yet. Its replacement is the semantic input region, which only
    ///   `B` and `C` can build. A shell that sends `A` and stops has therefore
    ///   switched the protection off and put nothing in its place, and a path
    ///   typed at a `cmd` prompt would light up as a link mid-word.
    /// * `133;B` opens an input region whose only closers are `C` and the *next*
    ///   `A`. Without `C` it stays open across the command's entire run, so
    ///   `typed_shell_input_live` reads the command's own output as an unsent
    ///   buffer: the ConPTY resize gate holds for as long as anything is
    ///   printing, and every resize commit owes an `InvokePrompt` chord to a
    ///   shell with no such binding.
    ///
    /// A third cost is `M2-restart-shell-contract.md` §1.6's: it defines idle as
    /// "已见 OSC 133 A/B、停在提示符", so a `cmd` pane sending A/B would be
    /// classified **idle** and a future `Restart shell` would skip its
    /// confirmation — precisely where we cannot know whether it is busy.
    ///
    /// All three are strictly worse than sending nothing, and sending nothing is
    /// a documented, tested position rather than a gap: a screen that never emits
    /// OSC 133 keeps the cursor/WRAPLINE heuristics byte for byte
    /// (`docs/shell-integration.md`, "Authority and fallback"). So `cmd` stays
    /// there, whole, and spends its one available slot on the marker that has no
    /// bracket to leave dangling. Pinned at
    /// `bt_term::…::a_prompt_that_can_never_send_c_must_not_send_a_or_b_either`.
    CmdPrompt,
}

pub const PROFILES: [Profile; 5] = [
    Profile {
        id: "pwsh",
        // **Two PowerShells, two rows** (user ruling 2026-08-11), which is
        // Windows Terminal's own arrangement and the one a machine with both
        // installed makes necessary: 7 and 5.1 are different products with
        // different language versions, and a single row could only ever start
        // one of them while claiming to be both.
        //
        // **Both rows carry their version** (user ruling 2026-08-11, reversing
        // the bare name this row shipped with). The two were named "PowerShell"
        // and "Windows PowerShell", which is what each product is *called* — and
        // in a tab strip, a tooltip and a picker standing one line apart, it left
        // the user unable to tell which row was 7 and which was 5.1. A name whose
        // job is to distinguish two things has to distinguish them.
        //
        // `7` and not `7.5`: the version is the product line's, which is what the
        // family has been called since it stopped being 6. `5.1` is the whole
        // number because 5.1 is where Windows PowerShell stopped — a fixed value,
        // not a reading. A `pwsh` 8 would be a new line and a new word here.
        //
        // **`scripts/shell-integration/folio.ps1` carries both of these
        // strings and must be changed with them, character for character.** The
        // script titles its session with the edition it is running, and
        // `pane_head_title` drops a program title that merely repeats its own
        // profile's — a shell agreeing with its launcher has announced nothing.
        // That test is string equality, so a rename on one side alone puts the
        // family name back in front of every pane head in the tab. Pinned by
        // `the_integration_script_names_the_profiles_own_titles`.
        title: "PowerShell 7",
        mark: ChromeMark::ProfilePowerShell,
        program: ProgramSource::PowerShellSeven,
        // The flag this terminal has always passed, now said by the profile that
        // means it rather than by the spawn path every profile goes through.
        args: &["-NoLogo"],
        starting_dir: StartingDir::WindowsHome,
        paths: PathNamespace::Windows,
        qualifier: Qualifier::None,
        integration: Integration::PowerShellOptIn,
    },
    Profile {
        id: "winps",
        // The qualifier was always this row's real name rather than one the list
        // invented; the version is the ruling above, and 5.1 is where this product
        // ends rather than where it happens to be.
        title: "Windows PowerShell 5.1",
        // The same mark. The mock-up has one PowerShell symbol and drew no
        // second one, and there is nothing to invent: both rows start a
        // PowerShell, the blue tile is what "a PowerShell is here" looks like,
        // and the titles already say which. A second glyph would be this list
        // asserting a visual distinction the family does not have.
        mark: ChromeMark::ProfilePowerShell,
        // Not `PowerShellSeven`, and not a bare name either: this is the one
        // shell that is *part of Windows*, so it is named where Windows keeps
        // it. That is what lets [`FALLBACK_PROFILE`] be this row — the probe
        // finds it on every Windows there is, so the floor under every other
        // profile is never itself greyed.
        program: ProgramSource::FirstOf(&[ProgramCandidate::Under {
            variable: "SystemRoot",
            tail: r"System32\WindowsPowerShell\v1.0\powershell.exe",
        }]),
        args: &["-NoLogo"],
        starting_dir: StartingDir::WindowsHome,
        paths: PathNamespace::Windows,
        qualifier: Qualifier::None,
        // The same script, and it already handles this shell: `folio.ps1`
        // is written for 5.1 and 7 alike, and the PSReadLine 2.0.0 anchor repair
        // 5.1 needs is an existing no-op sentinel rather than a second code path.
        integration: Integration::PowerShellOptIn,
    },
    Profile {
        id: "wsl",
        // The mock-up writes `WSL · Ubuntu`; this is the half of it that is a
        // constant, and [`Qualifier::WslDistribution`] is the half that is a
        // claim about this machine.
        //
        // The name after the `·` is a **discovery claim**: `wsl.exe` with no
        // arguments starts whatever the user's *default* distribution is, which
        // on one machine is Ubuntu and on the next is Debian or Alpine, so
        // printing "Ubuntu" over a command that will start Debian would be
        // chrome saying something it did not check. It is now checked —
        // `crate::wsl` asks `wsl.exe --list --verbose` which one carries the
        // `*` — and appended only when there is more than one installed and the
        // bare title would therefore be an unanswered question.
        //
        // The constant stays the short form, which is also the mock-up's own
        // rule at line 4013 that a session's name drops everything from the `·`
        // on: a tab falling back to its profile's name is called `WSL`.
        title: "WSL",
        mark: ChromeMark::ProfileUbuntu,
        program: ProgramSource::FirstOf(&[ProgramCandidate::Under {
            variable: "SystemRoot",
            tail: r"System32\wsl.exe",
        }]),
        args: &[],
        // The one profile whose home is not a Windows directory.
        starting_dir: StartingDir::LauncherFlag {
            flag: "--cd",
            home: "~",
        },
        paths: PathNamespace::Wsl,
        qualifier: Qualifier::WslDistribution,
        integration: Integration::BashInitFile,
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
        // **Windows, not MSYS.** Git Bash prints `/d/Developer` and its process
        // is standing in `D:\Developer` — one directory, two spellings, and the
        // Win32 one is the true one: it is what `CreateProcess` was handed, what
        // Explorer opens, and what every other pane in this window speaks. The
        // MSYS spelling is a third namespace that only this shell understands,
        // and the script reports the Win32 one (`pwd -W`) precisely so that it
        // never has to become one.
        paths: PathNamespace::Windows,
        qualifier: Qualifier::None,
        integration: Integration::BashInitFile,
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
        paths: PathNamespace::Windows,
        qualifier: Qualifier::None,
        integration: Integration::CmdPrompt,
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
/// What it must satisfy is one property: it names a program that is **part of
/// Windows**, so it always starts — which is what makes it safe to be the end of
/// every fallback chain (see [`the_fallback_profile_can_always_be_started`]).
/// The *user's* default carries no such guarantee (they can uninstall Git after
/// choosing Git Bash), which is precisely why [`default_profile`] resolves
/// through here.
///
/// **It is `winps` and no longer `pwsh`** (user ruling 2026-08-11), and the move
/// follows the property rather than changing it. The floor used to be the
/// PowerShell row because that row's resolution order *ended* at
/// `powershell.exe`; now that PowerShell 7 has been given a row that answers
/// honestly — greyed on a machine without it — the row that cannot fail is the
/// one that names Windows PowerShell directly. Pointing the floor at a profile
/// that can be greyed would be a fallback chain with a hole at the bottom.
///
/// It is also the more truthful landing for `bt-pty`'s own last-resort retry,
/// which spawns `powershell.exe` when a profile's program will not start: a pane
/// that ends up running 5.1 now says *Windows PowerShell* on its tab instead of
/// wearing the name of the shell it failed to be.
pub const FALLBACK_PROFILE: usize = 1;

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
/// when Folio starts" — and a leaf coming back off disk is neither. A
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

/// Whether this build has a profile by that name at all.
///
/// The question [`index_of_id`] answers *away*: it folds "this profile" and "no
/// such profile, have the default" into one number, which is right for every
/// caller that needs a profile and wrong for the one caller that needs to know a
/// substitution happened. `M2-restart-shell-contract.md` §3 requires that
/// substitution to be visible — "绝不静默替换" — and a function that answers
/// `FALLBACK_PROFILE` for a saved `"pwsh"` and a saved `"fish"` alike cannot tell
/// anyone which of the two it was looking at.
#[must_use]
pub fn has_id(id: &str) -> bool {
    PROFILES.iter().any(|profile| profile.id == id)
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
/// invites "where did you look?"; "Folio could not find Git Bash" makes
/// the terminal the subject of a sentence whose subject is the machine. `— not
/// found on this machine` says the search happened, that it was for a real thing,
/// and that the answer is about this computer rather than about the product.
#[must_use]
pub fn unavailable_tip(profile: usize) -> String {
    format!("{} — not found on this machine", title(profile))
}

/// What, if anything, this profile's title has to name before it is unambiguous
/// on the machine it is being read on.
///
/// A profile's [`Profile::title`] is a constant, and for three of the four that
/// is the whole truth: `Command Prompt` names one program. `WSL` names a
/// *launcher*, and which shell it launches is a fact about this machine — the
/// mock-up writes `WSL · Ubuntu` (line 2598) and P1 shipped the bare `WSL`
/// precisely because printing "Ubuntu" over a command that would start Debian is
/// chrome saying something it did not check. This field is where the checking
/// gets attached.
///
/// Its own field rather than keyed off [`PathNamespace::Wsl`], although today
/// exactly one profile has each. The two would come apart the moment the profile
/// editor (K86) lets somebody make a second WSL profile pinned to a *named*
/// distribution: that profile's paths are still WSL's, and its title is already
/// complete — qualifying it with the machine's *default* distribution would be a
/// title that names the wrong one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Qualifier {
    /// The constant title is the whole title.
    None,
    /// Append the distribution `wsl.exe` starts by default, when this machine
    /// has more than one to choose between.
    WslDistribution,
}

/// The title this machine spells `profile` by — the constant, plus whatever
/// [`Qualifier`] it earns here.
///
/// Composed once and kept for the life of the process, which is what lets every
/// reader keep the `&'static str` it already had: a menu row, a tooltip and a
/// settings option all name a profile, they are scattered through layout code
/// that has no other reason to know what WSL is, and threading a probe result
/// down to each of them would put that knowledge in all of it.
///
/// Once is also correct rather than merely convenient. [`crate::wsl::facts`] is
/// a statement about an installation, `ProfilePrograms` next to it is probed
/// once for the same reason, and a title that changed between the frame a row
/// was read on and the click aimed at it is a worse answer than a settled one.
#[must_use]
pub fn title(profile: usize) -> &'static str {
    static TITLES: OnceLock<[String; PROFILES.len()]> = OnceLock::new();
    &TITLES.get_or_init(|| {
        let qualifier = crate::wsl::facts().title_qualifier();
        std::array::from_fn(|index| compose_title(index, qualifier))
    })[profile]
}

/// [`title`]'s rule, without the cache — `Profile.title`, then the qualifier
/// this machine earned, joined the way the mock-up joins them.
///
/// ` · ` with spaces around it, which is the mock-up's own spelling and is also
/// what makes the mock-up's *other* rule expressible: a session's name is
/// everything before `" ·"` (line 4013), so a tab that falls back to its
/// profile's name is called `WSL` and not `WSL · Ubuntu-24.04`. That rule needs
/// no code here — the short form **is** the constant, and the only place the
/// qualifier is added is the place a long name fits.
fn compose_title(profile: usize, qualifier: Option<&str>) -> String {
    match (PROFILES[profile].qualifier, qualifier) {
        (Qualifier::WslDistribution, Some(distribution)) => {
            format!("{} · {distribution}", PROFILES[profile].title)
        }
        _ => PROFILES[profile].title.to_owned(),
    }
}

/// Which spelling of a filesystem path a profile's shell speaks.
///
/// Not a property of the *program* but of the world it stands in. Three of these
/// profiles are Windows processes whose working directory is a Win32 directory,
/// and they say so in drive letters. WSL's shell lives inside the distribution's
/// own filesystem, where this machine's `D:\Developer` is `/mnt/d/Developer` and
/// `/home/weiyi` is a place a drive letter cannot reach at all.
///
/// The field exists because a directory travelling between two panes is only
/// meaningful with the namespace it was written in attached, and the pane
/// already carries the one thing that knows it: its profile. Without this,
/// `C:\Users\Weiyi` inherited into a WSL tab is a string that names nothing, and
/// the pane opens in a place nobody chose while looking as though it worked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathNamespace {
    /// `D:\Developer` — a Win32 path, drive-rooted.
    Windows,
    /// `/mnt/d/Developer`, `/home/weiyi` — the distribution's own filesystem.
    Wsl,
}

/// `C:\Users\Weiyi` → `/mnt/c/Users/Weiyi`, or `None` when this path names
/// nothing a WSL shell can stand in.
///
/// The drive map is WSL's own and it is a documented, stable mount rule rather
/// than a convention we are inventing: every fixed drive appears under `/mnt/`
/// at its lower-cased letter. Verified against the launcher itself on this
/// machine — `wsl.exe --cd 'D:\Developer' -- pwd` answers `/mnt/d/Developer`,
/// which is the same answer this function gives.
///
/// `None` for everything that is not drive-rooted, and each refusal is a real
/// case rather than defensive noise:
///
/// * a UNC share (`\\server\share`, and the `\\wsl.localhost\…` spelling
///   `wslpath -w` produces) is not mounted under `/mnt` at all;
/// * a relative or drive-relative path (`src`, `C:src`) does not name a place
///   without knowing where somebody was standing, and that somebody is not this
///   process;
/// * a `..` or `.` left in the path is a directory nobody has resolved, and
///   resolving it here would be this module guessing at a filesystem it cannot
///   see.
///
/// The parse goes through [`std::path::Component`] rather than through the
/// string, so the shapes Windows actually has — including the verbatim
/// `\\?\C:\…` spelling of a drive — are classified by the platform's own parser
/// instead of by a prefix test that would read `\\?\C:\x` as a UNC share.
#[must_use]
pub fn windows_to_wsl(path: &Path) -> Option<PathBuf> {
    let mut components = path.components();
    let Component::Prefix(prefix) = components.next()? else {
        return None;
    };
    let (Prefix::Disk(letter) | Prefix::VerbatimDisk(letter)) = prefix.kind() else {
        return None;
    };
    if components.next() != Some(Component::RootDir) {
        return None;
    }
    let mut translated = format!("/mnt/{}", char::from(letter).to_ascii_lowercase());
    for component in components {
        let Component::Normal(name) = component else {
            return None;
        };
        translated.push('/');
        translated.push_str(name.to_str()?);
    }
    Some(PathBuf::from(translated))
}

/// `/mnt/c/Users/Weiyi` → `C:\Users\Weiyi`, or `None` when Windows has no name
/// for this place.
///
/// The inverse is **not total**, and that asymmetry is the whole reason a
/// translation can fail. `/home/weiyi` is a directory inside the distribution's
/// own root filesystem; the only Windows spelling of it is the
/// `\\wsl.localhost\<distro>\home\weiyi` share, which is a network path to a
/// service rather than a directory — it needs the distribution running, it is
/// not what `cd` in that shell means, and it is precisely the authority a
/// `file://` report is obliged to reject as remote. So the honest answer is that
/// there is no answer, and the caller falls back to the target profile's own
/// starting directory instead of opening somewhere plausible-looking.
///
/// `/mnt/cdrom` is not a drive: the segment after `/mnt/` has to be a single
/// ASCII letter, because that is what makes it one of WSL's drive mounts rather
/// than an ordinary directory somebody made.
#[must_use]
pub fn wsl_to_windows(path: &Path) -> Option<PathBuf> {
    let (drive, tail) = match path.to_str()?.strip_prefix("/mnt/")?.split_once('/') {
        Some((drive, tail)) => (drive, tail),
        None => (path.to_str()?.strip_prefix("/mnt/")?, ""),
    };
    let &[letter] = drive.as_bytes() else {
        return None;
    };
    if !letter.is_ascii_alphabetic() {
        return None;
    }
    let mut translated = format!("{}:\\", char::from(letter).to_ascii_uppercase());
    if !tail.is_empty() {
        translated.push_str(&tail.replace('/', "\\"));
    }
    Some(PathBuf::from(translated))
}

/// Where `cwd` — a directory written in `from`'s namespace — is, said in `to`'s,
/// or `None` when `to` has no name for it.
#[must_use]
pub fn translate_cwd(from: PathNamespace, to: PathNamespace, cwd: &Path) -> Option<PathBuf> {
    match (from, to) {
        (PathNamespace::Windows, PathNamespace::Windows)
        | (PathNamespace::Wsl, PathNamespace::Wsl) => Some(cwd.to_path_buf()),
        (PathNamespace::Windows, PathNamespace::Wsl) => windows_to_wsl(cwd),
        (PathNamespace::Wsl, PathNamespace::Windows) => wsl_to_windows(cwd),
    }
}

/// The directory a new leaf of `target` opens in when it is born beside a leaf
/// of `source` that is standing in `cwd` — P4's replacement for "only from its
/// own profile".
///
/// P3 answered this by refusing every crossing pair, which was the conservative
/// stand-in stated as temporary at the time: two profiles' directories are
/// written in two namespaces, and carrying one across unconverted names nothing.
/// The test it enforced was *"is this the same profile"*; the test now is **"can
/// the target say where you are standing"**, which is the question that was
/// always being asked. Every pair that can, inherits — a PowerShell in
/// `D:\Developer` opens a WSL tab in `/mnt/d/Developer`, and a WSL shell in
/// `/mnt/d/Developer` opens a PowerShell in `D:\Developer` — and the pairs that
/// cannot fall through to the target profile's own starting directory rather
/// than to a guess (`docs/shell-integration.md` §34-35).
#[must_use]
pub fn cwd_for_spawn(source: usize, target: usize, cwd: Option<&Path>) -> Option<PathBuf> {
    translate_cwd(PROFILES[source].paths, PROFILES[target].paths, cwd?)
}

/// Where a leaf is to be started, in the two forms a spawn can actually say it.
///
/// Both at once rather than an either/or, because they are not alternatives —
/// they are the two channels a process launch has, and a profile uses whichever
/// one its program listens on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpawnPlace {
    /// Handed over as the child's working directory. `None` when this machine
    /// cannot name the place — the shell then starts where a process started by
    /// this one starts, which is what every leaf did before there was a starting
    /// directory at all: an unchanged answer rather than a guessed one.
    pub working_directory: Option<PathBuf>,
    /// Appended to the profile's own arguments.
    pub arguments: Vec<OsString>,
}

/// A directory a leaf of `profile` was saved standing in, if it is still a
/// directory — the existence check that guards every revival, asked in the
/// namespace the path is written in.
///
/// `is_dir()` is a Win32 question, and asking it of `/mnt/d/Developer` answers
/// **no** on a machine where that directory is perfectly fine: nothing under
/// `/mnt` exists as far as Windows is concerned, so an unguarded check would
/// drop the directory of every WSL pane it ever restored, silently, and every
/// revived WSL tab would come back at `~`. The check is therefore asked only
/// where it can be answered, and a WSL directory is taken at its word — if it
/// has since been deleted, `wsl.exe --cd` reports that itself, in the pane,
/// which is an honest answer this side could not have produced anyway.
#[must_use]
pub fn revived_cwd(profile: usize, cwd: &Path) -> Option<PathBuf> {
    match PROFILES[profile].paths {
        PathNamespace::Windows => cwd.is_dir().then(|| cwd.to_path_buf()),
        PathNamespace::Wsl => Some(cwd.to_path_buf()),
    }
}

/// Where a leaf of `profile` opens, and how its launcher is told — the
/// resolution of [`Profile::starting_dir`] and an inherited directory against
/// this machine.
///
/// `inherited` is what [`cwd_for_spawn`] handed back, **already in this
/// profile's own namespace**; this function only decides which channel it
/// travels on. Both inputs meet here rather than at the call site because the
/// channel is a property of the profile and the P3-era split — cwd through one
/// path, the profile's home through another — is what let a WSL leaf's inherited
/// directory be handed to `CreateProcess` as if it were a Windows path.
///
/// Read at spawn rather than probed once like [`ProfilePrograms`], and the
/// difference is the rate: availability is asked by the *paint*, four times a
/// frame for as long as a menu is open, while this is asked once per shell
/// started. A value cached for the life of the process would be trading nothing
/// for a home directory that cannot then follow a `%USERPROFILE%` the user
/// changed under us.
/// `%USERPROFILE%`, the place this machine calls home.
///
/// Named here rather than read at the one call site because it is already the
/// rule [`spawn_place`] applies for [`StartingDir::WindowsHome`], and "where
/// does a thing start when nothing else says" must have exactly one answer. A
/// files pane opened from a shell that has never reported a folder falls back to
/// it (H115), which is the same fallback a shell of that profile would take.
///
/// Read through the environment rather than cached for the life of the process,
/// for the reason [`spawn_place`]'s own note gives: a cached home cannot follow a
/// `%USERPROFILE%` the user changed under us.
#[must_use]
pub fn home_directory(environment: &dyn ShellEnvironment) -> Option<PathBuf> {
    environment.var_os("USERPROFILE").map(PathBuf::from)
}

#[must_use]
pub fn spawn_place(
    profile: usize,
    inherited: Option<PathBuf>,
    environment: &dyn ShellEnvironment,
) -> SpawnPlace {
    match PROFILES[profile].starting_dir {
        StartingDir::WindowsHome => SpawnPlace {
            working_directory: inherited
                .or_else(|| environment.var_os("USERPROFILE").map(PathBuf::from)),
            arguments: Vec::new(),
        },
        StartingDir::LauncherFlag { flag, home } => SpawnPlace {
            working_directory: None,
            arguments: vec![
                OsString::from(flag),
                inherited.map_or_else(|| OsString::from(home), PathBuf::into_os_string),
            ],
        },
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

/// The installs `git.exe` is looked for in when it is not on `PATH`.
///
/// The same three the Git Bash profile falls back to — the system-wide, the
/// 32-bit and the per-user installers' defaults — pointed at `cmd\git.exe`
/// instead of `bin\bash.exe`, because they are two files of one install.
const GIT_FALLBACKS: &[ProgramCandidate] = &[
    ProgramCandidate::Under {
        variable: "ProgramFiles",
        tail: r"Git\cmd\git.exe",
    },
    ProgramCandidate::Under {
        variable: "ProgramFiles(x86)",
        tail: r"Git\cmd\git.exe",
    },
    ProgramCandidate::Under {
        variable: "LocalAppData",
        tail: r"Programs\Git\cmd\git.exe",
    },
];

/// Where `git.exe` is on this machine, or `None` when it is nowhere.
///
/// **`PATH` first, and it is more than a shortcut.** The Git block asks `git`
/// questions whose answers sit three inches from a pane where the user types
/// `git status` themselves — so the binary that answers must be *the one they
/// are already using*, not merely one that exists. `PATH` names that binary; the
/// three fallbacks under it only catch a machine where Git was installed but
/// never put on the path, and a Git found that way is still the only one there
/// is.
///
/// This is the same locator the `gitbash` profile resolves through, one level
/// down: that profile asks `PATH` for `git.exe` in order to find `bash.exe`
/// *beside* it, and this asks for the anchor itself. One implementation of "is
/// there a Git on this machine", not two that can disagree about it.
///
/// `None` is an answer and not a failure (W5): a machine with no Git gets a Git
/// page that says so once, and every other part of the product is untouched.
#[must_use]
pub fn find_git(environment: &dyn ShellEnvironment) -> Option<PathBuf> {
    search_path(environment, "git.exe").or_else(|| {
        GIT_FALLBACKS.iter().copied().find_map(|candidate| {
            ProfilePrograms::candidate_path(candidate, environment)
                .filter(|path| environment.is_file(path))
        })
    })
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
                // A real `None` on a machine with no PowerShell 7, which is
                // what greys the row rather than starting 5.1 under 7's name.
                ProgramSource::PowerShellSeven => resolve_powershell_seven(environment),
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
    pub(crate) fn candidate_path(
        candidate: ProgramCandidate,
        environment: &dyn ShellEnvironment,
    ) -> Option<PathBuf> {
        match candidate {
            ProgramCandidate::Under { variable, tail } => {
                Some(Path::new(&environment.var_os(variable)?).join(tail))
            }
            ProgramCandidate::BesideOnPath { anchor, tail } => {
                let found = search_path(environment, anchor)?;
                // The anchor's install root is some ancestor of wherever PATH
                // found it — `<root>\cmd\git.exe` in a plain shell, but
                // `<root>\mingw64\bin\git.exe` when PATH was set up by Git Bash
                // itself. Walking every ancestor and asking which one truly
                // carries the tail answers both spellings; a fixed two-step
                // climb answered only the first and greyed the profile out on
                // the second.
                found
                    .ancestors()
                    .skip(1)
                    .map(|ancestor| ancestor.join(tail))
                    .find(|candidate| environment.is_file(candidate))
            }
        }
    }

    /// Whether this profile can do what its row says it does.
    ///
    /// The picker draws a profile it cannot start greyed rather than hiding it
    /// (user ruling 2026-08-10): the row is the product saying "this is a thing
    /// Folio opens", and the grey is it saying "not on this machine".
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
    /// Give the tab you are looking at a files column (H113).
    ///
    /// Carries no index, and that is the third thing the tag is load-bearing
    /// for: this row indexes nothing. Both variants above name a position in a
    /// list, and a row that names no list is precisely the row that would have
    /// been mis-read as `Profile(4)` if the menu had gone on counting rows.
    FilesPane,
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
    /// The `.menu-sep` above the `Files pane` row.
    ///
    /// Unconditional where the Recent separator is optional, and the asymmetry
    /// is the mock-up's: a Recent heading over an empty list is a promise the
    /// menu cannot keep, while `Files pane` is always available — every tab can
    /// be given a files column.
    files_separator: [f32; 4],
    /// The `Files pane` row itself.
    files_pane: [f32; 4],
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
    /// Whether this point is on the menu at all.
    ///
    /// [`hit`] answers the same question and more, and cannot be used for it: it
    /// needs the machine's profile list and the seed vault, because a *row* has
    /// to know whether it can be chosen. The `⌄`'s leave grace asks something
    /// much smaller — "is the pointer still on the pair" — and asking it through
    /// the bigger question would make the grace depend on which shells are
    /// installed.
    #[must_use]
    pub fn contains(&self, x: f32, y: f32) -> bool {
        contains(self.frame, x, y)
    }

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
    measure: &mut dyn FnMut(&str, f32) -> f32,
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
    // The second section is one rule and one row, and it is never absent.
    let files_block = separator_block + item_height;

    // **`min-width`, at last read as a minimum.** The mock-up's menu is
    // content-sized — `min-width: 180px` over `white-space: nowrap` rows — and
    // this took that declaration for a fixed width, which was survivable only
    // while every row fitted inside it. `Windows PowerShell` does not: with its
    // `default` annotation beside it the pair wants about 200px, and a fixed
    // 180 clipped the name mid-glyph in the out-of-the-box configuration, with
    // no ellipsis to say so.
    //
    // The annotation slot always reserves the **widest** annotation a row could
    // carry rather than the one it happens to carry today, so that changing the
    // default profile — or unplugging the drive Git lives on — cannot make the
    // menu change width under the pointer.
    let annotation = measure(HINT_TEXT, px(HINT_FONT_LOGICAL_PX))
        .max(measure(UNAVAILABLE_HINT_TEXT, px(HINT_FONT_LOGICAL_PX)));
    // Measured before the closure below borrows `measure` for the rest of the
    // function, not because the order matters to the layout.
    let files_hint = measure(FILES_PANE_HINT_TEXT, px(HINT_FONT_LOGICAL_PX));
    let mut row_content = |name: &str, font: f32, annotation: f32| {
        px(ITEM_ICON_COLUMN_LOGICAL_PX)
            + px(ITEM_GAP_LOGICAL_PX)
            + measure(name, font)
            + px(ITEM_GAP_LOGICAL_PX)
            + annotation
    };
    let chrome = 2.0 * (border + padding) + 2.0 * px(ITEM_PADDING_X_LOGICAL_PX);
    // The **profile** rows decide the width, and the Recent rows do not.
    //
    // Not an oversight: a profile's title is this module's own (a constant, or a
    // constant plus a qualifier this machine answered), so its length is a fact
    // the product is responsible for and must make room for. A Recent row's name
    // is a *directory* — arbitrary length, chosen by nobody here — and letting
    // one stretch the popup across the window is exactly what mock-up 1030's
    // `max-width: 260px` exists to prevent. Recent rows therefore go on being
    // clamped into whatever width the profile rows established, which is the
    // behaviour they already had; the ellipsis that clamp still owes them
    // (mock-up 1031) is unchanged and still outstanding.
    // The `Files pane` row joins the profiles in deciding the width, for the
    // same reason they do and not the reason Recent does not: its caption and
    // its annotation are both this module's own constants, so their length is a
    // fact the product is responsible for making room for.
    let files_row = row_content(FILES_PANE_TEXT, px(ITEM_FONT_LOGICAL_PX), files_hint);
    let content = (0..PROFILES.len())
        // `title(index)` and not `Profile::title`: the qualifier is part of the
        // string the row draws, and on a machine with two distributions it is
        // the longest row in the list.
        .map(|index| row_content(title(index), px(ITEM_FONT_LOGICAL_PX), annotation))
        .fold(files_row, f32::max);
    let width = (chrome + content)
        .max(px(MENU_MIN_WIDTH_LOGICAL_PX))
        .round();
    let height = (2.0 * (border + padding)
        + item_height * PROFILES.len() as f32
        + files_block
        + recent_block)
        .round();
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
    // The second section, laid down before Recent so the menu reads in the order
    // the mock-up writes it: what makes a tab, then what makes a pane, then what
    // brings one back.
    let files_separator = [
        content_left,
        cursor + separator_margin,
        content_right,
        cursor + separator_margin + separator_thickness,
    ];
    cursor += separator_block;
    let files_pane = [content_left, cursor, content_right, cursor + item_height];
    cursor += item_height;
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
        files_separator,
        files_pane,
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
    // Always available: every tab can be given a files column, so unlike a
    // profile row there is no machine to probe for and no greyed state to be in.
    if contains(layout.files_pane, x, y) {
        return Some(Some(MenuRow::FilesPane));
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
                mark: Some(profile.mark),
                name: title(index),
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
                hint_ink: None,
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

    // ── the second section: `Files pane` ───────────────────────────────────
    quads.push(OverlayQuad {
        rect: layout.files_separator,
        color: palette.menu_border,
        alpha: separator_alpha(palette.menu_border),
    });
    push_row(
        &Row {
            rect: layout.files_pane,
            // The **generic** folder, not a profile's own artwork: this row
            // names a kind of pane, and every profile mark in the list above it
            // names a shell. Borrowing one here would say the tree belongs to
            // whichever shell lent its glyph.
            mark: Some(ChromeMark::Folder),
            name: FILES_PANE_TEXT,
            hint: Some(hint(FILES_PANE_HINT_TEXT.to_owned())),
            hint_ink: None,
            hovered: hover == Some(MenuRow::FilesPane),
            // A files column needs no program behind it, so there is nothing
            // this machine could be missing and no greyed state to reach.
            available: true,
        },
        scale,
        palette,
        &mut quads,
        &mut labels,
        &mut sprites,
    );

    if let Some(rule) = layout.separator {
        quads.push(OverlayQuad {
            rect: rule,
            color: palette.menu_border,
            alpha: separator_alpha(palette.menu_border),
        });
    }

    if let Some(band) = layout.section_label {
        labels.push(section_label(RECENT_SECTION_LABEL, band, scale, palette));
    }

    for (index, (row, entry)) in layout.recent.iter().zip(menu_rows(recent)).enumerate() {
        push_row(
            &Row {
                rect: *row,
                mark: Some(recent_mark(&entry.seed)),
                name: recent_label(&entry.seed),
                // Still the age, and deliberately not `not installed`: a Recent
                // row's one annotation answers "when", the grey already answers
                // "can you", and losing the timestamp would cost the row the
                // only thing that orders it against its neighbours.
                hint: Some(hint(ago_label(entry.at, now))),
                hint_ink: None,
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
/// `.menu-label` / `.rm-label` — a heading over a list, in the one form both
/// popups wear it.
fn section_label(text: &str, band: [f32; 4], scale: f32, palette: ChromePalette) -> ChromeLabel {
    let px = |value: f32| value * scale;
    ChromeLabel {
        text: text.to_owned(),
        // The band's content box: padding stripped, so the 12.5px line box is
        // centred in exactly its own height and the 3px above it and 5px below
        // it stay the stylesheet's rather than the renderer's.
        rect: [
            band[0] + px(SECTION_LABEL_PADDING_X_LOGICAL_PX),
            band[1] + px(SECTION_LABEL_PADDING_TOP_LOGICAL_PX),
            band[2] - px(SECTION_LABEL_PADDING_X_LOGICAL_PX),
            band[3] - px(SECTION_LABEL_PADDING_BOTTOM_LOGICAL_PX),
        ],
        font_size_px: px(SECTION_LABEL_FONT_LOGICAL_PX),
        // `--ink3` over `--menu` — the same ink the row hints wear, because it
        // is the same declaration on the same surface.
        color: palette.menu_item_hint_text,
        align_right: false,
        align_center: false,
        letter_spacing_em: SECTION_LABEL_TRACKING_EM,
        weight: ChromeLabelWeight::SemiBold,
        tabular_numerals: false,
        clip: None,
    }
}

struct Row<'a> {
    rect: [f32; 4],
    /// The glyph in the row's icon column, or **nothing at all**.
    ///
    /// `None` since the graph's branch filter (T2, v2 (3)): an unticked checkbox
    /// is an empty box and this window draws an empty box as empty space — a
    /// hollow square would be a second container idiom beside the ring the radio
    /// rows already use, and at a menu row's fourteen pixels a square and a
    /// circle differing only in their corners is a distinction nobody reads. The
    /// column is still *reserved*, so the names of ticked and unticked rows line
    /// up; what changes is whether anything stands in it.
    mark: Option<ChromeMark>,
    name: &'a str,
    /// The `.default-hint` slot and **its measured width**: `default` on the
    /// default profile, `3m ago` on a recent row, `not installed` on one this
    /// machine cannot start.
    ///
    /// The width travels with the words because the two are used together and
    /// once: the hint is right-aligned into it and the name's box ends where it
    /// begins.
    hint: Option<(String, f32)>,
    /// What the hint is set in, when it is not the menu's own quiet report ink.
    ///
    /// One caller: the preview switcher's dirty dot, which is `--accent` because
    /// it is the same dot the header wears (mock-up 580-582). Everything else
    /// leaves it alone and gets `--ink3` — a hint that *reports* rather than
    /// *warns*, which is why the ink is a parameter and not a rule.
    hint_ink: Option<[u8; 3]>,
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
    // does with a child one pixel wider than the box it is in — or the 10px box
    // a `×` gets instead, centred in exactly the same column so that a row with
    // a cross and a row with a folder still line their names up. See
    // [`ITEM_MARK_CLOSE_LOGICAL_PX`].
    let column_left = item[0] + px(ITEM_PADDING_X_LOGICAL_PX);
    let column_right = column_left + px(ITEM_ICON_COLUMN_LOGICAL_PX);
    if let Some(glyph) = row.mark {
        let mark = px(item_mark_logical_px(glyph)).round();
        let mark_left = ((column_left + column_right - mark) / 2.0).round();
        let mark_top = ((item[1] + item[3] - mark) / 2.0).round();
        let mut sprite = ChromeSprite::new(
            glyph,
            [mark_left, mark_top, mark_left + mark, mark_top + mark],
            palette.accent,
        );
        if !row.available {
            sprite.opacity = UNAVAILABLE_MARK_OPACITY;
            sprite.grayscale = true;
        }
        sprites.push(sprite);
    }
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
            color: row.hint_ink.unwrap_or(palette.menu_item_hint_text),
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
pub(crate) fn cwd_leaf(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['\\', '/']);
    let leaf = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed);
    if leaf.is_empty() { trimmed } else { leaf }
}

// ── `.root-menu` — where a files column is pointed (E53-E61) ───────────────
//
// **Why it lives in this file.** It is not a profile picker and it says so in
// its own names; what it *is* is the same popup — the same float window, the
// same 29.5px row, the same mark column, gap, ink and hover fill — hung off a
// different button. `push_row`'s own comment gives the reason two lists that
// look alike are drawn by one function: "the way two menu rows drift apart is
// that somebody fixes the ink on one of them". A second module would mean a
// second copy of fifteen numbers, and the first theme change would separate
// them.

/// Why a folder is being offered (mock-up 5049-5057).
///
/// The note is the menu's honesty: "a terminal is here" is the reason that path
/// is on the list at all, and a list of bare paths would make the user guess
/// which of them the app thinks is interesting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootNote {
    Home,
    /// One of this window's shells is standing in it.
    Terminal,
    /// The folder this column's root is in.
    Parent,
}

impl RootNote {
    pub fn text(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Terminal => "a terminal is here",
            Self::Parent => "parent",
        }
    }
}

/// One place the menu offers to point a column at.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootChoice {
    pub path: String,
    pub note: RootNote,
}

/// The places worth offering, in the mock-up's own order (E54).
///
/// **Home, then wherever the shells are standing, then one level up.** The order
/// is not alphabetical and is not most-recent-first: it runs from the most
/// permanent address this machine has to the most local one, so the list reads
/// the same on every window whatever the shells happen to be doing.
///
/// De-duplicated on the path and keeping the *first* note, so a home directory
/// a terminal happens to be standing in is offered once and called home.
#[must_use]
pub fn root_choices(root: &str, home: Option<&str>, cwds: &[String]) -> Vec<RootChoice> {
    let mut list: Vec<RootChoice> = Vec::new();
    let mut add = |path: &str, note: RootNote| {
        let path = path.trim();
        if path.is_empty() || list.iter().any(|choice| choice.path == path) {
            return;
        }
        list.push(RootChoice {
            path: path.to_owned(),
            note,
        });
    };
    if let Some(home) = home {
        add(home, RootNote::Home);
    }
    for cwd in cwds {
        add(cwd, RootNote::Terminal);
    }
    // The parent of the root, which the mock-up computes by trimming trailing
    // separators and then one segment. `Path::parent` is that, done by a
    // component walk that knows what a drive prefix is — so `C:\` has no parent
    // rather than an empty string, and a root already at the top of its drive
    // simply does not offer the row.
    if let Some(parent) = Path::new(root.trim_end_matches(['\\', '/']))
        .parent()
        .map(Path::to_string_lossy)
        .filter(|parent| !parent.is_empty())
    {
        add(&parent, RootNote::Parent);
    }
    list
}

/// A row of the root menu.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootMenuRow {
    /// An index into the [`root_choices`] the menu was laid out from.
    Choice(usize),
    /// `Browse…` — the escape hatch to any folder at all (E55).
    ///
    /// Its own variant rather than a last index, because it is the one row whose
    /// meaning is not "go to this named place": the list above it is a set of
    /// answers and this is the question re-asked of the system. Keeping it out
    /// of the index space also means a menu whose choices changed underneath a
    /// press cannot turn a stale index into "browse", or the reverse.
    Browse,
}

/// Which column's root menu is up, and which row the pointer is on.
///
/// The seat is *in* the state rather than beside it, which is what makes the
/// menu single by construction: opening one on another column replaces this,
/// and the chevron the old column was wearing re-derives from here on the very
/// next frame rather than having to be un-flipped by hand (E57's whole bug).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RootMenu {
    open: Option<bt_layout::SeatId>,
    hover: Option<RootMenuRow>,
}

impl RootMenu {
    pub fn seat(self) -> Option<bt_layout::SeatId> {
        self.open
    }

    /// The button: open here, or shut if this very column already has it open.
    pub fn toggle(&mut self, seat: bt_layout::SeatId) {
        self.open = (self.open != Some(seat)).then_some(seat);
        self.hover = None;
    }

    pub fn close(&mut self) -> bool {
        let was_open = self.open.is_some();
        self.open = None;
        self.hover = None;
        was_open
    }

    pub fn set_hover(&mut self, hover: Option<RootMenuRow>) -> bool {
        let hover = self.open.and(hover);
        let changed = self.hover != hover;
        self.hover = hover;
        changed
    }

    pub fn hover(self) -> Option<RootMenuRow> {
        self.hover
    }
}

/// Every rectangle the root menu draws and hit-tests.
#[derive(Clone, Debug, PartialEq)]
pub struct RootMenuLayout {
    scale: f32,
    frame: [f32; 4],
    label: [f32; 4],
    items: Vec<[f32; 4]>,
    /// The hairline above `Browse…` — unconditional, because the row below it is
    /// unconditional too. The profile menu's is an `Option` only because the
    /// Recent section it introduces can be empty.
    browse_separator: [f32; 4],
    browse: [f32; 4],
}

impl RootMenuLayout {
    /// Every row paired with the path it stands for, so a caption showing only
    /// the last segment can hang the whole path off itself.
    pub fn tips<'a>(
        &'a self,
        choices: &'a [RootChoice],
    ) -> impl Iterator<Item = (RootMenuRow, [f32; 4], String)> + 'a {
        self.items
            .iter()
            .zip(choices)
            .enumerate()
            .map(|(index, (rect, choice))| (RootMenuRow::Choice(index), *rect, choice.path.clone()))
    }
}

/// The root menu hung under the head's root button.
///
/// `top = anchor.bottom + 4`, `left = clamp(anchor.left)` — mock-up 5169-5175,
/// which is the same two lines [`layout`] uses for [`MenuSide::Below`], because
/// it is the same gesture: a button on a horizontal surface with the window
/// below it.
#[must_use]
pub fn root_menu_layout(
    anchor: [f32; 4],
    surface: (f32, f32),
    scale: f32,
    choices: &[RootChoice],
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> RootMenuLayout {
    let px = |value: f32| value * scale;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let padding = px(MENU_PADDING_LOGICAL_PX);
    let item_height = px(ITEM_HEIGHT_LOGICAL_PX).round();
    let separator_thickness = (SEPARATOR_THICKNESS_LOGICAL_PX * scale).round().max(1.0);
    let separator_margin = px(SEPARATOR_MARGIN_Y_LOGICAL_PX).round();
    let separator_block = 2.0 * separator_margin + separator_thickness;
    let section_block = px(SECTION_LABEL_PADDING_TOP_LOGICAL_PX
        + SECTION_LABEL_LINE_LOGICAL_PX
        + SECTION_LABEL_PADDING_BOTTOM_LOGICAL_PX)
    .round();

    // The widest note any row could carry, reserved for every row — the same
    // rule the profile menu's annotation follows and for the same reason: a
    // menu that changed width because a shell moved would move under the
    // pointer.
    let note = [RootNote::Home, RootNote::Terminal, RootNote::Parent]
        .into_iter()
        .map(|note| measure(note.text(), px(HINT_FONT_LOGICAL_PX)))
        .fold(0.0, f32::max);
    let chrome = 2.0 * (border + padding) + 2.0 * px(ITEM_PADDING_X_LOGICAL_PX);
    // Every row's name is a *directory* — arbitrary length, chosen by nobody
    // here — so the widest one does not get to stretch the popup across the
    // window. It is the same clamp `RECENT_ITEM_MAX_WIDTH_LOGICAL_PX` puts on
    // the Recent rows, applied to the whole menu because here every row is one.
    let content = choices
        .iter()
        .map(|choice| {
            px(ITEM_ICON_COLUMN_LOGICAL_PX)
                + px(ITEM_GAP_LOGICAL_PX)
                + measure(&cwd_leaf_or_path(&choice.path), px(ITEM_FONT_LOGICAL_PX))
                + px(ITEM_GAP_LOGICAL_PX)
                + note
        })
        .fold(0.0, f32::max);
    // `Browse…` is measured with the rest rather than assumed to fit: it is a
    // translated string one day, and a row that overflowed the box it was not
    // counted into would be clipped by the very menu it belongs to.
    let content = content.max(
        px(ITEM_ICON_COLUMN_LOGICAL_PX)
            + px(ITEM_GAP_LOGICAL_PX)
            + measure(BROWSE_TEXT, px(ITEM_FONT_LOGICAL_PX)),
    );
    let width = (chrome + content)
        .clamp(
            px(ROOT_MENU_MIN_WIDTH_LOGICAL_PX),
            px(ROOT_MENU_MIN_WIDTH_LOGICAL_PX + RECENT_ITEM_MAX_WIDTH_LOGICAL_PX),
        )
        .round();
    let height = (2.0 * (border + padding)
        + section_block
        + item_height * choices.len() as f32
        + separator_block
        + item_height)
        .round();

    let (surface_width, _) = surface;
    let edge = px(MENU_EDGE_MARGIN_LOGICAL_PX);
    let left = anchor[0]
        .min(surface_width - width - edge)
        .max(edge)
        .round();
    let top = (anchor[3] + px(MENU_OFFSET_LOGICAL_PX)).round();
    let frame = [left, top, left + width, top + height];

    let content_left = frame[0] + border + padding;
    let content_right = frame[2] - border - padding;
    let mut cursor = frame[1] + border + padding;
    let label = [content_left, cursor, content_right, cursor + section_block];
    cursor += section_block;
    let mut items = Vec::with_capacity(choices.len());
    for _ in choices {
        items.push([content_left, cursor, content_right, cursor + item_height]);
        cursor += item_height;
    }
    let browse_separator = [
        content_left,
        cursor + separator_margin,
        content_right,
        cursor + separator_margin + separator_thickness,
    ];
    cursor += separator_block;
    let browse = [content_left, cursor, content_right, cursor + item_height];
    RootMenuLayout {
        scale,
        frame,
        label,
        items,
        browse_separator,
        browse,
    }
}

/// What a point is over, with the same three answers [`hit`] gives and for the
/// same reasons.
#[must_use]
pub fn root_menu_hit(layout: &RootMenuLayout, x: f64, y: f64) -> Option<Option<RootMenuRow>> {
    let (x, y) = (x as f32, y as f32);
    for (index, item) in layout.items.iter().enumerate() {
        if contains(*item, x, y) {
            return Some(Some(RootMenuRow::Choice(index)));
        }
    }
    if contains(layout.browse, x, y) {
        return Some(Some(RootMenuRow::Browse));
    }
    contains(layout.frame, x, y).then_some(None)
}

/// The last segment of a path, or the whole of it when there is no segment to
/// take — a drive root is `C:\` and its "name" is itself.
fn cwd_leaf_or_path(path: &str) -> String {
    crate::cwd_leaf(Path::new(path)).unwrap_or_else(|| path.to_owned())
}

/// The root menu as one overlay layer.
#[must_use]
pub fn root_menu_build(
    layout: &RootMenuLayout,
    choices: &[RootChoice],
    current: &str,
    hover: Option<RootMenuRow>,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> Vec<OverlayLayer> {
    let palette = chrome_palette();
    let scale = layout.scale;
    let px = |value: f32| value * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
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
    labels.push(section_label(
        ROOT_SECTION_LABEL,
        layout.label,
        scale,
        palette,
    ));

    for (index, (item, choice)) in layout.items.iter().zip(choices).enumerate() {
        let note = choice.note.text().to_owned();
        let width = measure(&note, px(HINT_FONT_LOGICAL_PX));
        push_row(
            &Row {
                rect: *item,
                // The folder a column is *already* rooted at is drawn open, and
                // that is the tick's whole job done by the mark it already has:
                // one glyph saying "you are here" beats a second column of
                // empty space on every other row.
                mark: Some(if choice.path == current {
                    ChromeMark::FolderOpen
                } else {
                    ChromeMark::Folder
                }),
                name: &cwd_leaf_or_path(&choice.path),
                hint: Some((note, width)),
                hint_ink: None,
                hovered: hover == Some(RootMenuRow::Choice(index)),
                available: true,
            },
            scale,
            palette,
            &mut quads,
            &mut labels,
            &mut sprites,
        );
    }

    // ── the escape hatch (E55) ──────────────────────────────────────────────
    quads.push(OverlayQuad {
        rect: layout.browse_separator,
        color: palette.menu_border,
        alpha: separator_alpha(palette.menu_border),
    });
    push_row(
        &Row {
            rect: layout.browse,
            // The *open* folder, which is the mock-up's own choice (line 5150) and
            // the only row here that earns it by meaning rather than by state: the
            // rows above wear an open folder to say "you are already here", and
            // this one wears it to say "go and look".
            mark: Some(ChromeMark::FolderOpen),
            name: BROWSE_TEXT,
            // No note. Every row above answers "why is this offered?"; this one is
            // offered because nothing else was, and a hint saying so would be the
            // menu apologising for itself.
            hint: None,
            hint_ink: None,
            hovered: hover == Some(RootMenuRow::Browse),
            // The system always has a folder picker; there is no machine on which
            // this row is a promise the window cannot keep.
            available: true,
        },
        scale,
        palette,
        &mut quads,
        &mut labels,
        &mut sprites,
    );
    vec![OverlayLayer {
        quads,
        labels,
        sprites,
        ..Default::default()
    }]
}

/// `.rm-label` — the heading over the list (mock-up 5138).
const ROOT_SECTION_LABEL: &str = "OPEN FOLDER";

/// The escape hatch's own words (mock-up 5151).
///
/// The ellipsis is load-bearing and is the real character rather than three
/// dots: it is the platform convention for "this opens something that will ask
/// you again", which is exactly the promise that separates this row from the
/// rows above it, each of which commits the moment it is pressed.
const BROWSE_TEXT: &str = "Browse…";

/// `.root-menu { min-width: 190px }` (mock-up 629).
///
/// Its own constant rather than the profile picker's 180: the mock-up gives the
/// two menus different floors, and this one is wider because every row in it is
/// a directory name followed by a note, where the picker's rows are short shell
/// names. Sharing the number made the root menu 10px narrower than drawn.
const ROOT_MENU_MIN_WIDTH_LOGICAL_PX: f32 = 190.0;

// ── the file row's context menu (K143/K145) ─────────────────────────────────

/// One row of the menu a file row raises under the pointer.
///
/// Exactly three verbs, and the list is closed rather than a `Vec`: `DESIGN.md`
/// §7.1.3 names them — "Open preview / Copy path / Insert path into terminal" —
/// and the mock-up's fourth (`Save as…`, mock-up 8088) is conditional on a
/// *terminal artefact* and is raised from the inline-image path, not from a row
/// of the tree. A menu whose length cannot vary is also a menu whose keyboard
/// walk cannot go looking for a row that is not there.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileMenuRow {
    /// Hand the row to whatever opens it — the same verb its double click has,
    /// which is why the caller supplies the wording (see [`file_menu_layout`]).
    Open,
    CopyPath,
    InsertPath,
}

impl FileMenuRow {
    /// The rows in the order they are drawn, which is the order a keyboard
    /// walks them.
    pub const ALL: [Self; 3] = [Self::Open, Self::CopyPath, Self::InsertPath];

    /// The row `steps` away, stopping at the ends rather than wrapping round.
    ///
    /// Clamped, not cyclic, because the tree this menu was raised from clamps
    /// too (D45): one window should not hold two different ideas of what the
    /// bottom of a list does. From nowhere, a step in either direction lands on
    /// the end it came from — pressing Up on a fresh menu offers the last row,
    /// which is the convention every platform menu keeps.
    #[must_use]
    pub fn step(current: Option<Self>, forwards: bool) -> Self {
        let Some(current) = current else {
            return if forwards {
                Self::ALL[0]
            } else {
                Self::ALL[Self::ALL.len() - 1]
            };
        };
        let index = Self::ALL
            .iter()
            .position(|row| *row == current)
            .expect("every row is in ALL");
        let index = if forwards {
            (index + 1).min(Self::ALL.len() - 1)
        } else {
            index.saturating_sub(1)
        };
        Self::ALL[index]
    }

    fn mark(self) -> ChromeMark {
        match self {
            Self::Open => ChromeMark::File,
            Self::CopyPath => ChromeMark::Copy,
            Self::InsertPath => ChromeMark::Paste,
        }
    }
}

/// Every rectangle the file menu draws and hit-tests.
#[derive(Clone, Debug, PartialEq)]
pub struct FileMenuLayout {
    scale: f32,
    frame: [f32; 4],
    items: [[f32; 4]; 3],
    /// The rule under `Open` — mock-up 8089, which separates *what this row is*
    /// from *what its path is*.
    separator: [f32; 4],
}

/// `Insert path into terminal` — the widest of the three, and the reason the
/// menu is measured rather than given a fixed width.
pub const INSERT_PATH_TEXT: &str = "Insert path into terminal";
pub const COPY_PATH_TEXT: &str = "Copy path";

/// The menu hung under the point a row was right-clicked at.
///
/// **A point, not a widget.** Every other popup in this window hangs off a
/// button and must therefore re-find that button after a re-render (E59/E60).
/// This one is raised at the pointer, so the anchor is a coordinate that no
/// re-layout can move or destroy — which is also why it does not need the live
/// re-measure the root menu pays for on every frame.
///
/// `open_text` is the caller's because only the caller knows where the row
/// leads: a picture goes to the preview seat and everything else goes to the
/// system's own handler, and the menu must not promise the one while doing the
/// other.
#[must_use]
pub fn file_menu_layout(
    point: [f32; 2],
    surface: (f32, f32),
    scale: f32,
    open_text: &str,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> FileMenuLayout {
    let px = |value: f32| value * scale;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let padding = px(MENU_PADDING_LOGICAL_PX);
    let item_height = px(ITEM_HEIGHT_LOGICAL_PX).round();
    let separator_thickness = (SEPARATOR_THICKNESS_LOGICAL_PX * scale).round().max(1.0);
    let separator_margin = px(SEPARATOR_MARGIN_Y_LOGICAL_PX).round();
    let separator_block = 2.0 * separator_margin + separator_thickness;

    let chrome = 2.0 * (border + padding) + 2.0 * px(ITEM_PADDING_X_LOGICAL_PX);
    let row_width = |text: &str, measure: &mut dyn FnMut(&str, f32) -> f32| {
        px(ITEM_ICON_COLUMN_LOGICAL_PX)
            + px(ITEM_GAP_LOGICAL_PX)
            + measure(text, px(ITEM_FONT_LOGICAL_PX))
    };
    let content = row_width(open_text, measure)
        .max(row_width(COPY_PATH_TEXT, measure))
        .max(row_width(INSERT_PATH_TEXT, measure));
    let width = (chrome + content)
        .max(px(FILE_MENU_MIN_WIDTH_LOGICAL_PX))
        .round();
    let height = (2.0 * (border + padding) + 3.0 * item_height + separator_block).round();

    // Both axes clamped, unlike the root menu's one. A menu hung under a button
    // can only ever run off the side, because the button it hangs from is on a
    // horizontal strip near the top; a menu raised at the pointer can be raised
    // at the bottom row of a tall column, where an unclamped drop would put
    // every one of its verbs under the window's own edge.
    let (surface_width, surface_height) = surface;
    let edge = px(MENU_EDGE_MARGIN_LOGICAL_PX);
    let left = point[0].min(surface_width - width - edge).max(edge).round();
    let top = point[1]
        .min(surface_height - height - edge)
        .max(edge)
        .round();
    let frame = [left, top, left + width, top + height];

    let content_left = frame[0] + border + padding;
    let content_right = frame[2] - border - padding;
    let mut cursor = frame[1] + border + padding;
    let open = [content_left, cursor, content_right, cursor + item_height];
    cursor += item_height;
    let separator = [
        content_left,
        cursor + separator_margin,
        content_right,
        cursor + separator_margin + separator_thickness,
    ];
    cursor += separator_block;
    let copy_path = [content_left, cursor, content_right, cursor + item_height];
    cursor += item_height;
    let insert_path = [content_left, cursor, content_right, cursor + item_height];
    FileMenuLayout {
        scale,
        frame,
        items: [open, copy_path, insert_path],
        separator,
    }
}

/// What a point is over, with the same three answers the other menus give.
#[must_use]
pub fn file_menu_hit(layout: &FileMenuLayout, x: f64, y: f64) -> Option<Option<FileMenuRow>> {
    let (x, y) = (x as f32, y as f32);
    for (row, rect) in FileMenuRow::ALL.iter().zip(layout.items) {
        if contains(rect, x, y) {
            return Some(Some(*row));
        }
    }
    contains(layout.frame, x, y).then_some(None)
}

/// The file menu as one overlay layer.
#[must_use]
pub fn file_menu_build(
    layout: &FileMenuLayout,
    open_text: &str,
    hover: Option<FileMenuRow>,
) -> Vec<OverlayLayer> {
    let palette = chrome_palette();
    let scale = layout.scale;
    let px = |value: f32| value * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
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

    for (row, rect) in FileMenuRow::ALL.iter().zip(layout.items) {
        push_row(
            &Row {
                rect,
                mark: Some(row.mark()),
                name: match row {
                    FileMenuRow::Open => open_text,
                    FileMenuRow::CopyPath => COPY_PATH_TEXT,
                    FileMenuRow::InsertPath => INSERT_PATH_TEXT,
                },
                hint: None,
                hint_ink: None,
                hovered: hover == Some(*row),
                // All three verbs act on a path this process enumerated. There
                // is no machine on which one of them is a promise that cannot be
                // kept — the refusals these verbs *can* meet (a program the tree
                // will not run, a shell that has gone) happen after the press
                // and are spoken then, which is the same answer the double
                // click gives.
                available: true,
            },
            scale,
            palette,
            &mut quads,
            &mut labels,
            &mut sprites,
        );
        if *row == FileMenuRow::Open {
            quads.push(OverlayQuad {
                rect: layout.separator,
                color: palette.menu_border,
                alpha: separator_alpha(palette.menu_border),
            });
        }
    }
    vec![OverlayLayer {
        quads,
        labels,
        sprites,
        ..Default::default()
    }]
}

// ── the git context menus (v2 ④) ────────────────────────────────────────────
//
// **One menu machine, and what it offers is decided by what was pressed.**
// There are six things in this product a right press can land on that have a
// repository verb attached — a commit, a local branch, a remote-tracking
// branch, a tag, a changed file, and the working tree's own row — and they live
// in two different surfaces (the Git panel's column and the graph document). A
// menu per surface would be two lists of the same verbs drifting apart; a menu
// per row type would be six. So there is one [`GitMenuTarget`], one list of
// [`GitMenuRow`]s, and one function that says which rows a target offers.
//
// **The boundary is the ruling, and it is a boundary of verbs.** Read and
// navigate freely; write only what one command undoes. Nothing here merges,
// rebases, resets, cherry-picks, reverts, pushes, pulls, fetches, or reaches for
// `-D` or `--force` — see [`crate::git::GIT_NEVER_WORDS`], which is that
// sentence written as a test over every command this window can build.
//
// **The rule under the rule**: every menu that has both is split by one
// separator into *what this does to the repository* above and *what this tells
// you about it* below. A reader who has learned that once has learned it in all
// six menus, and it is why the separator's position is computed from the list
// rather than hand-placed per target.

/// What a right press landed on.
///
/// Self-contained — every variant carries the words its menu needs — for
/// [`crate::git_panel::GitRow`]'s own reason: a target that indexed into a list
/// the runtime also has to hold is a target that can disagree with it about
/// which row the menu is about, and the gap between raising a menu and pressing
/// one of its rows is exactly where a repository re-read lands.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GitMenuTarget {
    /// A commit row — the graph's, or the panel's own COMMITS list.
    Commit {
        /// The whole forty characters: what a `git branch <name> <at>` is given,
        /// and what goes on the clipboard.
        hash: String,
        /// git's abbreviation — what the card says out loud.
        short: String,
        subject: String,
        /// Whether the surface this row is on has D6's compare mode at all.
        ///
        /// **The graph has it and the panel does not**, and that is a ruling
        /// rather than a gap: compare mode is two rows lit, a block between them
        /// and a file list under it, and a 240-pixel column has none of that
        /// furniture. Offering the verb there would be offering a mode the
        /// surface cannot draw.
        can_compare: bool,
        /// Whether some *other* row is open to be the near end of a comparison
        /// (D6). Without one there is nothing for `Compare with selected` to
        /// compare against, so the row is not offered rather than offered and
        /// silent.
        compare_ready: bool,
    },
    /// A local branch — a panel BRANCHES row, or a filled pill in the graph.
    LocalBranch {
        name: String,
        /// Whether `HEAD` is on it. It decides two rows' availability and
        /// nothing else: you cannot check out where you already are, and git
        /// will not delete the branch you are standing on.
        current: bool,
    },
    /// A remote-tracking branch — a REMOTES row, or a hollow pill.
    ///
    /// `name` is git's own spelling with the remote on the front
    /// (`origin/main`), because that is what `--track` is handed.
    Remote { name: String },
    /// A tag pill in the graph.
    Tag { name: String },
    /// A changed file, in whichever of the three groups its row stands under.
    Change {
        path: String,
        group: crate::git::GitGroup,
        /// Whether git has ever seen this file — the difference between a
        /// discard that restores and one that deletes.
        untracked: bool,
        /// Where a rename came from, when this row is one — what the diff
        /// `Open diff` asks for needs in order to *be* a rename (see
        /// [`crate::git::GitQuestion::Diff::renamed_from`]).
        renamed_from: Option<String>,
    },
    /// The graph's **Uncommitted Changes** row (V5).
    ///
    /// The one target whose menu can be empty, and the emptiness is the ruling:
    /// with no commit open there is nothing to compare the working tree against,
    /// and every other verb on this list is about a *name* the working tree does
    /// not have. A menu with one greyed row in it would be a menu that opened in
    /// order to say nothing, so the press does nothing instead.
    Uncommitted { compare_ready: bool },
}

/// One row of a git context menu.
///
/// A single flat list across all six targets rather than an enum per target,
/// because the runtime's dispatch is one `match` and a row that means the same
/// thing in two menus should be the same value in both: `Copy name` off a branch
/// and off a tag put a name on the clipboard and raise the same card, and two
/// variants for that would be two code paths to keep in step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitMenuRow {
    /// `git checkout` — a branch by name, a commit or a tag detached.
    Checkout,
    /// The prompt that ends in `git branch <name> <hash>`.
    CreateBranchHere,
    /// The prompt that ends in `git tag <name> <hash>` — lightweight.
    CreateTagHere,
    /// The prompt that ends in `git branch -m <old> <new>`.
    RenameBranch,
    /// `git branch -d` — behind the gate, and never `-D`.
    DeleteBranch,
    /// `git tag -d` — behind the gate.
    DeleteTag,
    /// `git checkout -b <local> --track <remote>` (M10), or a plain checkout
    /// when the local already exists.
    CheckoutTracking,
    /// `git add`.
    Stage,
    /// `git restore --staged`.
    Unstage,
    /// `git restore --worktree`, or `git clean -f` — behind the gate.
    Discard,
    /// Put this file's diff on the preview seat, exactly as pressing the row
    /// does.
    OpenDiff,
    /// Hand the file to Explorer with it selected.
    RevealInExplorer,
    CopyPath,
    CopyHash,
    CopySubject,
    CopyName,
    /// Enter D6's compare mode with this row as the far end.
    CompareWithSelected,
    /// D6 with `b: None` — this commit against what is on disk.
    CompareWithWorkingTree,
}

pub const GIT_MENU_CHECKOUT_TEXT: &str = "Checkout";
pub const GIT_MENU_CREATE_BRANCH_TEXT: &str = "Create branch here…";
pub const GIT_MENU_CREATE_TAG_TEXT: &str = "Add tag here…";
pub const GIT_MENU_RENAME_BRANCH_TEXT: &str = "Rename…";
pub const GIT_MENU_DELETE_BRANCH_TEXT: &str = "Delete";
pub const GIT_MENU_DELETE_TAG_TEXT: &str = "Delete tag";
pub const GIT_MENU_CHECKOUT_TRACKING_TEXT: &str = "Checkout as local branch";
pub const GIT_MENU_STAGE_TEXT: &str = "Stage";
pub const GIT_MENU_UNSTAGE_TEXT: &str = "Unstage";
pub const GIT_MENU_DISCARD_TEXT: &str = "Discard";
pub const GIT_MENU_OPEN_DIFF_TEXT: &str = "Open diff";
pub const GIT_MENU_REVEAL_TEXT: &str = "Reveal in Explorer";
pub const GIT_MENU_COPY_PATH_TEXT: &str = "Copy path";
pub const GIT_MENU_COPY_HASH_TEXT: &str = "Copy hash";
pub const GIT_MENU_COPY_SUBJECT_TEXT: &str = "Copy subject";
pub const GIT_MENU_COPY_NAME_TEXT: &str = "Copy name";
pub const GIT_MENU_COMPARE_SELECTED_TEXT: &str = "Compare with selected";
pub const GIT_MENU_COMPARE_WORKING_TEXT: &str = "Compare with working tree";

impl GitMenuRow {
    /// What the row says.
    #[must_use]
    pub fn text(self) -> &'static str {
        match self {
            Self::Checkout => GIT_MENU_CHECKOUT_TEXT,
            Self::CreateBranchHere => GIT_MENU_CREATE_BRANCH_TEXT,
            Self::CreateTagHere => GIT_MENU_CREATE_TAG_TEXT,
            Self::RenameBranch => GIT_MENU_RENAME_BRANCH_TEXT,
            Self::DeleteBranch => GIT_MENU_DELETE_BRANCH_TEXT,
            Self::DeleteTag => GIT_MENU_DELETE_TAG_TEXT,
            Self::CheckoutTracking => GIT_MENU_CHECKOUT_TRACKING_TEXT,
            Self::Stage => GIT_MENU_STAGE_TEXT,
            Self::Unstage => GIT_MENU_UNSTAGE_TEXT,
            Self::Discard => GIT_MENU_DISCARD_TEXT,
            Self::OpenDiff => GIT_MENU_OPEN_DIFF_TEXT,
            Self::RevealInExplorer => GIT_MENU_REVEAL_TEXT,
            Self::CopyPath => GIT_MENU_COPY_PATH_TEXT,
            Self::CopyHash => GIT_MENU_COPY_HASH_TEXT,
            Self::CopySubject => GIT_MENU_COPY_SUBJECT_TEXT,
            Self::CopyName => GIT_MENU_COPY_NAME_TEXT,
            Self::CompareWithSelected => GIT_MENU_COMPARE_SELECTED_TEXT,
            Self::CompareWithWorkingTree => GIT_MENU_COMPARE_WORKING_TEXT,
        }
    }

    /// Whether this row *writes* to the repository, which is what the separator
    /// divides on.
    ///
    /// A property of the row rather than a position in a list, so the rule —
    /// verbs above the rule, readings below it — is stated once and every menu
    /// obeys it by construction. `Open diff` and the two compares are readings:
    /// they put a document on a seat and leave the repository exactly as they
    /// found it.
    #[must_use]
    pub fn writes(self) -> bool {
        matches!(
            self,
            Self::Checkout
                | Self::CreateBranchHere
                | Self::CreateTagHere
                | Self::RenameBranch
                | Self::DeleteBranch
                | Self::DeleteTag
                | Self::CheckoutTracking
                | Self::Stage
                | Self::Unstage
                | Self::Discard
        )
    }

    /// Which prompt this row opens, when it opens one.
    ///
    /// The three rows whose name ends in `…` and no others, which is the
    /// platform convention the file menu's `Browse…` already keeps: the ellipsis
    /// is a promise that pressing this asks you again before anything happens.
    #[must_use]
    pub fn prompt(self) -> Option<GitPromptKind> {
        match self {
            Self::CreateBranchHere => Some(GitPromptKind::CreateBranch),
            Self::CreateTagHere => Some(GitPromptKind::CreateTag),
            Self::RenameBranch => Some(GitPromptKind::RenameBranch),
            _ => None,
        }
    }

    /// The mark in the row's 14-pixel column.
    ///
    /// **`Rename…` has none, and that is a choice rather than an oversight.**
    /// The house's mark set is cut from geometry and has no pencil in it; the
    /// nearest thing to one would be a mark that means something else, and a
    /// wrong picture is read faster than a missing one. The row's own name, with
    /// its ellipsis, already says what it does.
    #[must_use]
    fn mark(self) -> Option<ChromeMark> {
        match self {
            Self::Checkout | Self::CheckoutTracking => Some(ChromeMark::GitBranch),
            Self::CreateBranchHere | Self::Stage => Some(ChromeMark::Plus),
            Self::CreateTagHere => Some(ChromeMark::Tag),
            Self::RenameBranch => None,
            Self::DeleteBranch | Self::DeleteTag | Self::Discard => Some(ChromeMark::PaneClose),
            Self::Unstage => Some(ChromeMark::Minus),
            Self::OpenDiff => Some(ChromeMark::File),
            Self::RevealInExplorer => Some(ChromeMark::FolderOpen),
            Self::CopyPath | Self::CopyHash | Self::CopySubject | Self::CopyName => {
                Some(ChromeMark::Copy)
            }
            // A comparison is two things stood beside each other, which is what
            // this mark draws.
            Self::CompareWithSelected | Self::CompareWithWorkingTree => Some(ChromeMark::Split),
        }
    }
}

/// Which of the three named prompts is open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitPromptKind {
    CreateBranch,
    CreateTag,
    RenameBranch,
}

impl GitPromptKind {
    /// The line over the field — **what is being named, and where.**
    ///
    /// It is here rather than at the call site because it is part of the menu's
    /// copy, and copy that lived at the call site would be copy that could differ
    /// between the graph and the panel raising the same prompt.
    #[must_use]
    pub fn caption(self, subject: &str) -> String {
        match self {
            Self::CreateBranch => format!("New branch at {subject}"),
            Self::CreateTag => format!("New tag at {subject}"),
            Self::RenameBranch => format!("Rename {subject}"),
        }
    }

    /// What the empty field says it wants.
    #[must_use]
    pub fn placeholder(self) -> &'static str {
        match self {
            Self::CreateBranch => "Branch name",
            Self::CreateTag => "Tag name",
            Self::RenameBranch => "New name",
        }
    }
}

/// The rows one target offers, and where the rule falls.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitMenu {
    pub rows: Vec<GitMenuRow>,
    /// The index the separator is drawn **under**, when the menu has both kinds
    /// of row. Derived from [`GitMenuRow::writes`] rather than written down per
    /// target, so the rule cannot be broken by adding a row in the wrong place.
    pub separator_after: Option<usize>,
}

/// What this target's menu holds.
///
/// **Empty means "do not open"**, and exactly one target can answer that way —
/// see [`GitMenuTarget::Uncommitted`].
#[must_use]
pub fn git_menu(target: &GitMenuTarget) -> GitMenu {
    let rows: Vec<GitMenuRow> = match target {
        GitMenuTarget::Commit {
            can_compare,
            compare_ready,
            ..
        } => {
            let mut rows = vec![
                GitMenuRow::Checkout,
                GitMenuRow::CreateBranchHere,
                GitMenuRow::CreateTagHere,
                GitMenuRow::CopyHash,
                GitMenuRow::CopySubject,
            ];
            if *can_compare && *compare_ready {
                rows.push(GitMenuRow::CompareWithSelected);
            }
            if *can_compare {
                rows.push(GitMenuRow::CompareWithWorkingTree);
            }
            rows
        }
        GitMenuTarget::LocalBranch { .. } => vec![
            GitMenuRow::Checkout,
            GitMenuRow::RenameBranch,
            GitMenuRow::DeleteBranch,
            GitMenuRow::CopyName,
        ],
        // **Two rows and no more** (M10). No fetch, no pull, no
        // delete-the-remote-branch: each of those talks to another machine or
        // changes one, and the ruling that opened this slice puts all three
        // outside it.
        GitMenuTarget::Remote { .. } => vec![GitMenuRow::CheckoutTracking, GitMenuRow::CopyName],
        GitMenuTarget::Tag { .. } => vec![
            GitMenuRow::Checkout,
            GitMenuRow::DeleteTag,
            GitMenuRow::CopyName,
        ],
        GitMenuTarget::Change { group, .. } => vec![
            // **Whichever applies**, decided by the group the row stands under
            // and not by the file's own state: a file that is in both STAGED and
            // CHANGES has a row in each, and each row means the thing its
            // heading says.
            if *group == crate::git::GitGroup::Staged {
                GitMenuRow::Unstage
            } else {
                GitMenuRow::Stage
            },
            GitMenuRow::Discard,
            GitMenuRow::OpenDiff,
            GitMenuRow::RevealInExplorer,
            GitMenuRow::CopyPath,
        ],
        GitMenuTarget::Uncommitted { compare_ready } => {
            if *compare_ready {
                vec![GitMenuRow::CompareWithSelected]
            } else {
                Vec::new()
            }
        }
    };
    // The rule falls where the writes stop. A menu that is all writes or all
    // readings gets none, which is what makes the remote's two-row menu and the
    // working tree's one-row menu look like the small things they are.
    let separator_after = rows
        .iter()
        .position(|row| !row.writes())
        .filter(|at| *at > 0 && *at < rows.len())
        .map(|at| at - 1);
    GitMenu {
        rows,
        separator_after,
    }
}

/// Whether a row can do what it says, on this target.
///
/// The two that cannot are both about the branch you are standing on, and both
/// are drawn rather than hidden: a menu whose rows move depending on where
/// `HEAD` is would be a menu you cannot learn the shape of. `Checkout` is
/// pointless there (you are already on it) and `Delete` is impossible (git
/// refuses to delete a checked-out branch), so both are shown greyed — the same
/// answer the profile picker gives a shell that is not installed.
#[must_use]
pub fn git_menu_row_available(row: GitMenuRow, target: &GitMenuTarget) -> bool {
    match target {
        GitMenuTarget::LocalBranch { current: true, .. } => {
            !matches!(row, GitMenuRow::Checkout | GitMenuRow::DeleteBranch)
        }
        _ => true,
    }
}

/// The row a keyboard step lands on, **skipping the ones that answer nothing**.
///
/// Clamped rather than cyclic, which is [`FileMenuRow::step`]'s ruling and the
/// tree's: one window should not hold two ideas of what the bottom of a list
/// does. From nowhere, a step forwards offers the first available row and a step
/// backwards the last. `None` when nothing in the menu is available at all,
/// which no target this slice builds can produce but is the honest answer rather
/// than a panic.
#[must_use]
pub fn git_menu_step(
    rows: &[GitMenuRow],
    target: &GitMenuTarget,
    current: Option<GitMenuRow>,
    forwards: bool,
) -> Option<GitMenuRow> {
    let walkable: Vec<GitMenuRow> = rows
        .iter()
        .copied()
        .filter(|row| git_menu_row_available(*row, target))
        .collect();
    if walkable.is_empty() {
        return None;
    }
    let Some(at) = current.and_then(|row| walkable.iter().position(|found| *found == row)) else {
        return Some(if forwards {
            walkable[0]
        } else {
            walkable[walkable.len() - 1]
        });
    };
    let next = if forwards {
        (at + 1).min(walkable.len() - 1)
    } else {
        at.saturating_sub(1)
    };
    Some(walkable[next])
}

/// The prompt as it stands this frame.
#[derive(Clone, Copy, Debug)]
pub struct GitPromptLook<'a> {
    pub kind: GitPromptKind,
    /// [`GitPromptKind::caption`]'s answer, built by the caller because the
    /// subject is the caller's.
    pub caption: &'a str,
    /// What is in the field, composition included — the whole line as drawn.
    pub text: &'a str,
    /// The part before the caret, which is what the caret's own x is measured
    /// from.
    pub before_caret: &'a str,
    /// Drawn *at* the caret and pushing it along (T4's rule, kept here so a
    /// composition in a prompt behaves as one in the search field does).
    pub preedit: &'a str,
    /// The red line under the field, when the name as typed is one git would
    /// refuse. **Not a card** (ticket ruling): a name being wrong is a fact about
    /// the field you are typing into, and a toast at the corner of the window
    /// would be the answer arriving somewhere other than the question.
    pub fault: Option<crate::git::RefNameFault>,
}

/// Everything the menu needs to lay itself out and draw.
#[derive(Clone, Copy, Debug)]
pub struct GitMenuLook<'a> {
    pub target: &'a GitMenuTarget,
    pub hover: Option<GitMenuRow>,
    /// When this is set the menu **is** the prompt: the rows are gone and the
    /// field stands in their place.
    ///
    /// The ticket's own shape, and the reason it is one popup rather than a
    /// popup with a dialog on top of it: what you are naming is the thing you
    /// right-clicked, the menu is already anchored to it, and a second surface
    /// would put the question somewhere other than where it was asked.
    pub prompt: Option<GitPromptLook<'a>>,
}

/// One laid-out row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GitMenuItem {
    pub row: GitMenuRow,
    pub rect: [f32; 4],
    pub available: bool,
}

/// Where the prompt's three parts stand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GitPromptRects {
    pub caption: [f32; 4],
    pub field: [f32; 4],
    /// `None` when the name as typed is fine — the menu is one line shorter, and
    /// grows by that line the moment it is not.
    pub hint: Option<[f32; 4]>,
    /// Where the caret stands, measured from the text's own left edge.
    pub caret_x: f32,
}

/// Every rectangle a git context menu draws and hit-tests.
#[derive(Clone, Debug, PartialEq)]
pub struct GitMenuLayout {
    scale: f32,
    frame: [f32; 4],
    items: Vec<GitMenuItem>,
    separator: Option<[f32; 4]>,
    prompt: Option<GitPromptRects>,
}

/// `.root-menu`'s floor, which is this menu's too: every row here is a verb and
/// a noun, and the widest of them (`Compare with working tree`) is measured
/// anyway — the floor only stops the two-row menus from looking like tooltips.
const GIT_MENU_MIN_WIDTH_LOGICAL_PX: f32 = 190.0;
/// The prompt's own floor. Wider than the rows', because a field you type a name
/// into that is exactly as wide as the word `Rename…` is a field you cannot see
/// what you typed in.
const GIT_PROMPT_MIN_WIDTH_LOGICAL_PX: f32 = 230.0;
const GIT_PROMPT_FIELD_HEIGHT_LOGICAL_PX: f32 = 26.0;
const GIT_PROMPT_FIELD_RADIUS_LOGICAL_PX: f32 = 5.0;
const GIT_PROMPT_FIELD_PADDING_X_LOGICAL_PX: f32 = 8.0;
const GIT_PROMPT_FONT_LOGICAL_PX: f32 = 12.5;
const GIT_PROMPT_CAPTION_LINE_LOGICAL_PX: f32 = 14.0;
const GIT_PROMPT_HINT_LINE_LOGICAL_PX: f32 = 14.0;
const GIT_PROMPT_GAP_LOGICAL_PX: f32 = 5.0;
/// The window's own one-pixel bar, at the search field's width.
const GIT_PROMPT_CARET_LOGICAL_PX: f32 = 1.5;
const GIT_PROMPT_CARET_INSET_LOGICAL_PX: f32 = 5.0;

/// Lay the menu out under the point it was raised at.
///
/// **A point, not a widget** — [`file_menu_layout`]'s ruling, and doubly true
/// here: the row this menu is about can be scrolled away, paged past or replaced
/// by a repository re-read while the menu is up, and a menu that re-found its
/// anchor every frame would follow it off the screen. Both axes are clamped into
/// the window for the same reason that one clamps both: a row at the bottom of a
/// tall graph is exactly where a menu with eight rows in it would otherwise hang
/// off the edge.
#[must_use]
pub fn git_menu_layout(
    point: [f32; 2],
    surface: (f32, f32),
    scale: f32,
    look: &GitMenuLook<'_>,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> GitMenuLayout {
    let px = |value: f32| value * scale;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let padding = px(MENU_PADDING_LOGICAL_PX);
    let item_height = px(ITEM_HEIGHT_LOGICAL_PX).round();
    let separator_thickness = (SEPARATOR_THICKNESS_LOGICAL_PX * scale).round().max(1.0);
    let separator_margin = px(SEPARATOR_MARGIN_Y_LOGICAL_PX).round();
    let separator_block = 2.0 * separator_margin + separator_thickness;
    let chrome = 2.0 * (border + padding) + 2.0 * px(ITEM_PADDING_X_LOGICAL_PX);
    let caption_line = px(GIT_PROMPT_CAPTION_LINE_LOGICAL_PX).round();
    let field_height = px(GIT_PROMPT_FIELD_HEIGHT_LOGICAL_PX).round();
    let prompt_gap = px(GIT_PROMPT_GAP_LOGICAL_PX).round();
    let hint_line = px(GIT_PROMPT_HINT_LINE_LOGICAL_PX).round();

    let menu = git_menu(look.target);
    let (content, height) = if let Some(prompt) = &look.prompt {
        let widest = measure(prompt.caption, px(HINT_FONT_LOGICAL_PX))
            .max(measure(prompt.text, px(GIT_PROMPT_FONT_LOGICAL_PX)))
            .max(prompt.fault.map_or(0.0, |fault| {
                measure(fault.sentence(), px(HINT_FONT_LOGICAL_PX))
            }))
            .max(px(GIT_PROMPT_MIN_WIDTH_LOGICAL_PX) - chrome);
        let height = 2.0f32.mul_add(
            border + padding,
            caption_line
                + prompt_gap
                + field_height
                + prompt.fault.map_or(0.0, |_| prompt_gap + hint_line),
        );
        (widest, height.round())
    } else {
        let row_width = |row: GitMenuRow, measure: &mut dyn FnMut(&str, f32) -> f32| {
            px(ITEM_ICON_COLUMN_LOGICAL_PX)
                + px(ITEM_GAP_LOGICAL_PX)
                + measure(row.text(), px(ITEM_FONT_LOGICAL_PX))
        };
        let content = menu
            .rows
            .iter()
            .fold(px(GIT_MENU_MIN_WIDTH_LOGICAL_PX) - chrome, |wide, row| {
                wide.max(row_width(*row, measure))
            });
        #[allow(clippy::cast_precision_loss)]
        let rows_height = menu.rows.len() as f32 * item_height;
        let height = 2.0f32.mul_add(
            border + padding,
            rows_height + menu.separator_after.map_or(0.0, |_| separator_block),
        );
        (content, height.round())
    };
    let width = (chrome + content).round();

    let (surface_width, surface_height) = surface;
    let edge = px(MENU_EDGE_MARGIN_LOGICAL_PX);
    let left = point[0].min(surface_width - width - edge).max(edge).round();
    let top = point[1]
        .min(surface_height - height - edge)
        .max(edge)
        .round();
    let frame = [left, top, left + width, top + height];
    let content_left = frame[0] + border + padding;
    let content_right = frame[2] - border - padding;
    let mut cursor = frame[1] + border + padding;

    if let Some(prompt) = &look.prompt {
        let caption = [content_left, cursor, content_right, cursor + caption_line];
        cursor += caption_line + prompt_gap;
        let field = [content_left, cursor, content_right, cursor + field_height];
        cursor += field_height;
        let hint = prompt.fault.map(|_| {
            cursor += prompt_gap;
            [content_left, cursor, content_right, cursor + hint_line]
        });
        return GitMenuLayout {
            scale,
            frame,
            items: Vec::new(),
            separator: None,
            prompt: Some(GitPromptRects {
                caption,
                field,
                hint,
                // The caret stands after the text the reader has typed up to it
                // **and after whatever they are composing**, which is where
                // every field in this window puts it (T4).
                caret_x: measure(
                    &format!("{}{}", prompt.before_caret, prompt.preedit),
                    px(GIT_PROMPT_FONT_LOGICAL_PX),
                ),
            }),
        };
    }

    let mut items = Vec::with_capacity(menu.rows.len());
    let mut separator = None;
    for (at, row) in menu.rows.iter().enumerate() {
        items.push(GitMenuItem {
            row: *row,
            rect: [content_left, cursor, content_right, cursor + item_height],
            available: git_menu_row_available(*row, look.target),
        });
        cursor += item_height;
        if menu.separator_after == Some(at) {
            separator = Some([
                content_left,
                cursor + separator_margin,
                content_right,
                cursor + separator_margin + separator_thickness,
            ]);
            cursor += separator_block;
        }
    }
    GitMenuLayout {
        scale,
        frame,
        items,
        separator,
        prompt: None,
    }
}

/// What a point is over, with the same three answers every other menu gives.
///
/// A row that cannot do what it says is **not** offered, which is [`hit`]'s own
/// rule: the pointer falls through it onto the menu's body, so it neither lights
/// nor answers a press.
#[must_use]
pub fn git_menu_hit(layout: &GitMenuLayout, x: f64, y: f64) -> Option<Option<GitMenuRow>> {
    let (x, y) = (x as f32, y as f32);
    for item in &layout.items {
        if item.available && contains(item.rect, x, y) {
            return Some(Some(item.row));
        }
    }
    contains(layout.frame, x, y).then_some(None)
}

/// The menu as one overlay layer.
#[must_use]
pub fn git_menu_build(layout: &GitMenuLayout, look: &GitMenuLook<'_>) -> Vec<OverlayLayer> {
    let palette = chrome_palette();
    let scale = layout.scale;
    let px = |value: f32| value * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
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

    if let (Some(rects), Some(prompt)) = (layout.prompt, look.prompt) {
        labels.push(ChromeLabel {
            text: prompt.caption.to_owned(),
            rect: rects.caption,
            font_size_px: px(HINT_FONT_LOGICAL_PX),
            color: palette.menu_item_hint_text,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: Some(rects.caption),
        });
        // The field's own ground is the row hover's fill — the quietest raised
        // surface this menu has, and therefore the one that reads as "a box you
        // type into" without inventing a colour the palette does not hold.
        quads.extend(rounded_overlay_fill(
            rects.field,
            px(GIT_PROMPT_FIELD_RADIUS_LOGICAL_PX),
            palette.menu_item_hover,
            1.0,
        ));
        let text_left = rects.field[0] + px(GIT_PROMPT_FIELD_PADDING_X_LOGICAL_PX);
        let text_right = rects.field[2] - px(GIT_PROMPT_FIELD_PADDING_X_LOGICAL_PX);
        let typed = !prompt.text.is_empty();
        let text_rect = [
            text_left,
            rects.field[1],
            text_right.max(text_left),
            rects.field[3],
        ];
        labels.push(ChromeLabel {
            text: if typed {
                prompt.text.to_owned()
            } else {
                prompt.kind.placeholder().to_owned()
            },
            rect: text_rect,
            font_size_px: px(GIT_PROMPT_FONT_LOGICAL_PX),
            // The placeholder is the field saying what it is for; typed text is
            // the reader's. Two inks, for the search field's own reason — one
            // ink and an empty field would read as a name nobody can delete.
            color: if typed {
                palette.menu_item_text
            } else {
                palette.menu_item_hint_text
            },
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: Some(text_rect),
        });
        let caret = px(GIT_PROMPT_CARET_LOGICAL_PX).round().max(1.0);
        let inset = px(GIT_PROMPT_CARET_INSET_LOGICAL_PX).round();
        let caret_x = (text_left + rects.caret_x).min(text_rect[2] - caret);
        quads.push(OverlayQuad {
            rect: [
                caret_x,
                rects.field[1] + inset,
                caret_x + caret,
                rects.field[3] - inset,
            ],
            color: palette.accent,
            alpha: 1.0,
        });
        if let (Some(hint), Some(fault)) = (rects.hint, prompt.fault) {
            labels.push(ChromeLabel {
                text: fault.sentence().to_owned(),
                rect: hint,
                font_size_px: px(HINT_FONT_LOGICAL_PX),
                color: palette.status_err,
                align_right: false,
                align_center: false,
                letter_spacing_em: 0.0,
                weight: ChromeLabelWeight::Regular,
                tabular_numerals: false,
                clip: Some(hint),
            });
        }
        return vec![OverlayLayer {
            quads,
            labels,
            sprites,
            ..Default::default()
        }];
    }

    for item in &layout.items {
        push_row(
            &Row {
                rect: item.rect,
                mark: item.row.mark(),
                name: item.row.text(),
                hint: None,
                hint_ink: None,
                hovered: look.hover == Some(item.row) && item.available,
                available: item.available,
            },
            scale,
            palette,
            &mut quads,
            &mut labels,
            &mut sprites,
        );
    }
    if let Some(separator) = layout.separator {
        quads.push(OverlayQuad {
            rect: separator,
            color: palette.menu_border,
            alpha: separator_alpha(palette.menu_border),
        });
    }
    vec![OverlayLayer {
        quads,
        labels,
        sprites,
        ..Default::default()
    }]
}

// ── the terminal's own context menu (`#term-menu`, ticket #62) ──────────────
//
// The oldest menu in the mock-up and the last one to be built, which is why the
// two menus above it read like siblings of something that was not there: the
// file row's menu and the pane head's menu both cite "the terminal menu's own
// rules" for anchoring at a point, and until this slice there was no terminal
// menu to have any.
//
// **The row list is `docs/DESIGN.md` §7.1.6, literally** — 「Copy、Paste、
// Select all、──、Clear screen、Clear scrollback…、Restart shell…」 — with the
// one addition §7.1.5d's S3 landing note booked against this ticket: `Find…`,
// after `Select all` and above the rule, because it belongs with the three verbs
// that read the pane rather than with the two that destroy something in it.
//
// It is a **flat list with a fixed order**, unlike the git menus next door,
// whose rows depend on what was pressed. That difference is the subject: a git
// menu is raised on a row and asks what kind of row it is, while this one is
// raised on a pane and every pane offers the same seven verbs. Which of them can
// *answer* varies (see [`term_menu_row_available`]); which of them are *there*
// does not, and a menu whose shape moved under the hand would be a menu nobody
// could learn.

/// One row of the terminal's context menu.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TermMenuRow {
    /// The selection onto the clipboard — **through copy-on-select's own door**,
    /// so the menu and the drag put the same bytes there and leave the selection
    /// standing.
    Copy,
    /// The clipboard into the shell — the keyboard's paste, byte for byte:
    /// bracketed when the shell asked for bracketing, chunked onto the one
    /// synchronous writer, view back at the bottom.
    Paste,
    /// Everything this pane has ever printed, selected: frozen history, the
    /// staged rows that have scrolled out but are not finalized, and the live
    /// grid.
    SelectAll,
    /// §7.1.5d's capsule, on this pane — `Runtime::open_search`.
    ///
    /// The row S3 shipped without: the engine, the capsule, the rail's merge and
    /// the keys all landed, and the *discoverable* door did not, because this
    /// menu did not exist to hang it on. `Ctrl+F` was the only way in.
    Find,
    /// **ED2 and the cursor home, executed here** — nothing is written to the
    /// PTY, the transcript and staging are not touched, and the rows that leave
    /// the viewport scroll out into history the ordinary way, so they can still
    /// be scrolled back to and searched (§7.1.6).
    ClearScreen,
    /// **The whole of §3.1's ED3 deletion** — history, staging, blocks, indexes,
    /// caches, anchor degradation, tombstones — which is what makes it the one
    /// row on this menu behind a confirmation.
    ClearScrollback,
    /// The shell is killed and a new one takes its place in the same seat, on
    /// the same profile, in the last folder it reported
    /// (`docs/M2-restart-shell-contract.md` §1).
    RestartShell,
}

pub const TERM_MENU_COPY_TEXT: &str = "Copy";
pub const TERM_MENU_PASTE_TEXT: &str = "Paste";
pub const TERM_MENU_SELECT_ALL_TEXT: &str = "Select all";
pub const TERM_MENU_FIND_TEXT: &str = "Find…";
pub const TERM_MENU_CLEAR_SCREEN_TEXT: &str = "Clear screen";
pub const TERM_MENU_CLEAR_SCROLLBACK_TEXT: &str = "Clear scrollback…";
pub const TERM_MENU_RESTART_TEXT: &str = "Restart shell…";

/// The menu, in §7.1.6's order.
///
/// A `const` list rather than a function that builds one, because there is
/// nothing to decide: every terminal pane offers these seven verbs in this order,
/// and the only thing a pane's own state changes is which of them are greyed.
pub const TERM_MENU_ROWS: [TermMenuRow; 7] = [
    TermMenuRow::Copy,
    TermMenuRow::Paste,
    TermMenuRow::SelectAll,
    TermMenuRow::Find,
    TermMenuRow::ClearScreen,
    TermMenuRow::ClearScrollback,
    TermMenuRow::RestartShell,
];

/// Where the one rule goes: **after the four rows that read the pane and before
/// the three that change it.**
///
/// An index into [`TERM_MENU_ROWS`] rather than a property of a row, unlike
/// [`GitMenuRow::writes`], and the difference is which fact each menu is
/// dividing on. A git menu is built afresh per target, so its rule has to be
/// *derived* or it would land in the wrong place on the next target; this list
/// never changes, so the honest statement is the position itself. Deriving it
/// from a `destroys()` predicate would be a second list agreeing with this one.
pub const TERM_MENU_SEPARATOR_AFTER: usize = 3;

/// What the pane under the menu can answer for (ticket #62, item 4).
///
/// Three facts and no pane, because none of the three is a *pointer* to
/// anything: the menu is laid out and drawn from a snapshot taken when it was
/// raised, exactly as the git menu carries its target by value, so that a shell
/// printing under an open menu cannot move a row out from under the hand.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TermMenuSubject {
    /// Whether there is anything for `Copy` to put on the clipboard.
    pub has_selection: bool,
    /// Whether this seat is between shells.
    ///
    /// `Restart shell…` is greyed rather than hidden while one is under way, for
    /// [`git_menu_row_available`]'s reason: a menu whose rows move is a menu you
    /// cannot learn the shape of, and "the verb you just pressed is still
    /// happening" is a thing worth saying rather than a row worth removing.
    pub restart_in_flight: bool,
    /// Whether the capsule can open on this pane at all.
    ///
    /// **False exactly on the alternate screen** (§7.1.5d, D-5/R3): §3.2 keeps
    /// that screen's anchors in an isolated namespace, so there is nothing there
    /// for a search to address and `Runtime::open_search` declines outright. A
    /// `Find…` offered over `vim` would be a row that opens nothing, which is
    /// worse than a greyed one — the reader would be entitled to think the
    /// search had failed rather than that it does not go there.
    ///
    /// It is a field rather than a second reading of `has_selection`'s source
    /// because the two are asked of different things: one is about what is
    /// highlighted, the other about which screen is up.
    pub can_search: bool,
}

impl Default for TermMenuSubject {
    /// **An ordinary pane**: a shell on its primary screen, with nothing
    /// highlighted and nothing being replaced.
    ///
    /// Written out rather than derived, because two of these three facts are
    /// good news and `bool::default()` is `false`: a derived default would say
    /// "this pane cannot be searched", which is the alternate screen — the rare
    /// case — and every caller that reached for `..Default::default()` would
    /// silently be describing `vim`.
    fn default() -> Self {
        Self {
            has_selection: false,
            restart_in_flight: false,
            can_search: true,
        }
    }
}

/// Whether a row can do what it says, on this pane.
///
/// **`Paste` is not on this list, and that is a decision** (ticket #62, item 4):
/// the row would be greyed on an empty clipboard *if asking were cheap*, and on
/// this platform it is not. `bt_platform::clipboard_text` is the only wrapper
/// there is and it opens the clipboard, reads the whole payload and closes it —
/// a transaction that contends with every other application on the machine, run
/// on every menu raise, to grey one row. `IsClipboardFormatAvailable` is the
/// cheap question and nothing wraps it, so the honest answer is the ticket's own
/// fallback: the row is always offered, and a paste with nothing to paste is
/// already a no-op that says so to the log.
///
/// The other four are always available because they are always answerable: a
/// pane with no selection still has a screen to clear, a scrollback to delete
/// and a grid to select.
#[must_use]
pub fn term_menu_row_available(row: TermMenuRow, subject: TermMenuSubject) -> bool {
    match row {
        TermMenuRow::Copy => subject.has_selection,
        // Not "there is nothing to find" — an empty transcript is searchable and
        // answers `0/0`, which is a real answer. This is the one place a search
        // cannot be *addressed*.
        TermMenuRow::Find => subject.can_search,
        TermMenuRow::RestartShell => !subject.restart_in_flight,
        _ => true,
    }
}

/// The row a keyboard step lands on, **skipping the ones that answer nothing** —
/// [`git_menu_step`]'s rule and [`FileMenuRow::step`]'s clamp, on this list.
#[must_use]
pub fn term_menu_step(
    current: Option<TermMenuRow>,
    subject: TermMenuSubject,
    forwards: bool,
) -> Option<TermMenuRow> {
    let walkable: Vec<TermMenuRow> = TERM_MENU_ROWS
        .into_iter()
        .filter(|row| term_menu_row_available(*row, subject))
        .collect();
    if walkable.is_empty() {
        return None;
    }
    let Some(at) = current.and_then(|row| walkable.iter().position(|found| *found == row)) else {
        return Some(if forwards {
            walkable[0]
        } else {
            walkable[walkable.len() - 1]
        });
    };
    let next = if forwards {
        (at + 1).min(walkable.len() - 1)
    } else {
        at.saturating_sub(1)
    };
    Some(walkable[next])
}

impl TermMenuRow {
    /// What the row says.
    ///
    /// **Three ellipses and no more**, which is the platform convention the git
    /// menu's three prompts already keep: `Clear scrollback…` asks again before
    /// it deletes anything, `Restart shell…` is the mock-up's own honest
    /// renaming of what `Refresh` used to do, and `Find…` opens a field rather
    /// than doing something. `Clear screen` has none because it does its whole
    /// job the moment it is pressed and takes nothing away.
    #[must_use]
    pub fn text(self) -> &'static str {
        match self {
            Self::Copy => TERM_MENU_COPY_TEXT,
            Self::Paste => TERM_MENU_PASTE_TEXT,
            Self::SelectAll => TERM_MENU_SELECT_ALL_TEXT,
            Self::Find => TERM_MENU_FIND_TEXT,
            Self::ClearScreen => TERM_MENU_CLEAR_SCREEN_TEXT,
            Self::ClearScrollback => TERM_MENU_CLEAR_SCROLLBACK_TEXT,
            Self::RestartShell => TERM_MENU_RESTART_TEXT,
        }
    }

    /// The mark in the row's 14-pixel column.
    ///
    /// **`Find…` has none**, and it is the git menu's `Rename…` decision made a
    /// second time for the same reason: the house's mark set is cut from the
    /// mock-up's own sheet, that sheet has no magnifier in it, and the nearest
    /// thing to one would be a mark that means something else. A wrong picture is
    /// read faster than a missing one; the row's name says what it does.
    #[must_use]
    fn mark(self) -> Option<ChromeMark> {
        match self {
            Self::Copy => Some(ChromeMark::Copy),
            Self::Paste => Some(ChromeMark::Paste),
            Self::SelectAll => Some(ChromeMark::SelectAll),
            Self::Find => None,
            Self::ClearScreen => Some(ChromeMark::Broom),
            Self::ClearScrollback => Some(ChromeMark::Eraser),
            Self::RestartShell => Some(ChromeMark::Refresh),
        }
    }
}

/// Everything the terminal menu needs to lay itself out and draw.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TermMenuLook {
    pub subject: TermMenuSubject,
    pub hover: Option<TermMenuRow>,
}

/// Every rectangle the terminal's context menu draws and hit-tests.
#[derive(Clone, Debug, PartialEq)]
pub struct TermMenuLayout {
    scale: f32,
    frame: [f32; 4],
    items: Vec<TermMenuItem>,
    separator: [f32; 4],
}

/// One laid-out row of the terminal menu.
///
/// Its own type rather than [`GitMenuItem`] with a different row in it, because
/// the two lists hold different enums and a shared item would have to be generic
/// over them — a type parameter bought for three fields that are copied out at
/// the one call site each.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TermMenuItem {
    pub row: TermMenuRow,
    pub rect: [f32; 4],
    pub available: bool,
}

/// `.term-menu`'s own `min-width: 172px` (mock-up 659), which is narrower than
/// the two menus above it declare — this list's longest row is two words.
const TERM_MENU_MIN_WIDTH_LOGICAL_PX: f32 = 172.0;

/// Lay the menu out under the point it was raised at.
///
/// **A point, not a pane** — [`git_menu_layout`]'s ruling, and it matters here
/// for a reason of its own: the pane under this menu is a *running shell*, so its
/// rectangle can be re-solved by a split, a divider drag or a window resize while
/// the menu stands, and a menu that re-found its pane every frame would walk
/// across the window while the reader was reading it.
#[must_use]
pub fn term_menu_layout(
    point: [f32; 2],
    surface: (f32, f32),
    scale: f32,
    look: &TermMenuLook,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> TermMenuLayout {
    let px = |value: f32| value * scale;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let padding = px(MENU_PADDING_LOGICAL_PX);
    let item_height = px(ITEM_HEIGHT_LOGICAL_PX).round();
    let separator_thickness = (SEPARATOR_THICKNESS_LOGICAL_PX * scale).round().max(1.0);
    let separator_margin = px(SEPARATOR_MARGIN_Y_LOGICAL_PX).round();
    let separator_block = 2.0 * separator_margin + separator_thickness;
    let chrome = 2.0 * (border + padding) + 2.0 * px(ITEM_PADDING_X_LOGICAL_PX);

    let row_width = |row: TermMenuRow, measure: &mut dyn FnMut(&str, f32) -> f32| {
        px(ITEM_ICON_COLUMN_LOGICAL_PX)
            + px(ITEM_GAP_LOGICAL_PX)
            + measure(row.text(), px(ITEM_FONT_LOGICAL_PX))
    };
    let content = TERM_MENU_ROWS
        .iter()
        .fold(px(TERM_MENU_MIN_WIDTH_LOGICAL_PX) - chrome, |wide, row| {
            wide.max(row_width(*row, measure))
        });
    #[allow(clippy::cast_precision_loss)]
    let rows_height = TERM_MENU_ROWS.len() as f32 * item_height;
    let height = 2.0f32
        .mul_add(border + padding, rows_height + separator_block)
        .round();
    let width = (chrome + content).round();

    let (surface_width, surface_height) = surface;
    let edge = px(MENU_EDGE_MARGIN_LOGICAL_PX);
    let left = point[0].min(surface_width - width - edge).max(edge).round();
    let top = point[1]
        .min(surface_height - height - edge)
        .max(edge)
        .round();
    let frame = [left, top, left + width, top + height];
    let content_left = frame[0] + border + padding;
    let content_right = frame[2] - border - padding;
    let mut cursor = frame[1] + border + padding;

    let mut items = Vec::with_capacity(TERM_MENU_ROWS.len());
    let mut separator = [0.0_f32; 4];
    for (at, row) in TERM_MENU_ROWS.iter().enumerate() {
        items.push(TermMenuItem {
            row: *row,
            rect: [content_left, cursor, content_right, cursor + item_height],
            available: term_menu_row_available(*row, look.subject),
        });
        cursor += item_height;
        if at == TERM_MENU_SEPARATOR_AFTER {
            separator = [
                content_left,
                cursor + separator_margin,
                content_right,
                cursor + separator_margin + separator_thickness,
            ];
            cursor += separator_block;
        }
    }
    TermMenuLayout {
        scale,
        frame,
        items,
        separator,
    }
}

/// What a point is over, with the same three answers every other menu gives: a
/// row, the menu's own padding, or nothing at all.
///
/// A row that cannot do what it says is **not** offered — the pointer falls
/// through it onto the menu's body, so it neither lights nor answers a press.
#[must_use]
pub fn term_menu_hit(layout: &TermMenuLayout, x: f64, y: f64) -> Option<Option<TermMenuRow>> {
    let (x, y) = (x as f32, y as f32);
    for item in &layout.items {
        if item.available && contains(item.rect, x, y) {
            return Some(Some(item.row));
        }
    }
    contains(layout.frame, x, y).then_some(None)
}

/// The menu as one overlay layer.
#[must_use]
pub fn term_menu_build(layout: &TermMenuLayout, look: &TermMenuLook) -> Vec<OverlayLayer> {
    let palette = chrome_palette();
    let scale = layout.scale;
    let px = |value: f32| value * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
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
    for item in &layout.items {
        push_row(
            &Row {
                rect: item.rect,
                mark: item.row.mark(),
                name: item.row.text(),
                hint: None,
                hint_ink: None,
                hovered: look.hover == Some(item.row) && item.available,
                available: item.available,
            },
            scale,
            palette,
            &mut quads,
            &mut labels,
            &mut sprites,
        );
    }
    quads.push(OverlayQuad {
        rect: layout.separator,
        color: palette.menu_border,
        alpha: separator_alpha(palette.menu_border),
    });
    vec![OverlayLayer {
        quads,
        labels,
        sprites,
        ..Default::default()
    }]
}

// ── the `⌄` open policy, shared by every chevron in the house ───────────────
//
// **User ruling, 2026-08-16.** There are two `⌄` in this window — the tab
// strip's, beside the `+`, and now the pane head's — and until this ruling they
// opened by two different rules: the strip's on a click, the head's not at all
// (its slot held a `⊞` that split without asking). The ruling makes them one
// control with one grammar: **rest on it for 250ms, or click it, and the menu
// comes; take the pointer off both the button and the menu and it goes after a
// short grace.**
//
// The ruling's own argument for the hover half is discoverability: "用户此前从
// 没发现 pane 头右键有菜单". A verb reachable only by right click is a verb most
// people never learn exists, and the answer is not a fifth button — it is that
// the one glyph in this product that already *means* "there is a list behind
// me" should behave the way a list behind a glyph behaves everywhere else.
//
// And the ruling is equally firm about the boundary: `⌄` is the **only** hover-
// opening surface. Everything else keeps the house's division of labour — hover
// is for looking (a tip, a peek), click is for doing, right click is for
// options. A window where any button might open a panel under a resting hand is
// a window you cannot rest your hand in.
//
// The policy is a state machine and not a pair of `if`s at two call sites,
// because "the two chevrons agree" is exactly the property that a second copy
// cannot keep. Both doors go through [`ChevronGate`]; the constants are declared
// once; and the test that matters is the one that drives both gates through the
// same steps and gets the same answers.

/// How long a pointer has to rest on a `⌄` before the menu behind it opens.
///
/// 250ms is the ruling's own number and it is chosen against the two failures at
/// either end. Shorter, and a pointer merely *crossing* the button on its way to
/// the `+` beside it drops a menu in its path — the classic hover-menu bug,
/// which is why the mock-up's own menus were click-only to begin with. Longer,
/// and the gesture stops reading as "rest here" and starts reading as "wait
/// here", which is a different and worse instruction.
pub const CHEVRON_HOVER_OPEN: Duration = Duration::from_millis(250);

/// How long a menu a `⌄` opened stays up once the pointer has left **both** it
/// and the button.
///
/// The grace exists because the button and its menu are two rectangles with a
/// four-pixel gap between them ([`MENU_OFFSET_LOGICAL_PX`]), and a hand
/// travelling from one to the other crosses that gap. Without a grace the menu
/// would close in the gap it was drawn across, every time, and the ruling's
/// "面板和图标可视为一体" would be false in exactly the pixels where it matters.
///
/// 150ms rather than the 250 above, and deliberately asymmetric: opening is a
/// commitment the user is making and deserves deliberation, while closing is
/// them having already left, and a menu that lingers a quarter of a second after
/// the hand has gone reads as a menu that is stuck.
pub const CHEVRON_LEAVE_GRACE: Duration = Duration::from_millis(150);

/// Where the pointer stands, as far as one `⌄` and the menu it opens are
/// concerned.
///
/// Three answers and not two, because the middle one is the whole of what makes
/// the pair one object: a pointer *on the menu* is neither on the button nor
/// away from the control, and treating it as "away" is the bug the grace above
/// exists to paper over rather than to hide.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChevronPointer {
    /// On the `⌄` itself.
    Button,
    /// On the menu the `⌄` opened — its rows, its padding, its submenu.
    Surface,
    /// On neither.
    Away,
}

/// What a `⌄`'s clock says is owed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChevronAction {
    Open,
    Close,
}

/// One `⌄`'s two clocks: how long the pointer has rested on it, and how long it
/// has been gone from both surfaces.
///
/// Two `Option<Instant>` rather than one enum, because the two are genuinely
/// exclusive by construction — [`Self::observe`] never leaves both set — and
/// writing them as an enum would put the proof of that in a `match` arm instead
/// of in the one function that assigns them.
///
/// No `Duration` is stored and no clock is read here: an instant is handed in at
/// each observation, which is what lets the whole policy be tested without a
/// window and without sleeping.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChevronGate {
    resting_since: Option<Instant>,
    leaving_since: Option<Instant>,
}

impl ChevronGate {
    /// Tell the gate where the pointer is and whether the menu is up.
    ///
    /// Idempotent under repetition, which is the property the caller depends on:
    /// a pointer that moves two pixels inside the button reports `Button` again,
    /// and the rest it has already accumulated must not be thrown away. That is
    /// why each clock is started with `get_or_insert` rather than assigned.
    pub fn observe(&mut self, pointer: ChevronPointer, open: bool, now: Instant) {
        match (pointer, open) {
            // Resting on a shut chevron is the one state that earns an open.
            (ChevronPointer::Button, false) => {
                self.leaving_since = None;
                self.resting_since.get_or_insert(now);
            }
            // On the button of a menu that is already up, or anywhere on the
            // menu itself: nothing is owed in either direction, and both clocks
            // are cleared so that leaving again starts a fresh grace.
            (ChevronPointer::Button, true) | (ChevronPointer::Surface, _) => self.clear(),
            // Gone, with a menu up: the grace runs.
            (ChevronPointer::Away, true) => {
                self.resting_since = None;
                self.leaving_since.get_or_insert(now);
            }
            // Gone, with nothing up: there is no clock to run.
            (ChevronPointer::Away, false) => self.clear(),
        }
    }

    /// What the gate owes at `now`, if anything.
    ///
    /// Reading it does not spend it — the caller clears the gate by acting on
    /// the answer, through [`Self::clear`] — because "what is due" and "do it"
    /// are two questions and the second one can fail.
    #[must_use]
    pub fn due(&self, now: Instant) -> Option<ChevronAction> {
        if self
            .resting_since
            .is_some_and(|since| now.duration_since(since) >= CHEVRON_HOVER_OPEN)
        {
            return Some(ChevronAction::Open);
        }
        if self
            .leaving_since
            .is_some_and(|since| now.duration_since(since) >= CHEVRON_LEAVE_GRACE)
        {
            return Some(ChevronAction::Close);
        }
        None
    }

    /// The next instant this gate has something to do, for the loop's wake set.
    ///
    /// `None` while neither clock is running, which is the ordinary state: a
    /// window whose pointer is not on a chevron costs no wake-ups at all.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        match (self.resting_since, self.leaving_since) {
            (Some(since), _) => Some(since + CHEVRON_HOVER_OPEN),
            (None, Some(since)) => Some(since + CHEVRON_LEAVE_GRACE),
            (None, None) => None,
        }
    }

    /// Stop both clocks — what a caller does once it has acted on [`Self::due`],
    /// and what every other door onto these menus (a click, Esc, another popup
    /// opening) does on its way through.
    pub fn clear(&mut self) {
        self.resting_since = None;
        self.leaving_since = None;
    }
}

// ── the pane head's own menu (user rulings, 2026-08-15 and 2026-08-16) ──────
//
// The mouse had no way to reach a split. The chords have had three since the
// fleet shipped — Ctrl+Shift+D and Alt+Shift+-/= — but a hand on the mouse had
// to open a new tab, drag its body across the window and drop it on an edge:
// three gestures for one verb, and the only one of the three that is obvious is
// the one that makes a tab you did not want.
//
// The 2026-08-15 ruling answered that with two doors onto one machine: a `⊞` in
// the head that took the pane's longer side and asked nothing, and a right-click
// menu that named both axes outright. The 2026-08-16 ruling folds the first into
// the second. The `⊞` is now a `⌄` — the house's one glyph for "there is a list
// behind me" — and the list it opens is this one, which has grown from three
// flat verbs into the whole of what a hand can ask of a pane: which way, with
// what, from where, and where to.
//
// It is still `#file-menu`'s skin and `push_row`'s rows, for the reason the
// switcher's note gives — a menu that looked like its neighbours but was drawn
// by different code would be a second popup wearing the first one's clothes.
// Two things here are genuinely new to the house and are built rather than
// borrowed: the **picker**, which is a drawing you press rather than a row you
// read, and the **submenu**, which brings the safety triangle (queue item #53)
// with it.

// ── the picker (Snap Layouts' idiom, this house's geometry) ────────────────
//
// Windows' own Snap Layouts is the reference and the reason: a hand that knows
// where it wants the new pane should be able to *point at* that place, not read
// four sentences and pick the one whose adverb matches. The ruling says so in as
// many words — "方向选择优先给图不给字".
//
// Drawn from quads rather than struck as a mark, and that is the one decision
// here worth arguing. A mark is a raster keyed on a glyph and a box; this is
// five rectangles whose *relationship* is the whole meaning, one of which lights
// up under the pointer. As a mark it would be five marks (or one mark per
// state, which is twenty), each cached separately, and the gap between the pane
// and its zones — the thing that makes them read as "beside" rather than "part
// of" — would be a fact about a bitmap instead of a number in this file.

/// The little pane at the middle of the diagram: 48 × 34.
///
/// A landscape rectangle rather than a square, because it stands for a terminal
/// pane and terminal panes are wide. A square would make the up/down zones the
/// same shape as the left/right ones, which is precisely the distinction the
/// diagram exists to draw.
const PICKER_PANE_WIDTH_LOGICAL_PX: f32 = 48.0;
const PICKER_PANE_HEIGHT_LOGICAL_PX: f32 = 34.0;
/// The pane's own round — `.pane`'s 6px read down for a drawing a third its
/// size, on the same ladder every small rounded box in this window climbs.
const PICKER_PANE_RADIUS_LOGICAL_PX: f32 = 4.0;
/// How thick a drop zone's slab is.
const PICKER_ZONE_THICKNESS_LOGICAL_PX: f32 = 10.0;
/// The air between the pane and a slab, which is what makes the slab read as a
/// *place the new pane would go* rather than as a border on the old one.
const PICKER_ZONE_GAP_LOGICAL_PX: f32 = 3.0;
const PICKER_ZONE_RADIUS_LOGICAL_PX: f32 = 3.0;
/// The hairline the pane and the four slabs are outlined in — one logical pixel,
/// the same weight `#file-menu`'s own edge wears.
const PICKER_EDGE_LOGICAL_PX: f32 = 1.0;
/// `--accent` at 15% is the wash a zone takes under the pointer.
///
/// A wash and not a fill: the slab has to go on reading as an outline with
/// something behind it, because what it means is "the new pane lands here" and a
/// solid block means "there is already a pane here".
const PICKER_ZONE_WASH_ALPHA: f32 = 0.15;
/// The air above the diagram, inside the picker's own block.
const PICKER_PADDING_TOP_LOGICAL_PX: f32 = 6.0;
/// And below the caption.
const PICKER_PADDING_BOTTOM_LOGICAL_PX: f32 = 5.0;
/// The caption, in the section label's grammar (`.glabel`: 9.5-10.5px, tracked,
/// upper-cased, `--ink3`).
///
/// Written upper-case at the source for [`RECENT_SECTION_LABEL`]'s reason and
/// not for a different one: this pipeline has no `text-transform`, a chrome
/// label draws the string it is given, and a lower-case constant that nothing
/// upper-cases would be a lie about what appears on screen.
pub const PICKER_CAPTION_TEXT: &str = "SPLIT";

/// The whole height of the picker's block: the air, the diagram, the caption.
///
/// **Derived rather than declared.** The ruling asks for "one row, ~92px tall",
/// and 92 is what these terms sum to — but writing `92.0` here and laying the
/// parts out inside it would make the number the specification and the parts an
/// arrangement that happens to fit, which is the shape in which a later change
/// to the slab thickness silently leaves a gap at the bottom.
fn picker_block_logical_px() -> f32 {
    PICKER_PADDING_TOP_LOGICAL_PX
        + picker_diagram_height_logical_px()
        + SECTION_LABEL_PADDING_TOP_LOGICAL_PX
        + SECTION_LABEL_LINE_LOGICAL_PX
        + SECTION_LABEL_PADDING_BOTTOM_LOGICAL_PX
        + PICKER_PADDING_BOTTOM_LOGICAL_PX
}

/// The diagram's own box: the pane, plus a gap and a slab on each of four sides.
fn picker_diagram_width_logical_px() -> f32 {
    PICKER_PANE_WIDTH_LOGICAL_PX
        + 2.0 * (PICKER_ZONE_GAP_LOGICAL_PX + PICKER_ZONE_THICKNESS_LOGICAL_PX)
}

fn picker_diagram_height_logical_px() -> f32 {
    PICKER_PANE_HEIGHT_LOGICAL_PX
        + 2.0 * (PICKER_ZONE_GAP_LOGICAL_PX + PICKER_ZONE_THICKNESS_LOGICAL_PX)
}

/// The four sides a pane can be split toward.
///
/// **Named by where the arriving pane lands**, which is how every terminal names
/// a split: "right" puts a shell on the right, and the rule it draws is
/// vertical. Nobody says the second half out loud, and the picker does not have
/// to — a hand pointing at the slab on the right is not reading an adverb.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SplitZone {
    Right,
    Down,
    Left,
    Up,
}

impl SplitZone {
    /// The four in the order the keyboard's compass and the tests walk them.
    pub const ALL: [Self; 4] = [Self::Right, Self::Down, Self::Left, Self::Up];

    /// Which axis this zone cuts along — `Row` for side by side, `Col` for
    /// stacked.
    #[must_use]
    pub fn axis(self) -> Axis {
        match self {
            Self::Right | Self::Left => Axis::Row,
            Self::Down | Self::Up => Axis::Col,
        }
    }

    /// Whether the arriving pane goes **first** in the run this split makes.
    ///
    /// The whole of what `Left` and `Up` are: the same two axes the menu has
    /// always offered, with the new leaf inserted on the other side of the one it
    /// came out of. `bt-layout`'s `Edit::SplitSeat` has carried a `leading` flag
    /// since the tree existed and `Seats::split_terminal` has always taken it —
    /// no layout work was owed here, only a caller that stops passing `false`.
    #[must_use]
    pub fn leading(self) -> bool {
        matches!(self, Self::Left | Self::Up)
    }
}

// ── the rows ───────────────────────────────────────────────────────────────

/// The verbs a pane head's `⌄` offers.
///
/// Closed rather than a `Vec`, for [`FileMenuRow`]'s reason: a menu whose length
/// cannot vary is also a menu whose keyboard walk cannot go looking for a row
/// that is not there.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneMenuRow {
    /// The Snap-Layouts picker — a drawing, not a row of text, and the only
    /// entry here that carries its own four answers.
    Picker,
    /// The submenu heading: split against a profile you name.
    SplitWith,
    /// Split, with the new shell rooted in a folder the system chooser names.
    NewInFolder,
    /// Split, with the new shell on this pane's own profile *and* this pane's
    /// own directory.
    Duplicate,
    /// The pane leaves this tab and becomes a tab of its own.
    MoveToNewTab,
    /// The same verb the `×` in the head has.
    ClosePane,
}

impl PaneMenuRow {
    /// Every entry, top to bottom — the order they are laid out in and the order
    /// a keyboard walks them.
    pub const ALL: [Self; 6] = [
        Self::Picker,
        Self::SplitWith,
        Self::NewInFolder,
        Self::Duplicate,
        Self::MoveToNewTab,
        Self::ClosePane,
    ];

    /// The five that are rows of text with a mark, in order — [`Self::ALL`]
    /// without the picker.
    pub const TEXT_ROWS: [Self; 5] = [
        Self::SplitWith,
        Self::NewInFolder,
        Self::Duplicate,
        Self::MoveToNewTab,
        Self::ClosePane,
    ];

    /// The glyph in this row's icon column.
    fn mark(self) -> Option<ChromeMark> {
        match self {
            // The picker is the drawing; it has no column and no glyph.
            Self::Picker => None,
            // The `⊞` the pane head just gave up, in the one place it still
            // means what it always meant: "another one of these, beside this".
            Self::SplitWith => Some(ChromeMark::Split),
            Self::NewInFolder => Some(ChromeMark::Folder),
            Self::Duplicate => Some(ChromeMark::Copy),
            // `#i-float`'s own sentence is "opens outside this frame" — and a
            // pane leaving for a tab of its own is exactly that. It is the same
            // glyph the files head wears for undocking, which is the same idea
            // aimed at a different container.
            Self::MoveToNewTab => Some(ChromeMark::Float),
            Self::ClosePane => Some(ChromeMark::TabClose),
        }
    }

    fn text(self) -> &'static str {
        match self {
            Self::Picker => PICKER_CAPTION_TEXT,
            Self::SplitWith => SPLIT_WITH_TEXT,
            Self::NewInFolder => NEW_IN_FOLDER_TEXT,
            Self::Duplicate => DUPLICATE_PANE_TEXT,
            Self::MoveToNewTab => MOVE_TO_NEW_TAB_TEXT,
            Self::ClosePane => CLOSE_PANE_TEXT,
        }
    }

    /// Whether this row hangs a submenu off itself.
    #[must_use]
    pub fn has_submenu(self) -> bool {
        self == Self::SplitWith
    }
}

/// The submenu heading. The `▸` is drawn rather than written: see
/// [`pane_menu_build`], which strikes the house's `⌄` turned a quarter into the
/// row's trailing edge.
pub const SPLIT_WITH_TEXT: &str = "Split with";
/// The ellipsis is load-bearing: it is this window's promise that a row asks
/// before it acts, and this row opens a system dialog you can cancel.
pub const NEW_IN_FOLDER_TEXT: &str = "New terminal in folder…";
pub const DUPLICATE_PANE_TEXT: &str = "Duplicate pane";
pub const MOVE_TO_NEW_TAB_TEXT: &str = "Move pane to new tab";
/// The `×`'s verb, spelled — a menu row has room for the word the button does
/// not, and `Close pane` is the mock-up's own `title` for that button (4672).
pub const CLOSE_PANE_TEXT: &str = "Close pane";
/// What the submenu writes beside the profile this pane is already running.
///
/// The `.default-hint` slot, on the profile picker's own precedent: the mark
/// column belongs to the profile's own glyph, so "this is the one you are on"
/// has to be said in words rather than with a tick.
pub const CURRENT_PROFILE_HINT_TEXT: &str = "current";

// ── what the pointer and the keyboard are on ───────────────────────────────

/// What a point in one of the menu's two surfaces is over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneMenuHit {
    /// One of the picker's four zones.
    Zone(SplitZone),
    /// A row of the menu proper. Never [`PaneMenuRow::Picker`] — a point inside
    /// the picker's block is a zone or it is nothing.
    Row(PaneMenuRow),
    /// A row of the open submenu, by its index into [`PROFILES`].
    Submenu(usize),
    /// Inside one of the two surfaces but on no control: the padding, the gap
    /// between the picker's slabs, the rule above `Close pane`.
    Surface,
}

/// What is lit — the pointer's hover and the keyboard's cursor, which are one
/// thing in a menu.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneMenuHover {
    /// The picker, with the zone that is lit. The picker's hover *is* a zone:
    /// there is no state in which the block is highlighted and no direction is.
    Zone(SplitZone),
    /// A text row. Never [`PaneMenuRow::Picker`].
    Row(PaneMenuRow),
    /// A row of the open submenu.
    Submenu(usize),
}

/// An arrow key, as the menu's walk sees it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuStep {
    Up,
    Down,
    Left,
    Right,
}

impl PaneMenuHover {
    /// Where an arrow key moves the highlight, or `None` when it moves nothing.
    ///
    /// **The picker is a compass inside a list**, and that is the whole of the
    /// subtlety here. Four zones lie in four directions, so `←→↑↓` must aim at
    /// them; but the picker is also the first entry of a vertical list, so `↓`
    /// must eventually leave it. The rule that satisfies both: an arrow that
    /// names a zone lights that zone, and an arrow that names the zone **already
    /// lit** walks out of the picker in that direction. So `↓ ↓` from the picker
    /// is "aim down, then leave downward", which is what a hand that wanted the
    /// row below would do anyway, and `↑` from the row below comes back in
    /// landing on `Down` — the zone nearest the row it came from.
    ///
    /// `→` and `←` on a row are deliberately absent: they are the submenu's open
    /// and close, which are verbs with side effects (a menu appears, a clock
    /// starts) rather than movements of a highlight, and they are the window's to
    /// run. See `Runtime::step_pane_menu`.
    #[must_use]
    pub fn step(current: Option<Self>, step: MenuStep, submenu_rows: usize) -> Option<Self> {
        match current {
            // Nothing lit: the list is entered at whichever end the key names.
            None => match step {
                MenuStep::Down => Some(Self::Zone(SplitZone::Right)),
                MenuStep::Up => Some(Self::Row(PaneMenuRow::ClosePane)),
                MenuStep::Left | MenuStep::Right => None,
            },
            Some(Self::Zone(zone)) => {
                let aimed = match step {
                    MenuStep::Up => SplitZone::Up,
                    MenuStep::Down => SplitZone::Down,
                    MenuStep::Left => SplitZone::Left,
                    MenuStep::Right => SplitZone::Right,
                };
                if aimed != zone {
                    return Some(Self::Zone(aimed));
                }
                // Already aimed that way: walk out of the picker. Only downward
                // leads anywhere — the picker is the first entry, so `↑` from
                // `Up` is the top of the list and clamps, exactly as every other
                // walk in this window clamps at its ends.
                match step {
                    MenuStep::Down => Some(Self::Row(PaneMenuRow::SplitWith)),
                    _ => None,
                }
            }
            Some(Self::Row(row)) => {
                let index = PaneMenuRow::TEXT_ROWS
                    .iter()
                    .position(|it| *it == row)
                    .expect("a hovered row is one of TEXT_ROWS");
                match step {
                    MenuStep::Down => Some(Self::Row(
                        PaneMenuRow::TEXT_ROWS[(index + 1).min(PaneMenuRow::TEXT_ROWS.len() - 1)],
                    )),
                    MenuStep::Up if index == 0 => Some(Self::Zone(SplitZone::Down)),
                    MenuStep::Up => Some(Self::Row(PaneMenuRow::TEXT_ROWS[index - 1])),
                    MenuStep::Left | MenuStep::Right => None,
                }
            }
            Some(Self::Submenu(index)) => match step {
                MenuStep::Down => Some(Self::Submenu(
                    (index + 1).min(submenu_rows.saturating_sub(1)),
                )),
                MenuStep::Up => Some(Self::Submenu(index.saturating_sub(1))),
                MenuStep::Left | MenuStep::Right => None,
            },
        }
    }
}

// ── the safety triangle (queue item #53, closed here) ──────────────────────

/// Whether a pointer that has left a submenu's heading is **still on its way to
/// the submenu**, and must therefore not be stolen by the row it is crossing.
///
/// **The failure this closes.** A submenu hangs to the side of the row that owns
/// it, so a hand travelling from that row to the submenu's first entry moves
/// diagonally — and every row between the heading and the submenu's top edge
/// passes under the pointer on the way. A menu that hands the highlight to
/// whatever is under the pointer therefore closes the submenu the moment the
/// hand starts moving toward it, which is the version of this feature every
/// user has already met, and hated, and learned to work around by tracing an L
/// with their wrist.
///
/// **The geometry.** Take the pointer's last position as the apex and the two
/// corners of the submenu's *near* vertical edge as the base: everything a hand
/// aiming at any part of that edge would cross lies inside that triangle. A move
/// that lands inside it is a move toward the submenu, and the submenu holds; a
/// move that lands outside it is a move at something else, and the row under the
/// pointer takes over at once. It is Amazon's mega-menu trick, and it is the
/// only one that needs no timer to be *correct* — the timer above it (300ms) is
/// a cap on how long a hand may dawdle inside the triangle, not the mechanism.
///
/// The near edge is chosen by which side of the pointer the submenu is on, so
/// this is equally right for a submenu that had to flip to the left because the
/// window's right edge was too close.
///
/// A pure function of four numbers, which is what lets the whole rule be tested
/// without a menu, a pointer or a clock.
#[must_use]
pub fn safe_triangle_holds(from: [f32; 2], to: [f32; 2], submenu: [f32; 4]) -> bool {
    // A submenu with no area cannot be aimed at.
    if submenu[2] <= submenu[0] || submenu[3] <= submenu[1] {
        return false;
    }
    // The near vertical edge: the submenu's left when it stands to the right of
    // the hand, its right when it stands to the left. A pointer already between
    // the two edges is inside the submenu's own column, where the hit test has
    // already answered — the left edge is as good an answer as any.
    let near_x = if from[0] <= submenu[0] {
        submenu[0]
    } else {
        submenu[2]
    };
    let a = from;
    let b = [near_x, submenu[1]];
    let c = [near_x, submenu[3]];
    let cross = |p: [f32; 2], q: [f32; 2], r: [f32; 2]| {
        (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
    };
    let d1 = cross(a, b, to);
    let d2 = cross(b, c, to);
    let d3 = cross(c, a, to);
    // Inside, or on an edge. A degenerate triangle — the pointer standing
    // exactly on the near edge's line — has every cross product zero, which this
    // reads as "inside", and that is the right answer: a hand on the edge of the
    // submenu is not a hand that has left for somewhere else.
    let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(negative && positive)
}

/// How long the triangle may hold the submenu open against the rows the pointer
/// is crossing.
///
/// The triangle is the mechanism and this is only its cap, but the cap is owed:
/// a hand that stops *inside* the triangle — over some other row, thinking —
/// sends no further events, so without a clock the submenu would stay up under a
/// pointer that has plainly stopped travelling. 300ms is long enough to cross
/// three rows at any speed a hand actually moves and short enough that a stopped
/// hand does not notice waiting.
pub const SUBMENU_SAFE_HOLD: Duration = Duration::from_millis(300);

// ── the geometry ───────────────────────────────────────────────────────────

/// Every rectangle the pane menu draws and hit-tests.
#[derive(Clone, Debug, PartialEq)]
pub struct PaneMenuLayout {
    scale: f32,
    frame: [f32; 4],
    /// One rectangle per entry of [`PaneMenuRow::ALL`], in that order. The
    /// picker's is its whole block.
    items: [[f32; 4]; 6],
    /// The pane at the middle of the picker's diagram.
    picker_pane: [f32; 4],
    /// The four slabs as they are **drawn**, in [`SplitZone::ALL`] order.
    zones: [[f32; 4]; 4],
    /// The four slabs as they are **pressed** — each grown across its gap toward
    /// the pane, so the three logical pixels of air between a slab and the pane
    /// are not a dead band in a control that is already only ten wide. Disjoint
    /// by construction: the gap belongs to exactly one slab.
    zone_hits: [[f32; 4]; 4],
    /// Where the caption sits.
    caption: [f32; 4],
    /// The rule above `Close pane`, which separates the four verbs that *make* a
    /// pane or move one from the one that ends it.
    separator: [f32; 4],
    /// The submenu's frame and rows, when it is open.
    submenu: Option<PaneSubmenuLayout>,
}

/// The `Split with` submenu's own boxes — the house's first.
#[derive(Clone, Debug, PartialEq)]
pub struct PaneSubmenuLayout {
    frame: [f32; 4],
    /// One row per entry of [`PROFILES`], top to bottom.
    items: Vec<[f32; 4]>,
}

impl PaneMenuLayout {
    /// Where one entry landed.
    ///
    /// Read by the tests that pin the geometry; the window walks the entries
    /// through [`pane_menu_hit`] and never needs a named one. It is the
    /// `SettingsLayout::row` arrangement and it is here for that function's
    /// reason: a pin that indexed `items` directly would be a pin that silently
    /// moved to the row below when a verb was inserted above it.
    #[allow(dead_code)]
    #[must_use]
    pub fn item(&self, row: PaneMenuRow) -> [f32; 4] {
        let index = PaneMenuRow::ALL
            .iter()
            .position(|it| *it == row)
            .expect("every row is in ALL");
        self.items[index]
    }

    /// Where one of the picker's zones is drawn. [`Self::item`]'s sibling, and
    /// read by the same pins for the same reason.
    #[allow(dead_code)]
    #[must_use]
    pub fn zone(&self, zone: SplitZone) -> [f32; 4] {
        let index = SplitZone::ALL
            .iter()
            .position(|it| *it == zone)
            .expect("every zone is in ALL");
        self.zones[index]
    }

    /// The submenu's border box, when one is up — the triangle's base, and the
    /// second rectangle a press has to miss.
    #[must_use]
    pub fn submenu_frame(&self) -> Option<[f32; 4]> {
        self.submenu.as_ref().map(|submenu| submenu.frame)
    }

    /// Whether this point is on either surface. Two rectangles, because a menu
    /// with a submenu open is two windows and a pointer on the second one has
    /// not left the first.
    #[must_use]
    pub fn contains(&self, x: f32, y: f32) -> bool {
        contains(self.frame, x, y)
            || self
                .submenu
                .as_ref()
                .is_some_and(|submenu| contains(submenu.frame, x, y))
    }
}

/// The pane menu, hung under the point the head's `⌄` was pressed at.
///
/// [`file_menu_layout`]'s twin, down to the both-axis clamp: a pane head can be
/// the bottom head of a tall stack, and an unclamped drop would put every verb
/// under the window's own edge.
///
/// `submenu_open` rather than a second function, because the submenu's placement
/// is a fact about *this* menu's frame — it hangs off one of these rows and
/// flips to the other side when this frame is already near the window's edge —
/// and a second entry point would be a second opinion about where the parent is.
#[must_use]
pub fn pane_menu_layout(
    point: [f32; 2],
    surface: (f32, f32),
    scale: f32,
    submenu_open: bool,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> PaneMenuLayout {
    let px = |value: f32| value * scale;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let padding = px(MENU_PADDING_LOGICAL_PX);
    let item_height = px(ITEM_HEIGHT_LOGICAL_PX).round();
    let picker_height = px(picker_block_logical_px()).round();
    let separator_thickness = (SEPARATOR_THICKNESS_LOGICAL_PX * scale).round().max(1.0);
    let separator_margin = px(SEPARATOR_MARGIN_Y_LOGICAL_PX).round();
    let separator_block = 2.0 * separator_margin + separator_thickness;

    let chrome = 2.0 * (border + padding) + 2.0 * px(ITEM_PADDING_X_LOGICAL_PX);
    // The submenu's `▸` claims the same slot the profile picker's `default` hint
    // claims, and it is reserved on every row rather than on the one that wears
    // it — a menu whose width depended on which rows had submenus would change
    // width the day a second row grew one.
    let indicator = px(SUBMENU_INDICATOR_LOGICAL_PX) + px(ITEM_GAP_LOGICAL_PX);
    let content = PaneMenuRow::TEXT_ROWS
        .iter()
        .map(|row| {
            px(ITEM_ICON_COLUMN_LOGICAL_PX)
                + px(ITEM_GAP_LOGICAL_PX)
                + measure(row.text(), px(ITEM_FONT_LOGICAL_PX))
                + indicator
        })
        // The diagram is content too, and on a narrow menu it is the widest
        // content there is: a frame that clipped its own picker would be a
        // drawing with a slab missing.
        .fold(px(picker_diagram_width_logical_px()), f32::max);
    let width = (chrome + content)
        .max(px(FILE_MENU_MIN_WIDTH_LOGICAL_PX))
        .round();
    let height = (2.0 * (border + padding)
        + picker_height
        + PaneMenuRow::TEXT_ROWS.len() as f32 * item_height
        + separator_block)
        .round();

    let (surface_width, surface_height) = surface;
    let edge = px(MENU_EDGE_MARGIN_LOGICAL_PX);
    let left = point[0].min(surface_width - width - edge).max(edge).round();
    let top = point[1]
        .min(surface_height - height - edge)
        .max(edge)
        .round();
    let frame = [left, top, left + width, top + height];

    let content_left = frame[0] + border + padding;
    let content_right = frame[2] - border - padding;
    let mut cursor = frame[1] + border + padding;

    // One walk of [`PaneMenuRow::ALL`] lays every entry out, which is what keeps
    // the order on screen and the order the keyboard walks from being two lists.
    let mut items = [[0.0_f32; 4]; PaneMenuRow::ALL.len()];
    let mut separator = [0.0_f32; 4];
    for (index, row) in PaneMenuRow::ALL.iter().enumerate() {
        let height = match row {
            PaneMenuRow::Picker => picker_height,
            _ => item_height,
        };
        // The rule falls where the sentence changes: four verbs that make a pane
        // or move one, then the one that ends it. `#file-menu` puts its own rule
        // after the first row for the same kind of reason (mock-up 8089).
        if *row == PaneMenuRow::ClosePane {
            separator = [
                content_left,
                cursor + separator_margin,
                content_right,
                cursor + separator_margin + separator_thickness,
            ];
            cursor += separator_block;
        }
        items[index] = [content_left, cursor, content_right, cursor + height];
        cursor += height;
    }
    let picker = items[0];

    // The diagram, centred in the picker's block.
    let diagram_width = px(picker_diagram_width_logical_px());
    let diagram_height = px(picker_diagram_height_logical_px());
    let diagram_left = ((picker[0] + picker[2] - diagram_width) / 2.0).round();
    let diagram_top = (picker[1] + px(PICKER_PADDING_TOP_LOGICAL_PX)).round();
    let slab = px(PICKER_ZONE_THICKNESS_LOGICAL_PX).round().max(1.0);
    let gap = px(PICKER_ZONE_GAP_LOGICAL_PX).round();
    let pane_left = diagram_left + slab + gap;
    let pane_top = diagram_top + slab + gap;
    // On whole physical pixels: the diagram is all edges, and an edge on a
    // subpixel is a resampled edge — the crisp hairline the drawing is made of,
    // blurred across two rows of pixels.
    let pane_left = pane_left.round();
    let pane_top = pane_top.round();
    let picker_pane = [
        pane_left,
        pane_top,
        pane_left + px(PICKER_PANE_WIDTH_LOGICAL_PX).round(),
        pane_top + px(PICKER_PANE_HEIGHT_LOGICAL_PX).round(),
    ];
    let zones = [
        // Right
        [
            picker_pane[2] + gap,
            picker_pane[1],
            picker_pane[2] + gap + slab,
            picker_pane[3],
        ],
        // Down
        [
            picker_pane[0],
            picker_pane[3] + gap,
            picker_pane[2],
            picker_pane[3] + gap + slab,
        ],
        // Left
        [
            picker_pane[0] - gap - slab,
            picker_pane[1],
            picker_pane[0] - gap,
            picker_pane[3],
        ],
        // Up
        [
            picker_pane[0],
            picker_pane[1] - gap - slab,
            picker_pane[2],
            picker_pane[1] - gap,
        ],
    ];
    // Each slab's press area reaches back across its own gap to the pane's edge.
    let zone_hits = [
        [picker_pane[2], zones[0][1], zones[0][2], zones[0][3]],
        [zones[1][0], picker_pane[3], zones[1][2], zones[1][3]],
        [zones[2][0], zones[2][1], picker_pane[0], zones[2][3]],
        [zones[3][0], zones[3][1], zones[3][2], picker_pane[1]],
    ];

    let caption_top = diagram_top + diagram_height + px(SECTION_LABEL_PADDING_TOP_LOGICAL_PX);
    let caption = [
        picker[0] + px(SECTION_LABEL_PADDING_X_LOGICAL_PX),
        caption_top.round(),
        picker[2] - px(SECTION_LABEL_PADDING_X_LOGICAL_PX),
        (caption_top + px(SECTION_LABEL_LINE_LOGICAL_PX)).round(),
    ];

    let submenu = submenu_open.then(|| {
        pane_submenu_layout(
            frame,
            items[1],
            surface,
            scale,
            border,
            padding,
            item_height,
            measure,
        )
    });

    PaneMenuLayout {
        scale,
        frame,
        items,
        picker_pane,
        zones,
        zone_hits,
        caption,
        separator,
        submenu,
    }
}

/// Where the `Split with` submenu hangs.
///
/// **To the right of the parent, level with the heading's own top padding, and
/// flipped to the left when the window's right edge is too close** — the two
/// rules every submenu in every product follows, and the second is not optional:
/// a menu opened on a pane head near the right edge is a menu whose child has
/// nowhere to go on that side, and a child clamped instead of flipped would sit
/// *on top of* the parent it hangs from.
#[allow(clippy::too_many_arguments)]
fn pane_submenu_layout(
    parent: [f32; 4],
    heading: [f32; 4],
    surface: (f32, f32),
    scale: f32,
    border: f32,
    padding: f32,
    item_height: f32,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> PaneSubmenuLayout {
    let px = |value: f32| value * scale;
    let hint = measure(CURRENT_PROFILE_HINT_TEXT, px(HINT_FONT_LOGICAL_PX));
    let chrome = 2.0 * (border + padding) + 2.0 * px(ITEM_PADDING_X_LOGICAL_PX);
    let content = (0..PROFILES.len())
        .map(|index| {
            px(ITEM_ICON_COLUMN_LOGICAL_PX)
                + px(ITEM_GAP_LOGICAL_PX)
                + measure(title(index), px(ITEM_FONT_LOGICAL_PX))
                + px(ITEM_GAP_LOGICAL_PX)
                + hint
        })
        .fold(0.0_f32, f32::max);
    let width = (chrome + content)
        .max(px(FILE_MENU_MIN_WIDTH_LOGICAL_PX))
        .round();
    let height = (2.0 * (border + padding) + PROFILES.len() as f32 * item_height).round();

    let (surface_width, surface_height) = surface;
    let edge = px(MENU_EDGE_MARGIN_LOGICAL_PX);
    // Overlapping the parent's edge by its own border and padding, so the two
    // surfaces read as one object with a fold in it rather than as two windows
    // with a seam — and, more practically, so the corridor the safety triangle
    // has to cover is as short as it can be.
    let overlap = border + padding;
    let right_of = parent[2] - overlap;
    let left_of = parent[0] + overlap - width;
    let left = if right_of + width + edge <= surface_width {
        right_of
    } else {
        left_of.max(edge)
    }
    .round();
    let top = (heading[1] - border - padding)
        .min(surface_height - height - edge)
        .max(edge)
        .round();
    let frame = [left, top, left + width, top + height];

    let content_left = frame[0] + border + padding;
    let content_right = frame[2] - border - padding;
    let mut cursor = frame[1] + border + padding;
    let mut items = Vec::with_capacity(PROFILES.len());
    for _ in 0..PROFILES.len() {
        items.push([content_left, cursor, content_right, cursor + item_height]);
        cursor += item_height;
    }
    PaneSubmenuLayout { frame, items }
}

/// What a point is over.
///
/// **The submenu is asked first**, because it is drawn over the parent and
/// overlaps it: a point in the strip where the two frames cross belongs to the
/// child, exactly as the topmost window owns a point everywhere else in this
/// program.
#[must_use]
pub fn pane_menu_hit(layout: &PaneMenuLayout, x: f64, y: f64) -> Option<PaneMenuHit> {
    let (x, y) = (x as f32, y as f32);
    if let Some(submenu) = layout.submenu.as_ref()
        && contains(submenu.frame, x, y)
    {
        for (index, rect) in submenu.items.iter().enumerate() {
            if contains(*rect, x, y) {
                return Some(PaneMenuHit::Submenu(index));
            }
        }
        return Some(PaneMenuHit::Surface);
    }
    if !contains(layout.frame, x, y) {
        return None;
    }
    for (zone, rect) in SplitZone::ALL.iter().zip(layout.zone_hits) {
        if contains(rect, x, y) {
            return Some(PaneMenuHit::Zone(*zone));
        }
    }
    for (row, rect) in PaneMenuRow::TEXT_ROWS.iter().zip(layout.items[1..].iter()) {
        if contains(*rect, x, y) {
            return Some(PaneMenuHit::Row(*row));
        }
    }
    Some(PaneMenuHit::Surface)
}

// ── the drawing ────────────────────────────────────────────────────────────

/// The trailing `▸`, at the file tree's own disclosure size.
const SUBMENU_INDICATOR_LOGICAL_PX: f32 = 9.0;

/// The pane menu and its submenu as overlay layers.
///
/// `current_profile` is the profile the pane is running, which the submenu marks
/// — `None` for a pane whose profile this build cannot name, which is the same
/// silence the profile picker keeps about a machine's missing shells.
#[must_use]
pub fn pane_menu_build(
    layout: &PaneMenuLayout,
    hover: Option<PaneMenuHover>,
    current_profile: Option<usize>,
    programs: &ProfilePrograms,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> Vec<OverlayLayer> {
    let palette = chrome_palette();
    let scale = layout.scale;
    let px = |value: f32| value * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
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

    push_picker(layout, hover, palette, scale, &mut quads, &mut labels);

    for (row, rect) in PaneMenuRow::TEXT_ROWS.iter().zip(layout.items[1..].iter()) {
        push_row(
            &Row {
                rect: *rect,
                mark: row.mark(),
                name: row.text(),
                hint: None,
                hint_ink: None,
                hovered: hover == Some(PaneMenuHover::Row(*row)),
                // A split the solver has no room for is refused *by the solver*,
                // after the press, exactly as the chord's is; a pane can always
                // be closed; and the chooser `New terminal in folder…` opens is
                // Windows', which this window does not get to promise about in
                // advance. Nothing here is a promise this build knows it cannot
                // keep.
                available: true,
            },
            scale,
            palette,
            &mut quads,
            &mut labels,
            &mut sprites,
        );
        if *row == PaneMenuRow::SplitWith {
            // The `▸`, which is `#i-tri` **at rest**: the file tree's disclosure
            // triangle points right until something opens it, and pointing right
            // is the whole of what a submenu indicator says. No new glyph, no new
            // angle, and no fifth close-enough triangle in a build that already
            // has one — see `ChromeMark::TreeDisclosure`, whose own note argues
            // that three marks differing by where a line falls are three marks
            // nobody can tell apart at fourteen pixels.
            let size = px(SUBMENU_INDICATOR_LOGICAL_PX).round().max(1.0);
            let right = rect[2] - px(ITEM_PADDING_X_LOGICAL_PX);
            let top = ((rect[1] + rect[3] - size) / 2.0).round();
            sprites.push(ChromeSprite::new(
                ChromeMark::TreeDisclosure { turned_degrees: 0 },
                [right - size, top, right, top + size],
                if hover == Some(PaneMenuHover::Row(*row)) {
                    palette.menu_item_text_selected
                } else {
                    palette.menu_item_hint_text
                },
            ));
        }
        if *row == PaneMenuRow::MoveToNewTab {
            quads.push(OverlayQuad {
                rect: layout.separator,
                color: palette.menu_border,
                alpha: separator_alpha(palette.menu_border),
            });
        }
    }

    let mut layers = vec![OverlayLayer {
        quads,
        labels,
        sprites,
        ..Default::default()
    }];
    if let Some(submenu) = layout.submenu.as_ref() {
        layers.extend(push_submenu(
            submenu,
            scale,
            hover,
            current_profile,
            programs,
            measure,
        ));
    }
    layers
}

/// The picker's five rectangles and its caption.
fn push_picker(
    layout: &PaneMenuLayout,
    hover: Option<PaneMenuHover>,
    palette: ChromePalette,
    scale: f32,
    quads: &mut Vec<OverlayQuad>,
    labels: &mut Vec<ChromeLabel>,
) {
    let px = |value: f32| value * scale;
    let edge = (PICKER_EDGE_LOGICAL_PX * scale).round().max(1.0);
    let alpha = |value: u8| f32::from(value) / 255.0;
    // A bordered box is two fills — the whole box in the border's colour, the
    // face laid one border in with one border less radius — which is exactly
    // what a browser leaves for `border: 1px solid`. See `rounded_overlay_fill`.
    let outlined = |rect: [f32; 4],
                    radius: f32,
                    ink: [u8; 3],
                    ink_alpha: f32,
                    face: [u8; 3],
                    face_alpha: f32,
                    quads: &mut Vec<OverlayQuad>| {
        quads.extend(rounded_overlay_fill(rect, radius, ink, ink_alpha));
        quads.extend(rounded_overlay_fill(
            [
                rect[0] + edge,
                rect[1] + edge,
                rect[2] - edge,
                rect[3] - edge,
            ],
            (radius - edge).max(0.0),
            face,
            face_alpha,
        ));
    };

    outlined(
        layout.picker_pane,
        px(PICKER_PANE_RADIUS_LOGICAL_PX),
        palette.menu_border,
        alpha(palette.menu_border_alpha),
        palette.menu_surface,
        1.0,
        quads,
    );
    for (zone, rect) in SplitZone::ALL.iter().zip(layout.zones) {
        let lit = hover == Some(PaneMenuHover::Zone(*zone));
        outlined(
            rect,
            px(PICKER_ZONE_RADIUS_LOGICAL_PX),
            if lit {
                palette.accent
            } else {
                palette.menu_border
            },
            if lit {
                1.0
            } else {
                alpha(palette.menu_border_alpha)
            },
            if lit {
                palette.accent
            } else {
                palette.menu_surface
            },
            if lit { PICKER_ZONE_WASH_ALPHA } else { 1.0 },
            quads,
        );
    }
    labels.push(ChromeLabel {
        text: PICKER_CAPTION_TEXT.to_owned(),
        rect: layout.caption,
        font_size_px: px(SECTION_LABEL_FONT_LOGICAL_PX),
        color: palette.menu_item_hint_text,
        align_right: false,
        align_center: true,
        letter_spacing_em: SECTION_LABEL_TRACKING_EM,
        weight: ChromeLabelWeight::SemiBold,
        tabular_numerals: false,
        clip: None,
    });
}

/// The submenu as its own layer, above the parent's.
///
/// Its own layer and not more quads on the parent's, for the reason the overlay
/// stack itself is a list: a child menu overlaps its parent, so what covers what
/// has to be a statement rather than an accident of the order two loops happened
/// to run in.
fn push_submenu(
    layout: &PaneSubmenuLayout,
    scale: f32,
    hover: Option<PaneMenuHover>,
    current_profile: Option<usize>,
    programs: &ProfilePrograms,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> Vec<OverlayLayer> {
    let palette = chrome_palette();
    let px = |value: f32| value * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
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
    for (index, rect) in layout.items.iter().enumerate() {
        let hint = (current_profile == Some(index)).then(|| {
            (
                CURRENT_PROFILE_HINT_TEXT.to_owned(),
                measure(CURRENT_PROFILE_HINT_TEXT, px(HINT_FONT_LOGICAL_PX)),
            )
        });
        push_row(
            &Row {
                rect: *rect,
                mark: Some(PROFILES[index].mark),
                name: title(index),
                hint,
                hint_ink: None,
                hovered: hover == Some(PaneMenuHover::Submenu(index)),
                // The picker's own rule, and the same fact: a profile whose
                // program this machine does not have cannot start a shell, and a
                // row that lights under the pointer and then does nothing is
                // worse than one that says so.
                available: programs.is_available(index),
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

// ── the commit graph's branch filter (T2/T3, v2 ③) ──────────────────────────
//
// The fifth popup in this module and the first one with *state on its rows*: a
// radio, a list of checkboxes and two more checkboxes under a divider. It is
// `#file-menu`'s skin and `push_row`'s rows for the reason every menu here gives
// — a popup that looked like its neighbours but was drawn by different code
// would be a second popup wearing the first one's clothes — and the only thing
// it adds to the family is that a row's mark now says whether the row is *on*.
//
// **Cloned rather than generalised, and the ruling is the one `#pane-menu` made
// first.** The four menus above this one differ in exactly what a menu differs
// in: how many rows, which words, which marks, where the divider falls. A
// generic "menu of rows" that all five went through would have to take all four
// of those as parameters, which is the same code with the layout inverted and
// one more indirection between a row and the rectangle it is drawn in. What is
// actually shared is what should be — `push_row`, `push_float_window`, the
// spacing constants — and that is shared here too.

/// One row of the branch filter.
///
/// A `Vec`-shaped menu and not a closed one, unlike its four neighbours: how
/// many rows it has is how many branches the repository has, which is the one
/// thing a menu in this window has never varied by before. The two ends are
/// fixed and the middle is the repository's.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GitFilterRow {
    /// The radio at the top: walk everything.
    All,
    /// One local branch, by name, with a tick when it is picked.
    Branch(String),
    /// Whether remote-tracking names are drawn and walked.
    Remotes,
    /// Whether tags are.
    Tags,
}

/// The two words under the divider.
pub const GIT_FILTER_REMOTES_TEXT: &str = "Show remote branches";
pub const GIT_FILTER_TAGS_TEXT: &str = "Show tags";

/// Every rectangle the filter menu draws and hit-tests.
#[derive(Clone, Debug, PartialEq)]
pub struct GitFilterMenuLayout {
    scale: f32,
    frame: [f32; 4],
    /// One rectangle per row of [`Self::rows`], in the same order.
    items: Vec<[f32; 4]>,
    rows: Vec<GitFilterRow>,
    /// The rule between the branches and the two flags.
    separator: [f32; 4],
}

impl GitFilterMenuLayout {
    /// The rows this menu is showing, in the order they are drawn.
    ///
    /// Read by the tests that pin the layout; the window walks the rows through
    /// [`git_filter_menu_hit`] and never needs the list itself.
    #[allow(dead_code)]
    #[must_use]
    pub fn rows(&self) -> &[GitFilterRow] {
        &self.rows
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn frame(&self) -> [f32; 4] {
        self.frame
    }
}

/// The rows a repository's branches make, top to bottom.
#[must_use]
pub fn git_filter_rows(branches: &[String]) -> Vec<GitFilterRow> {
    let mut rows = vec![GitFilterRow::All];
    rows.extend(branches.iter().cloned().map(GitFilterRow::Branch));
    rows.push(GitFilterRow::Remotes);
    rows.push(GitFilterRow::Tags);
    rows
}

/// The menu hung under the filter button.
///
/// **Anchored to the button and not to the pointer**, unlike `#file-menu`: it is
/// opened by pressing a control, so it hangs from that control's own bottom-left
/// exactly as the profile picker hangs from its chevron. The caller re-finds the
/// button's rectangle every frame for E59/E60's reason — a re-layout can move it,
/// and a menu pinned to where it *was* is a menu floating over nothing.
#[must_use]
pub fn git_filter_menu_layout(
    anchor: [f32; 4],
    surface: (f32, f32),
    scale: f32,
    rows: Vec<GitFilterRow>,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> GitFilterMenuLayout {
    let px = |value: f32| value * scale;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let padding = px(MENU_PADDING_LOGICAL_PX);
    let item_height = px(ITEM_HEIGHT_LOGICAL_PX).round();
    let separator_thickness = (SEPARATOR_THICKNESS_LOGICAL_PX * scale).round().max(1.0);
    let separator_margin = px(SEPARATOR_MARGIN_Y_LOGICAL_PX).round();
    let separator_block = 2.0 * separator_margin + separator_thickness;

    let chrome = 2.0 * (border + padding) + 2.0 * px(ITEM_PADDING_X_LOGICAL_PX);
    let content = rows
        .iter()
        .map(|row| {
            px(ITEM_ICON_COLUMN_LOGICAL_PX)
                + px(ITEM_GAP_LOGICAL_PX)
                + measure(git_filter_text(row), px(ITEM_FONT_LOGICAL_PX))
        })
        .fold(0.0_f32, f32::max);
    let width = (chrome + content)
        .max(px(FILE_MENU_MIN_WIDTH_LOGICAL_PX))
        .round();
    #[allow(clippy::cast_precision_loss)]
    let height =
        (2.0 * (border + padding) + rows.len() as f32 * item_height + separator_block).round();

    // Both axes clamped, on `#file-menu`'s reasoning: the button this hangs from
    // is on a preview seat's own toolbar, which can be anywhere in the window —
    // including the bottom of a short pane, where an unclamped drop would put
    // every branch under the window's edge.
    let (surface_width, surface_height) = surface;
    let edge = px(MENU_EDGE_MARGIN_LOGICAL_PX);
    let left = anchor[0]
        .min(surface_width - width - edge)
        .max(edge)
        .round();
    let top = anchor[3]
        .min(surface_height - height - edge)
        .max(edge)
        .round();
    let frame = [left, top, left + width, top + height];

    let content_left = frame[0] + border + padding;
    let content_right = frame[2] - border - padding;
    let mut cursor = frame[1] + border + padding;
    let mut items = Vec::with_capacity(rows.len());
    let mut separator = [content_left, cursor, content_right, cursor];
    for row in &rows {
        // The divider stands above the first of the two flags, which is where
        // the menu stops being about *which history* and starts being about
        // *which names*.
        if *row == GitFilterRow::Remotes {
            separator = [
                content_left,
                cursor + separator_margin,
                content_right,
                cursor + separator_margin + separator_thickness,
            ];
            cursor += separator_block;
        }
        items.push([content_left, cursor, content_right, cursor + item_height]);
        cursor += item_height;
    }
    GitFilterMenuLayout {
        scale,
        frame,
        items,
        rows,
        separator,
    }
}

/// What a point is over, with the same three answers the other menus give:
/// a row, the frame but no row, or nothing at all.
#[must_use]
pub fn git_filter_menu_hit(
    layout: &GitFilterMenuLayout,
    x: f64,
    y: f64,
) -> Option<Option<GitFilterRow>> {
    let (x, y) = (x as f32, y as f32);
    for (row, rect) in layout.rows.iter().zip(&layout.items) {
        if contains(*rect, x, y) {
            return Some(Some(row.clone()));
        }
    }
    contains(layout.frame, x, y).then_some(None)
}

/// What each row says.
#[must_use]
pub fn git_filter_text(row: &GitFilterRow) -> &str {
    match row {
        GitFilterRow::All => crate::git_graph::GRAPH_FILTER_ALL,
        GitFilterRow::Branch(name) => name,
        GitFilterRow::Remotes => GIT_FILTER_REMOTES_TEXT,
        GitFilterRow::Tags => GIT_FILTER_TAGS_TEXT,
    }
}

/// Whether a row is currently on, given the filter it is about.
#[must_use]
pub fn git_filter_row_on(row: &GitFilterRow, filter: &crate::git_graph::GraphFilter) -> bool {
    match row {
        GitFilterRow::All => filter.all_branches(),
        GitFilterRow::Branch(name) => filter.branches.iter().any(|held| held == name),
        GitFilterRow::Remotes => filter.remotes,
        GitFilterRow::Tags => filter.tags,
    }
}

/// The mark a row wears for its state.
///
/// **A radio is a dot and a checkbox is a tick**, which is the distinction the
/// Git page's own branch list already draws (G35): a filled circle is a *state*
/// — this is the one — and it is only ever used where exactly one of the rows
/// can be it. The branch rows and the two flags are checkboxes, several of which
/// can be on at once, so they get the tick this window already uses for "done"
/// and nothing at all when they are off.
#[must_use]
fn git_filter_mark(row: &GitFilterRow, on: bool) -> Option<ChromeMark> {
    match (row, on) {
        (GitFilterRow::All, true) => Some(ChromeMark::ControlPill {
            radius_px: GIT_FILTER_RADIO_RADIUS_PX,
        }),
        (GitFilterRow::All, false) => Some(ChromeMark::ControlPillRing {
            radius_px: GIT_FILTER_RADIO_RADIUS_PX,
            stroke_px: 1,
        }),
        (_, true) => Some(ChromeMark::Check),
        (_, false) => None,
    }
}

/// The radio's own round, in the raster's own pixels — half of a mark slot, so
/// the pill it asks for is a circle rather than a lozenge.
const GIT_FILTER_RADIO_RADIUS_PX: u32 = 8;

/// The filter menu as one overlay layer.
#[must_use]
pub fn git_filter_menu_build(
    layout: &GitFilterMenuLayout,
    filter: &crate::git_graph::GraphFilter,
    hover: Option<&GitFilterRow>,
) -> Vec<OverlayLayer> {
    let palette = chrome_palette();
    let scale = layout.scale;
    let px = |value: f32| value * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
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
    quads.push(OverlayQuad {
        rect: layout.separator,
        color: palette.menu_border,
        alpha: separator_alpha(palette.menu_border),
    });

    for (row, rect) in layout.rows.iter().zip(&layout.items) {
        let on = git_filter_row_on(row, filter);
        push_row(
            &Row {
                rect: *rect,
                mark: git_filter_mark(row, on),
                name: git_filter_text(row),
                hint: None,
                hint_ink: None,
                hovered: hover == Some(row),
                // Every row here is a setting, and a setting can always be set.
                // There is no machine on which one of these is a promise that
                // cannot be kept: what the *repository* answers to a filter is
                // git's business, and an empty graph is an honest answer.
                available: true,
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

// ── the preview pane's buffer switcher (`#pv-menu`, mock-up 5085-5138) ───────
//
// "Every live buffer, dirty dots and all — **the dropdown IS the honest
// inventory of hidden state**" (P130, `DESIGN.md` §7.1.3). It wears `.term-menu`'s
// skin like `#file-menu` does, and it is built out of the same `push_row` every
// other menu in this module is: the block's own instruction was to reuse the
// context-menu family rather than mint a second popup, and a menu that looked
// like its neighbours but was drawn by different code would be a second popup
// wearing the first one's clothes.

/// The dot a dirty buffer wears in the switcher — `.pvm-dot` (mock-up 581).
pub const PREVIEW_MENU_DIRTY_DOT: &str = "\u{25cf}";

/// One row of the switcher: a live buffer in the tab's pool.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewMenuItem {
    pub name: String,
    /// Unsaved edits — the dot on the right of the row.
    pub dirty: bool,
    /// Whether this is the buffer the pane is showing (`.tm-item.cur`).
    pub current: bool,
}

/// Which preview pane's switcher is up, and which row the pointer is on.
///
/// The seat is *in* the state, exactly as [`RootMenu`]'s is, and for the reason
/// that one spells out at length: it is what makes "one close path so the chevron
/// never gets stranded flipped" (P133, "the root menu's lesson, applied on day
/// one this time") true by construction. The chevron is re-derived from this on
/// the next frame; there is no second place holding an "open" flag to forget.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PreviewMenu {
    open: Option<bt_layout::SeatId>,
    hover: Option<usize>,
}

impl PreviewMenu {
    pub fn seat(self) -> Option<bt_layout::SeatId> {
        self.open
    }

    /// The name button: open here, or shut if this very pane already has it open
    /// (P136 — "同一个 `data-leaf` 再点 → 收").
    pub fn toggle(&mut self, seat: bt_layout::SeatId) {
        self.open = (self.open != Some(seat)).then_some(seat);
        self.hover = None;
    }

    pub fn close(&mut self) -> bool {
        let was_open = self.open.is_some();
        self.open = None;
        self.hover = None;
        was_open
    }

    pub fn set_hover(&mut self, hover: Option<usize>) -> bool {
        let hover = self.open.and(hover);
        let changed = self.hover != hover;
        self.hover = hover;
        changed
    }

    pub fn hover(self) -> Option<usize> {
        self.hover
    }
}

/// Every rectangle the switcher draws and hit-tests.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewMenuLayout {
    scale: f32,
    frame: [f32; 4],
    items: Vec<[f32; 4]>,
}

/// The switcher hung under the head's file name.
///
/// `top = anchor.bottom + 4`, `left = clamp(anchor.left, 6, width - menu - 8)` —
/// mock-up 5109-5113, which is [`root_menu_layout`]'s pair of lines because it is
/// the same gesture with a different list under it.
#[must_use]
pub fn preview_menu_layout(
    anchor: [f32; 4],
    surface: (f32, f32),
    scale: f32,
    items: &[PreviewMenuItem],
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> PreviewMenuLayout {
    let px = |value: f32| value * scale;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let padding = px(MENU_PADDING_LOGICAL_PX);
    let item_height = px(ITEM_HEIGHT_LOGICAL_PX).round();

    // The dot's column is reserved on **every** row, dirty or not, for the rule
    // the root menu's note already states: a menu that changed width because a
    // buffer was edited would move under the pointer. It is the same reservation
    // the header's own `.pv-dirty { width: 12px }` makes, for the same reason.
    let dot = measure(PREVIEW_MENU_DIRTY_DOT, px(HINT_FONT_LOGICAL_PX)) + px(ITEM_GAP_LOGICAL_PX);
    let chrome = 2.0 * (border + padding) + 2.0 * px(ITEM_PADDING_X_LOGICAL_PX);
    let content = items
        .iter()
        .map(|item| {
            px(ITEM_ICON_COLUMN_LOGICAL_PX)
                + px(ITEM_GAP_LOGICAL_PX)
                + measure(&item.name, px(ITEM_FONT_LOGICAL_PX))
                + dot
        })
        .fold(0.0_f32, f32::max);
    // A file name is arbitrary and chosen by nobody here, so the widest one does
    // not get to stretch the popup across the window — the clamp the root menu
    // puts on its directory names, for the identical reason.
    let width = (chrome + content)
        .clamp(
            px(FILE_MENU_MIN_WIDTH_LOGICAL_PX),
            px(FILE_MENU_MIN_WIDTH_LOGICAL_PX + RECENT_ITEM_MAX_WIDTH_LOGICAL_PX),
        )
        .round();
    let height = (2.0 * (border + padding) + item_height * items.len() as f32).round();

    let (surface_width, surface_height) = surface;
    let edge = px(MENU_EDGE_MARGIN_LOGICAL_PX);
    let left = anchor[0]
        .min(surface_width - width - edge)
        .max(edge)
        .round();
    // Both axes clamped, unlike the root menu's one: a preview pane can be the
    // bottom half of a split, and a pool of eight buffers hung under a head down
    // there would put its last rows below the window.
    let top = (anchor[3] + px(MENU_OFFSET_LOGICAL_PX))
        .min(surface_height - height - edge)
        .max(edge)
        .round();
    let frame = [left, top, left + width, top + height];

    let content_left = frame[0] + border + padding;
    let content_right = frame[2] - border - padding;
    let mut cursor = frame[1] + border + padding;
    let items = items
        .iter()
        .map(|_| {
            let rect = [content_left, cursor, content_right, cursor + item_height];
            cursor += item_height;
            rect
        })
        .collect();
    PreviewMenuLayout {
        scale,
        frame,
        items,
    }
}

/// What a point is over, with the same three answers every other menu gives:
/// `None` for "not this menu at all", `Some(None)` for "the menu but no row".
#[must_use]
pub fn preview_menu_hit(layout: &PreviewMenuLayout, x: f64, y: f64) -> Option<Option<usize>> {
    let (x, y) = (x as f32, y as f32);
    for (index, item) in layout.items.iter().enumerate() {
        if contains(*item, x, y) {
            return Some(Some(index));
        }
    }
    contains(layout.frame, x, y).then_some(None)
}

/// The switcher as one overlay layer.
#[must_use]
pub fn preview_menu_build(
    layout: &PreviewMenuLayout,
    items: &[PreviewMenuItem],
    hover: Option<usize>,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> Vec<OverlayLayer> {
    let palette = chrome_palette();
    let scale = layout.scale;
    let px = |value: f32| value * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
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

    let dot_width = measure(PREVIEW_MENU_DIRTY_DOT, px(HINT_FONT_LOGICAL_PX));
    for (index, (item, rect)) in items.iter().zip(&layout.items).enumerate() {
        push_row(
            &Row {
                rect: *rect,
                mark: Some(ChromeMark::File),
                name: &item.name,
                // Reserved on every row and inked on the dirty ones. Drawn as an
                // empty string rather than omitted so the name's box ends in the
                // same place down the whole list — see the reservation in
                // [`preview_menu_layout`].
                hint: Some((
                    if item.dirty {
                        PREVIEW_MENU_DIRTY_DOT.to_owned()
                    } else {
                        String::new()
                    },
                    dot_width,
                )),
                hint_ink: Some(palette.accent),
                // `.tm-item.cur { background: var(--hover) }` is the same fill
                // the pointer draws, which is what the mock-up asks for: the row
                // you are on and the row you are pointing at look alike, and when
                // they are the same row there is nothing to reconcile.
                hovered: hover == Some(index) || item.current,
                available: true,
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

/// `.term-menu { min-width: 172px }` (mock-up 638), which `#file-menu` wears —
/// the mock-up gives the two the same skin and says so at K145.
///
/// Narrower than either of the other two menus, and rightly: its rows are three
/// fixed verbs rather than directory names of unknown length, so nothing in it
/// can grow, and a floor sized for names it will never hold would leave a band
/// of empty menu beside every row.
const FILE_MENU_MIN_WIDTH_LOGICAL_PX: f32 = 172.0;

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
            ..crate::seats::TabTrailer::default()
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

    // ── P130-P137: the preview's filename switcher ──────────────────────────

    fn pool(names: &[(&str, bool, bool)]) -> Vec<PreviewMenuItem> {
        names
            .iter()
            .map(|(name, dirty, current)| PreviewMenuItem {
                name: (*name).to_owned(),
                dirty: *dirty,
                current: *current,
            })
            .collect()
    }

    /// PIN (P130-P133) — **the switcher is the honest inventory of hidden
    /// state**: every live buffer, in the pool's order, each with its own dirty
    /// dot, and the current one marked.
    ///
    /// MUTATIONS:
    /// ① list only the clean buffers — the dirty rows vanish and the dropdown
    ///    stops being the one place unsaved work is visible;
    /// ② draw the dot only where it is dirty *and* size the row to it — the
    ///    layout's width changes with the dirty bit and the menu moves under the
    ///    pointer when a buffer is edited;
    /// ③ ink the dot from `menu_item_hint_text` — it stops being the header's own
    ///    dot and starts looking like a note.
    #[test]
    fn the_switcher_lists_every_buffer_with_its_own_dot() {
        let items = pool(&[
            ("a.txt", true, false),
            ("notes.md", false, true),
            ("main.rs", false, false),
        ]);
        let layout = preview_menu_layout(
            [40.0, 8.0, 140.0, 27.0],
            (960.0, 600.0),
            1.0,
            &items,
            &mut fake_measure,
        );
        let layer = one_layer(preview_menu_build(&layout, &items, None, &mut fake_measure));
        for (name, _, _) in [("a.txt", 0, 0), ("notes.md", 0, 0), ("main.rs", 0, 0)] {
            assert!(
                layer.labels.iter().any(|label| label.text == name),
                "{name} is in the inventory"
            );
        }
        let palette = chrome_palette();
        let dots: Vec<_> = layer
            .labels
            .iter()
            .filter(|label| label.text == PREVIEW_MENU_DIRTY_DOT)
            .collect();
        assert_eq!(dots.len(), 1, "one dot, for the one dirty buffer");
        assert_eq!(dots[0].color, palette.accent);

        // The width does not move with the dirty bit: the dot's column is
        // reserved on every row.
        let clean = pool(&[
            ("a.txt", false, false),
            ("notes.md", false, true),
            ("main.rs", false, false),
        ]);
        let clean_layout = preview_menu_layout(
            [40.0, 8.0, 140.0, 27.0],
            (960.0, 600.0),
            1.0,
            &clean,
            &mut fake_measure,
        );
        assert_eq!(clean_layout.frame, layout.frame);

        // Every row answers for its own rectangle, and the box answers for the
        // rest of itself — a press on the menu's padding is the menu's.
        for (index, item) in layout.items.iter().enumerate() {
            let (x, y) = ((item[0] + item[2]) / 2.0, (item[1] + item[3]) / 2.0);
            assert_eq!(
                preview_menu_hit(&layout, f64::from(x), f64::from(y)),
                Some(Some(index))
            );
        }
        assert_eq!(
            preview_menu_hit(
                &layout,
                f64::from(layout.frame[0] + 1.0),
                f64::from(layout.frame[1] + 1.0)
            ),
            Some(None)
        );
        assert_eq!(preview_menu_hit(&layout, 1.0, 1.0), None);
    }

    /// PIN (P133/P136) — **one close path, and the button that opened it
    /// collapses it.**
    ///
    /// The chevron's angle is derived from this state on the next frame, so there
    /// is nothing to un-flip: shutting the menu *is* turning it back. That is the
    /// mock-up's own note — "the root menu's lesson, applied on day one this
    /// time".
    ///
    /// Mutation: make `toggle` always open, and a second press on the name leaves
    /// the menu up with its chevron flipped for good.
    #[test]
    fn the_switcher_belongs_to_one_pane_and_its_own_button_shuts_it() {
        let mut menu = PreviewMenu::default();
        let one = bt_layout::SeatId(1);
        let two = bt_layout::SeatId(2);
        assert_eq!(menu.seat(), None);
        menu.toggle(one);
        assert_eq!(menu.seat(), Some(one));
        menu.toggle(one);
        assert_eq!(menu.seat(), None, "the same button shuts it");
        menu.toggle(one);
        menu.toggle(two);
        assert_eq!(menu.seat(), Some(two), "and another pane's takes it over");
        assert!(menu.set_hover(Some(1)));
        assert_eq!(menu.hover(), Some(1));
        assert!(menu.close());
        assert_eq!(menu.hover(), None, "a shut menu is over nothing");
        assert!(!menu.close(), "and closing it again is not a change");
        assert!(!menu.set_hover(Some(0)));
    }

    // ── E53-E61: the root menu ─────────────────────────────────────────────

    /// PIN — E54. The list runs from the most permanent address to the most
    /// local, says why each place is on it, and names each place once.
    #[test]
    fn the_root_menu_offers_home_then_the_shells_then_one_level_up() {
        let choices = root_choices(
            r"C:\work\project",
            Some(r"C:\Users\dev"),
            &[r"D:\repos\api".to_owned(), r"C:\work".to_owned()],
        );
        assert_eq!(
            choices,
            vec![
                RootChoice {
                    path: r"C:\Users\dev".to_owned(),
                    note: RootNote::Home
                },
                RootChoice {
                    path: r"D:\repos\api".to_owned(),
                    note: RootNote::Terminal
                },
                RootChoice {
                    path: r"C:\work".to_owned(),
                    note: RootNote::Terminal
                },
            ],
            "the parent is already on the list as a terminal, and is not repeated"
        );

        // With no shell standing in it, the parent is offered as the parent.
        let choices = root_choices(r"C:\work\project", Some(r"C:\Users\dev"), &[]);
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[1].path, r"C:\work");
        assert_eq!(choices[1].note, RootNote::Parent);
    }

    /// PIN — a home directory a shell happens to be standing in is offered once,
    /// and called home.
    #[test]
    fn one_folder_is_one_row_however_many_reasons_it_has_to_be_there() {
        let choices = root_choices(
            r"C:\Users\dev\work",
            Some(r"C:\Users\dev"),
            &[r"C:\Users\dev".to_owned()],
        );
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].note, RootNote::Home);
    }

    /// PIN — the top of a drive has no parent, and an unrooted column has no
    /// list of its own to be at the top of.
    #[test]
    fn a_root_at_the_top_of_its_drive_offers_no_step_up() {
        let choices = root_choices(r"C:\", None, &[]);
        assert!(
            choices.is_empty(),
            "`C:\\` has no parent, and there is nothing else to offer: {choices:?}"
        );
        assert!(root_choices("", None, &[]).is_empty());
        assert!(
            root_choices(r"C:\work\", None, &[])
                .iter()
                .any(|choice| choice.path == r"C:\"),
            "a trailing separator does not hide the folder above"
        );
    }

    #[test]
    fn the_menu_hangs_under_the_root_button_and_stays_inside_the_window() {
        let choices = root_choices(r"C:\work\project", Some(r"C:\Users\dev"), &[]);
        let button = [40.0, 8.0, 140.0, 27.0];
        let layout = root_menu_layout(button, (960.0, 600.0), 1.0, &choices, &mut fake_measure);
        let frame = layout.frame;
        assert_eq!(frame[1], button[3] + MENU_OFFSET_LOGICAL_PX);
        assert_eq!(frame[0], button[0], "it shares the button's left edge");

        // A button near the right edge pulls the menu back inside rather than
        // hanging it off the window.
        let far = [900.0, 8.0, 950.0, 27.0];
        let clamped = root_menu_layout(far, (960.0, 600.0), 1.0, &choices, &mut fake_measure);
        assert!(clamped.frame[2] <= 960.0 - MENU_EDGE_MARGIN_LOGICAL_PX);
    }

    /// PIN — the rows are what answer, the body is the menu's own, and outside is
    /// nobody's. The same three answers the picker gives, because a press
    /// outside a popup has to reach what it was aimed at.
    #[test]
    fn a_root_row_answers_the_press_and_the_body_swallows_it() {
        let choices = root_choices(r"C:\work\project", Some(r"C:\Users\dev"), &[]);
        let layout = root_menu_layout(
            [40.0, 8.0, 140.0, 27.0],
            (960.0, 600.0),
            1.0,
            &choices,
            &mut fake_measure,
        );
        let frame = layout.frame;
        let first = layout
            .tips(&choices)
            .next()
            .expect("the menu has a first row");
        let middle = |rect: [f32; 4]| ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0);
        let (x, y) = middle(first.1);
        assert_eq!(
            root_menu_hit(&layout, f64::from(x), f64::from(y)),
            Some(Some(RootMenuRow::Choice(0)))
        );
        assert_eq!(
            root_menu_hit(
                &layout,
                f64::from(frame[0] + 1.0),
                f64::from(frame[1] + 1.0)
            ),
            Some(None),
            "the padding above the first row is the menu's own"
        );
        assert_eq!(
            root_menu_hit(
                &layout,
                f64::from(frame[0] - 4.0),
                f64::from(frame[1] - 4.0)
            ),
            None
        );
        assert_eq!(first.2, r"C:\Users\dev", "the tip says the whole path");
    }

    /// PIN — `Browse…` is the last row, it is below a rule, and it is reachable
    /// (E55).
    ///
    /// The red gate this replaces: for one whole block the menu could only offer
    /// paths the application already knew — HOME, a shell's cwd, the parent —
    /// so a folder that was none of those three was not reachable from the
    /// window at all. Asserting the row's *position* as well as its existence is
    /// what stops it from being quietly re-ordered into the list of places,
    /// where it would read as one of them.
    #[test]
    fn browse_is_the_last_row_of_the_root_menu_and_answers_a_press() {
        let choices = root_choices(r"C:\work\project", Some(r"C:\Users\dev"), &[]);
        let layout = root_menu_layout(
            [40.0, 8.0, 140.0, 27.0],
            (960.0, 600.0),
            1.0,
            &choices,
            &mut fake_measure,
        );
        let last_choice = *layout.items.last().expect("the menu offers somewhere");
        assert!(
            layout.browse_separator[1] >= last_choice[3],
            "the rule sits under the last place, not over it"
        );
        assert!(
            layout.browse[1] >= layout.browse_separator[3],
            "and `Browse…` sits under the rule"
        );
        assert!(
            layout.browse[3] <= layout.frame[3],
            "the menu is tall enough to hold the row it grew"
        );

        let middle = |rect: [f32; 4]| ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0);
        let (x, y) = middle(layout.browse);
        assert_eq!(
            root_menu_hit(&layout, f64::from(x), f64::from(y)),
            Some(Some(RootMenuRow::Browse))
        );
        let (x, y) = middle(layout.browse_separator);
        assert_eq!(
            root_menu_hit(&layout, f64::from(x), f64::from(y)),
            Some(None),
            "the rule is body — pressing a hairline commits nothing"
        );

        let layer = one_layer(root_menu_build(
            &layout,
            &choices,
            r"C:\work\project",
            Some(RootMenuRow::Browse),
            &mut fake_measure,
        ));
        assert!(
            layer.labels.iter().any(|label| label.text == BROWSE_TEXT),
            "the row says its own name"
        );
    }

    /// PIN — the folder the column is already rooted at is the one drawn open.
    /// That mark is the menu's "you are here", and getting it from the *current
    /// root* rather than from the hover is what keeps it from following the
    /// pointer around.
    #[test]
    fn the_folder_a_column_is_already_in_is_the_one_drawn_open() {
        let choices = root_choices(r"C:\work\project", Some(r"C:\Users\dev"), &[]);
        let layout = root_menu_layout(
            [40.0, 8.0, 140.0, 27.0],
            (960.0, 600.0),
            1.0,
            &choices,
            &mut fake_measure,
        );
        let open_marks = |current: &str| {
            let layer = one_layer(root_menu_build(
                &layout,
                &choices,
                current,
                None,
                &mut fake_measure,
            ));
            layer
                .sprites
                .iter()
                .filter(|sprite| sprite.mark == ChromeMark::FolderOpen)
                // Only among the *choices*. `Browse…` wears the open folder
                // unconditionally (it means "go and look", not "you are here"),
                // so counting the whole menu would let a choice row lose its
                // mark without the count noticing.
                .filter(|sprite| {
                    layout
                        .items
                        .iter()
                        .any(|item| sprite.rect[1] >= item[1] && sprite.rect[3] <= item[3])
                })
                .count()
        };
        assert_eq!(open_marks(r"C:\Users\dev"), 1);
        assert_eq!(
            open_marks(r"C:\work\project"),
            0,
            "the column's own root is not one of the places it offers to go"
        );
    }

    /// PIN — E57. The button toggles, and opening one column's menu is closing
    /// every other column's.
    #[test]
    fn the_root_menu_belongs_to_one_column_at_a_time_and_its_button_shuts_it() {
        let (a, b) = (bt_layout::SeatId(1), bt_layout::SeatId(2));
        let mut menu = RootMenu::default();
        assert!(menu.seat().is_none());
        menu.toggle(a);
        assert_eq!(menu.seat(), Some(a));
        menu.toggle(b);
        assert_eq!(menu.seat(), Some(b), "the second column takes it over");
        menu.toggle(b);
        assert!(menu.seat().is_none(), "and its own button shuts it");
        assert!(!menu.close(), "there was nothing left to shut");

        menu.toggle(a);
        assert!(menu.set_hover(Some(RootMenuRow::Choice(1))));
        assert_eq!(menu.hover(), Some(RootMenuRow::Choice(1)));
        menu.close();
        assert!(
            !menu.set_hover(Some(RootMenuRow::Choice(1))),
            "a shut menu has no row under the pointer"
        );
        assert_eq!(menu.hover(), None);
    }

    /// What the PowerShell 7 row is called, read from the table rather than
    /// written down.
    ///
    /// The rows below are about *drawing* — this row's name is inked here, greyed
    /// there, tipped with what its caption left out — and none of them is about
    /// what the name happens to be. Spelling it out made every one of them a
    /// second, accidental copy of `PROFILES[…].title`, so the 7 / 5.1 rename came
    /// back as six failures in tests that had no opinion about it.
    fn powershell_seven() -> &'static str {
        PROFILES[index_of_id("pwsh")].title
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

        /// A machine with all five shells on it, spelled the way a real Windows
        /// install spells them.
        fn fully_equipped() -> Self {
            Self::bare_windows()
                .with_var("ProgramFiles", r"C:\Program Files")
                .with_file(r"C:\Program Files\PowerShell\7\pwsh.exe")
                .with_file(r"C:\WINDOWS\System32\wsl.exe")
                .with_file(r"C:\WINDOWS\System32\cmd.exe")
                .with_file(r"C:\Program Files\Git\bin\bash.exe")
        }

        /// Windows with nothing installed on top of it — which still has Windows
        /// PowerShell, because that one ships *inside* the OS.
        ///
        /// A fixture without it would be a machine that does not exist, and it
        /// would quietly make [`FALLBACK_PROFILE`] unavailable, which is the one
        /// thing this module guarantees can never happen.
        fn bare_windows() -> Self {
            Self::default()
                .with_var("SystemRoot", r"C:\WINDOWS")
                .with_file(r"C:\WINDOWS\System32\WindowsPowerShell\v1.0\powershell.exe")
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

    /// A bare Windows box: Windows PowerShell and nothing else this product can
    /// start — not even PowerShell 7, which is an install rather than part of
    /// the OS.
    fn bare() -> ProfilePrograms {
        ProfilePrograms::probe(&FakeMachine::bare_windows())
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
            previews: Vec::new(),
            at: at(100_000 - secs_ago),
        }
    }

    fn files(root: &str, secs_ago: u64) -> RecentEntry {
        RecentEntry {
            seed: Seed::Files {
                root: root.to_owned(),
            },
            previews: Vec::new(),
            at: at(100_000 - secs_ago),
        }
    }

    /// The height the Recent section adds at `scale`: `.menu-sep` with its two
    /// margins, `.menu-label` with its padding, and one row per seed.
    /// The `Files pane` section: one rule and one row, always drawn.
    ///
    /// Named beside [`recent_block`] so the three height pins state the menu's
    /// shape the same way, and so the day this section grows a second row there
    /// is one place to say so.
    fn files_block(scale: f32) -> f32 {
        separator_block(scale) + (ITEM_HEIGHT_LOGICAL_PX * scale).round()
    }

    /// `.menu-sep` — 1px between two 5px margins.
    fn separator_block(scale: f32) -> f32 {
        2.0 * (SEPARATOR_MARGIN_Y_LOGICAL_PX * scale).round()
            + (SEPARATOR_THICKNESS_LOGICAL_PX * scale).round().max(1.0)
    }

    fn recent_block(scale: f32, rows: usize) -> f32 {
        let separator = separator_block(scale);
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
                &mut fake_measure,
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
            // `min-width: 180px` is a **floor**, and the mock-up's rows are
            // content-sized over it (`white-space: nowrap`). Reading it as a
            // fixed width clipped `Windows PowerShell` mid-glyph the day that
            // row shipped, so the assertion is the declaration rather than a
            // number: at least the minimum, and exactly what the longest row
            // needs when that is more.
            let width = (frame[2] - frame[0]).round();
            assert!(
                width >= (180.0 * scale).round(),
                "scale {scale}: `min-width: 180px`, got {width}"
            );
            let longest = (0..PROFILES.len())
                .map(|index| fake_measure(title(index), ITEM_FONT_LOGICAL_PX * scale))
                .fold(0.0_f32, f32::max);
            let annotation = fake_measure(HINT_TEXT, HINT_FONT_LOGICAL_PX * scale).max(
                fake_measure(UNAVAILABLE_HINT_TEXT, HINT_FONT_LOGICAL_PX * scale),
            );
            let chrome = 2.0
                * ((FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0)
                    + MENU_PADDING_LOGICAL_PX * scale
                    + ITEM_PADDING_X_LOGICAL_PX * scale);
            assert_eq!(
                width,
                (chrome
                    + ITEM_ICON_COLUMN_LOGICAL_PX * scale
                    + 2.0 * ITEM_GAP_LOGICAL_PX * scale
                    + longest
                    + annotation)
                    .max(180.0 * scale)
                    .round(),
                "scale {scale}: and the longest row decides the rest"
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

        let open = layout(
            chevron,
            MenuSide::Beside,
            (1400.0, 900.0),
            scale,
            NO_RECENT,
            &mut fake_measure,
        );
        assert_eq!(
            open.frame[0],
            (chevron[2] + 4.0 * scale).round(),
            "the menu stands clear of the chevron's right edge, not under it"
        );
        assert_eq!(
            open.frame[1], chevron[1],
            "and level with its top rather than below its bottom"
        );

        let parked = layout(
            plus,
            MenuSide::Beside,
            (1400.0, 900.0),
            scale,
            NO_RECENT,
            &mut fake_measure,
        );
        assert_eq!(
            parked.frame[1], open.frame[1],
            "a parked rail anchors on the `+` instead, and the two share a top, \
             so the menu does not jump as the panel opens"
        );

        // The `Below` placement is still the strip's own, and still different.
        let strip = layout(
            chevron,
            MenuSide::Below,
            (1400.0, 900.0),
            scale,
            NO_RECENT,
            &mut fake_measure,
        );
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
            &mut fake_measure,
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
    /// to: a list of rows that all started PowerShell would have been rows that
    /// cannot do what they say. What makes the list honest now is the rest of
    /// this file — a program per profile, and a greyed row where the machine
    /// has none.
    ///
    /// The two PowerShells sit adjacent and in that order (user ruling
    /// 2026-08-11): 7 first because it is the one a person who installed it
    /// means by "PowerShell", 5.1 beside it because the pair is the choice.
    #[test]
    fn the_picker_offers_exactly_the_profiles_this_build_has() {
        assert_eq!(PROFILES.len(), 5);
        let listed: Vec<_> = PROFILES.iter().map(|profile| profile.id).collect();
        assert_eq!(listed, ["pwsh", "winps", "wsl", "gitbash", "cmd"]);
        assert_eq!(PROFILES[FALLBACK_PROFILE].title, "Windows PowerShell 5.1");

        // **Mark × title, and not the mark alone.** This used to require every
        // mark to be distinct, and the two PowerShells retire that: they are one
        // family and there is one PowerShell symbol in the mock-up, so drawing a
        // second glyph would assert a visual distinction the family does not
        // have. What has to stay unique is what `docs/UI-UX.md` §126-137 says
        // identity *is* — "图标 × 目录", the icon and the text together — and in
        // this list the text is the title. Two rows with the same mark are fine;
        // two rows a reader cannot tell apart are not.
        for (index, left) in PROFILES.iter().enumerate() {
            for right in &PROFILES[index + 1..] {
                assert_ne!(
                    (left.mark, left.title),
                    (right.mark, right.title),
                    "{} and {} would be one row twice",
                    left.id,
                    right.id
                );
            }
        }
        assert_eq!(
            PROFILES[index_of_id("pwsh")].mark,
            PROFILES[index_of_id("winps")].mark,
            "and the two PowerShells share theirs on purpose"
        );

        // And five ids, because an id is what a seed is keyed on: two profiles
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
            // Both PowerShells: the flag belongs to the family, not to one row.
            let is_powershell = profile.id == "pwsh" || profile.id == "winps";
            assert_eq!(
                has_nologo,
                is_powershell,
                "{} must {} pass -NoLogo",
                profile.id,
                if is_powershell { "" } else { "not" }
            );
        }
        assert_eq!(PROFILES[index_of_id("cmd")].args, &[] as &[&str]);
        assert_eq!(
            PROFILES[index_of_id("gitbash")].args,
            &["--login", "-i"],
            "without --login this is a bash that cannot find git"
        );
    }

    /// PIN — the two PowerShells are two rows, two programs, and two answers.
    ///
    /// User ruling 2026-08-11, and it reverses this module's earlier reading. One
    /// row called `PowerShell` whose resolution order ended at `powershell.exe`
    /// was defensible while it was the *only* PowerShell row: which end of the
    /// chain a machine landed on was a fact about the machine. It stops being
    /// defensible on a machine that has both installed, which is the common case
    /// — the row silently starts one of two visibly different products, and the
    /// user has no way to ask for the other.
    ///
    /// Red gate, and each half catches a different way of half-doing it:
    ///
    /// * leave `pwsh` resolving through the old chain and a machine without
    ///   PowerShell 7 gets a row labelled `PowerShell` that starts 5.1 — the
    ///   original lie, now with a second row beside it making the lie visible;
    /// * point `FALLBACK_PROFILE` at `pwsh` and the floor under every other
    ///   profile becomes a row that is allowed to be greyed.
    #[test]
    fn the_two_powershells_are_two_rows_and_only_one_of_them_can_be_missing() {
        let (seven, five) = (index_of_id("pwsh"), index_of_id("winps"));
        assert_ne!(seven, five, "two rows, so two indices");
        // Each row says which one it is. The bare "PowerShell" / "Windows
        // PowerShell" pair these shipped with is what the user could not read
        // apart at a glance, and the version is the whole answer.
        assert_eq!(PROFILES[seven].title, "PowerShell 7");
        assert_eq!(PROFILES[five].title, "Windows PowerShell 5.1");
        for profile in [seven, five] {
            assert!(
                PROFILES[profile]
                    .title
                    .split_whitespace()
                    .any(|word| word.starts_with(|first: char| first.is_ascii_digit())),
                "{:?} names its version, which is the only thing telling it from \
                 the row beside it",
                PROFILES[profile].id
            );
        }

        // On a machine with both, each row starts its own binary — which is the
        // whole of what splitting them buys.
        let both = ProfilePrograms::probe(&FakeMachine::fully_equipped());
        assert_eq!(
            both.program(seven)
                .map(|p| p.to_string_lossy().into_owned()),
            Some(r"C:\Program Files\PowerShell\7\pwsh.exe".to_owned())
        );
        assert_eq!(
            both.program(five).map(|p| p.to_string_lossy().into_owned()),
            Some(r"C:\WINDOWS\System32\WindowsPowerShell\v1.0\powershell.exe".to_owned())
        );

        // On a machine with only what Windows ships, the 7 row says so rather
        // than starting 5.1 under 7's name, and the 5.1 row is still there.
        let plain = bare();
        assert!(!plain.is_available(seven), "no install, no row that works");
        assert!(plain.is_available(five), "and this one is part of the OS");
        assert_eq!(five, FALLBACK_PROFILE);

        // `BT_SHELL` still belongs to the 7 row (Q4) and still bypasses the
        // probe: it names a shell, and naming one that is not there leaves the
        // row greyed rather than silently ignored.
        let overridden = ProfilePrograms::probe(
            &FakeMachine::bare_windows().with_var("BT_SHELL", r"C:\Tools\pwsh.exe"),
        );
        assert_eq!(
            overridden
                .program(seven)
                .map(|p| p.to_string_lossy().into_owned()),
            Some(r"C:\Tools\pwsh.exe".to_owned())
        );
        assert_eq!(
            overridden
                .program(five)
                .map(|p| p.to_string_lossy().into_owned()),
            Some(r"C:\WINDOWS\System32\WindowsPowerShell\v1.0\powershell.exe".to_owned()),
            "and it does not reach across into the row it is not for"
        );
    }

    /// PIN — the integration script titles a session with its own profile's name,
    /// character for character.
    ///
    /// The two files are one decision wearing two syntaxes.
    /// `scripts/shell-integration/folio.ps1` ends by writing the running
    /// edition as an OSC 0 title; `pane_head_title` then *drops* a program title
    /// equal to the profile's own, because a shell that agrees with its launcher
    /// has announced nothing. That test is string equality and nothing weaker, so
    /// the moment one side is renamed alone every pane head in a PowerShell tab
    /// goes back to reading `PowerShell · D:\…` — the exact defect the suppression
    /// was written for, re-entered through the back door of a half-done rename.
    ///
    /// This reads the bytes that ship rather than a copy of them, which is the
    /// only version of this claim worth making: a constant restated here would
    /// agree with `profiles.rs` forever and with the script never.
    ///
    /// Red gate: rename either title in the table, or either literal in the
    /// script, and this fails naming the one that moved. It is what the
    /// 7 / 5.1 rename was carried out under.
    #[test]
    fn the_integration_script_names_the_profiles_own_titles() {
        let script = crate::shell_integration::script_source_ps1();
        // The assignment as the script writes it — the two arms of the one
        // conditional that decides what a PowerShell calls itself. `Core` is 7 and
        // everything else is the 5.1 that ships with Windows.
        for (id, edition) in [("pwsh", "Core"), ("winps", "Desktop")] {
            let title = PROFILES[index_of_id(id)].title;
            let quoted = format!("'{title}'");
            assert!(
                script.contains(&quoted),
                "{id}'s title {title:?} is what the script's {edition} arm writes; \
                 folio.ps1 does not contain {quoted}"
            );
        }
        // And the arms are told apart the way the script tells them apart, so the
        // pair above cannot both be satisfied by one arm carrying both strings.
        let seven = PROFILES[index_of_id("pwsh")].title;
        let five = PROFILES[index_of_id("winps")].title;
        assert!(
            script.contains(&format!(
                "$PSVersionTable.PSEdition -eq 'Core') {{ '{seven}' }} else {{ '{five}' }}"
            )),
            "the script picks {seven:?} for Core and {five:?} for every other \
             edition, in that order"
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
            PROFILES[FALLBACK_PROFILE].id, "winps",
            "the floor is the shell that is part of Windows"
        );
        assert!(
            !matches!(
                PROFILES[FALLBACK_PROFILE].program,
                ProgramSource::PowerShellSeven
            ),
            "and never the row that is allowed to answer `no` — a fallback chain              whose bottom can be greyed has a hole in it"
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
            &mut fake_measure,
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
            &mut fake_measure,
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
            &mut fake_measure,
        );

        let bare_tips: Vec<_> = layout.tips(&bare(), &vault).collect();
        let profiles_tipped: Vec<_> = bare_tips
            .iter()
            .filter_map(|(row, _, text)| match row {
                MenuRow::Profile(index) => Some((*index, text.clone())),
                // The files row's caption says everything it knows, so it is
                // never tipped — the same rule an available profile row follows.
                MenuRow::Recent(_) | MenuRow::FilesPane => None,
            })
            .collect();
        assert_eq!(
            profiles_tipped,
            vec![
                // PowerShell 7 is an install and this box has none of it; the
                // 5.1 row beside it is part of Windows and says nothing.
                (
                    index_of_id("pwsh"),
                    format!("{} — not found on this machine", powershell_seven())
                ),
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
                MenuRow::FilesPane => layout.files_pane,
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
                spawn_place(index_of_id(profile), None, &machine),
                SpawnPlace {
                    working_directory: Some(PathBuf::from(r"C:\Users\dev")),
                    arguments: Vec::new(),
                },
                "{profile} is a Windows process and takes a working directory"
            );
        }
        assert_eq!(
            spawn_place(index_of_id("wsl"), None, &machine),
            SpawnPlace {
                working_directory: None,
                arguments: vec![OsString::from("--cd"), OsString::from("~")],
            },
            "WSL's home has no Windows spelling, so it is asked for rather than handed over"
        );
        // The home is read from the variable and never composed, so a redirected
        // profile is followed rather than guessed at.
        assert_eq!(
            spawn_place(
                FALLBACK_PROFILE,
                None,
                &FakeMachine::default().with_var("USERPROFILE", r"\\server\redirected\dev")
            )
            .working_directory,
            Some(PathBuf::from(r"\\server\redirected\dev")),
        );
        assert_eq!(
            spawn_place(FALLBACK_PROFILE, None, &FakeMachine::default()),
            SpawnPlace::default(),
            "a machine that cannot name its own home is told nothing, not a guess"
        );
    }

    /// PIN — an inherited directory travels on the channel its own profile
    /// listens on, and never on the other one.
    ///
    /// Red gate, and it is the failure P4 would otherwise ship: hand a WSL leaf
    /// its inherited `/mnt/d/Developer` as a *working directory* and it reaches
    /// `CreateProcess` as a Windows path, where `bt-pty`'s own "does this
    /// directory exist" check rejects it and silently substitutes this process's
    /// folder — so the WSL tab opens in `/mnt/c/WINDOWS/system32` while the menu
    /// row said it would open where you were standing. Nothing about that is
    /// visible: it is a real directory, and the shell starts.
    #[test]
    fn an_inherited_directory_is_told_to_the_launcher_that_can_read_it() {
        let machine = FakeMachine::default().with_var("USERPROFILE", r"C:\Users\dev");
        assert_eq!(
            spawn_place(
                index_of_id("pwsh"),
                Some(PathBuf::from(r"D:\Developer")),
                &machine
            ),
            SpawnPlace {
                working_directory: Some(PathBuf::from(r"D:\Developer")),
                arguments: Vec::new(),
            },
            "a Windows process is simply started there"
        );
        assert_eq!(
            spawn_place(
                index_of_id("wsl"),
                Some(PathBuf::from("/mnt/d/Developer")),
                &machine
            ),
            SpawnPlace {
                working_directory: None,
                arguments: vec![OsString::from("--cd"), OsString::from("/mnt/d/Developer")],
            },
            "the launcher is told the place, in the namespace the shell reads"
        );
    }

    /// PIN — the drive map, in both directions, including every shape that has
    /// no answer.
    ///
    /// Red gate: a `to_string_lossy().replace('\\', "/")` translation passes the
    /// happy row and takes `\\server\share\src` to `/mnt/\/server/share/src`,
    /// `src` to `src`, and `/mnt/cdrom` to `C:` — three paths that name nothing,
    /// handed to a shell as the place it should open in. Every `None` row here
    /// is a path that a string-level translation would have accepted.
    #[test]
    fn the_drive_map_translates_what_it_can_and_refuses_the_rest() {
        for (windows, wsl) in [
            (r"C:\Users\Weiyi", "/mnt/c/Users/Weiyi"),
            (
                r"D:\Developer\BetterTerminal",
                "/mnt/d/Developer/BetterTerminal",
            ),
            // The letter is lower-cased going out and upper-cased coming back:
            // `/mnt/C` is not one of WSL's mounts, and `c:\` is not how Windows
            // writes a drive.
            (r"c:\src", "/mnt/c/src"),
            (r"C:\", "/mnt/c"),
            // A space needs no escaping in either spelling; it is a path, not a
            // command line.
            (r"D:\My Pictures", "/mnt/d/My Pictures"),
        ] {
            assert_eq!(
                windows_to_wsl(Path::new(windows)).as_deref(),
                Some(Path::new(wsl)),
                "{windows} → WSL"
            );
        }
        // Not symmetric as a pair of tables: the lower-cased drive and the
        // forward slashes are canonical going out, so coming back is checked on
        // its own inputs.
        for (wsl, windows) in [
            ("/mnt/c/Users/Weiyi", r"C:\Users\Weiyi"),
            ("/mnt/d/Developer", r"D:\Developer"),
            ("/mnt/c", r"C:\"),
            ("/mnt/c/", r"C:\"),
        ] {
            assert_eq!(
                wsl_to_windows(Path::new(wsl)).as_deref(),
                Some(Path::new(windows)),
                "{wsl} → Windows"
            );
        }
        for unnameable in [
            // The UNC shapes, including the one `wslpath -w` answers for a Linux
            // home — a share, not a mount.
            r"\\server\share\src",
            r"\\wsl.localhost\Ubuntu-24.04\home\weiyi",
            r"\\?\UNC\server\share",
            // Not rooted: nobody said where from.
            r"src\a",
            r"C:src",
            r"\rooted-but-driveless",
            "",
        ] {
            assert_eq!(
                windows_to_wsl(Path::new(unnameable)),
                None,
                "{unnameable:?} names no directory a WSL shell can stand in"
            );
        }
        // The verbatim spelling of a drive is still a drive, and only the
        // platform's own parser knows that — a `\\` prefix test reads it as a
        // share.
        assert_eq!(
            windows_to_wsl(Path::new(r"\\?\C:\src")).as_deref(),
            Some(Path::new("/mnt/c/src"))
        );
        for unnameable in [
            // The distribution's own root filesystem. Windows can only reach it
            // through the `\\wsl.localhost` share, which is a service and not a
            // directory — the ruling is that this has no answer.
            "/home/weiyi",
            "/",
            "/usr/local/bin",
            // A directory somebody made under `/mnt`, which is not a drive.
            "/mnt/cdrom/disc",
            "/mnt/9/x",
            "/mnt",
            "/mnt/",
            "relative/path",
        ] {
            assert_eq!(
                wsl_to_windows(Path::new(unnameable)),
                None,
                "{unnameable:?} has no Windows spelling"
            );
        }
    }

    /// PIN — every pair of profiles, and what a new tab inherits across it.
    ///
    /// This is P3's `a_new_tab_inherits_a_folder_only_from_its_own_profile`
    /// grown up: that test asserted `Some ⟺ from == to`, and it would now fail,
    /// which is the point. The rule it enforced was a stand-in for this one.
    ///
    /// Red gate: translating only on the diagonal (P3's rule) leaves a WSL tab
    /// opened from a PowerShell standing in `D:\Developer` at `~`, and the
    /// mock-up's own promise — "a new shell opens where the one you are looking
    /// at is standing" — quietly not kept for three quarters of the table.
    #[test]
    fn a_new_tab_inherits_a_folder_whenever_the_shell_it_starts_can_name_it() {
        let windows = Path::new(r"D:\Developer\BetterTerminal");
        let mounted = Path::new("/mnt/d/Developer/BetterTerminal");
        let inside = Path::new("/home/weiyi/src");
        for (source, profile) in PROFILES.iter().enumerate() {
            for (target, other) in PROFILES.iter().enumerate() {
                let (standing, expected) = match (profile.paths, other.paths) {
                    (PathNamespace::Windows, PathNamespace::Windows) => (windows, Some(windows)),
                    (PathNamespace::Windows, PathNamespace::Wsl) => (windows, Some(mounted)),
                    (PathNamespace::Wsl, PathNamespace::Windows) => (mounted, Some(windows)),
                    (PathNamespace::Wsl, PathNamespace::Wsl) => (mounted, Some(mounted)),
                };
                assert_eq!(
                    cwd_for_spawn(source, target, Some(standing)).as_deref(),
                    expected,
                    "{} standing in {} opens {}",
                    profile.id,
                    standing.display(),
                    other.id
                );
                // The one directory that cannot cross, from the one profile that
                // can be standing in it.
                if profile.paths == PathNamespace::Wsl {
                    assert_eq!(
                        cwd_for_spawn(source, target, Some(inside)).as_deref(),
                        (other.paths == PathNamespace::Wsl).then_some(inside),
                        "{} in a Linux home opens {}",
                        profile.id,
                        other.id
                    );
                }
                // A shell that has never said where it is hands on nothing to
                // say, whichever pair it is — the OSC 7 rule, unchanged.
                assert_eq!(cwd_for_spawn(source, target, None), None);
            }
        }
    }

    /// PIN — a saved directory is checked for existence only where the check
    /// can be answered.
    ///
    /// Red gate, and it is the silent kind: `is_dir()` is a Win32 question and
    /// `/mnt/d/Developer` answers **no** to it on the very machine where that
    /// directory is fine. An unguarded check therefore drops the directory of
    /// every WSL pane it restores — every revived WSL tab comes back at `~`,
    /// nothing is logged, and the session file that has the right answer in it
    /// is overwritten with the wrong one on the next save.
    #[test]
    fn a_saved_directory_is_only_checked_for_existence_where_that_is_answerable() {
        let real = std::env::temp_dir();
        let gone = real.join("betterterminal-no-such-directory-here");
        for id in ["pwsh", "gitbash", "cmd"] {
            let profile = index_of_id(id);
            assert_eq!(
                revived_cwd(profile, &real).as_deref(),
                Some(real.as_path()),
                "{id} comes back where it was"
            );
            assert_eq!(
                revived_cwd(profile, &gone),
                None,
                "{id} does not come back in a directory that is gone"
            );
        }
        let wsl = index_of_id("wsl");
        for inside in ["/home/weiyi/src", "/mnt/d/Developer"] {
            assert_eq!(
                revived_cwd(wsl, Path::new(inside)).as_deref(),
                Some(Path::new(inside)),
                "a WSL directory is taken at its word: Windows cannot see it to check"
            );
        }
    }

    /// PIN — the qualifier is appended to the constant, and only to the profile
    /// whose title is incomplete without it.
    ///
    /// Red gate: qualifying every profile gives `PowerShell · Ubuntu-24.04`, and
    /// qualifying none leaves the `˅` menu on a three-distribution machine
    /// unable to say which of them `WSL` opens.
    #[test]
    fn only_the_profile_that_names_a_launcher_wears_the_machine_s_answer() {
        for (index, profile) in PROFILES.iter().enumerate() {
            assert_eq!(
                compose_title(index, None),
                profile.title,
                "{} is its own title on a machine that answered nothing",
                profile.id
            );
            let qualified = compose_title(index, Some("Ubuntu-24.04"));
            match profile.qualifier {
                Qualifier::WslDistribution => {
                    assert_eq!(qualified, "WSL · Ubuntu-24.04");
                    // The mock-up's own rule (line 4013): a session's name is
                    // everything before `" ·"`, and it is the constant.
                    assert_eq!(qualified.split(" ·").next(), Some(profile.title));
                }
                Qualifier::None => assert_eq!(
                    qualified, profile.title,
                    "{} names a program, not a launcher",
                    profile.id
                ),
            }
        }
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
            &mut fake_measure,
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

        // The fallback profile is on the same machine and is untouched: greying
        // is per row, and the row that always works still looks like it works.
        //
        // Its sprite is taken by *row index* rather than by mark, because the
        // two PowerShells share a mark on purpose — a search would find whichever
        // came first and could not tell the greyed 7 row from the startable 5.1
        // one, which is exactly the pair this test has to distinguish.
        let winps = layer.sprites[FALLBACK_PROFILE];
        assert_eq!(winps.mark, ChromeMark::ProfilePowerShell);
        assert_eq!(winps.opacity, 1.0);
        assert!(!winps.grayscale);
        // …while PowerShell 7, one row above it, is greyed on this machine.
        let pwsh = layer.sprites[index_of_id("pwsh")];
        assert_eq!(pwsh.mark, ChromeMark::ProfilePowerShell);
        assert_eq!(pwsh.opacity, UNAVAILABLE_MARK_OPACITY);
        assert!(pwsh.grayscale);
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
                previews: Vec::new(),
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
            &mut fake_measure,
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
        let layout = layout(
            button,
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            NO_RECENT,
            &mut fake_measure,
        );
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
    ///
    /// The window is sized from the menu rather than written down, because the
    /// menu is content-sized and this pin is about **placement**: a button jammed
    /// against the right edge, and a popup that comes back inside for it. A fixed
    /// number here quietly turns into a second, different claim the moment a
    /// profile is renamed longer than it — which is how the 7 / 5.1 rename found
    /// it, the old 300px window no longer being wide enough to hold the list at
    /// all. That case has its own answer (`max(0.0)`: a menu with nowhere to fit
    /// hangs off the right, never off the left) and it is not this one.
    #[test]
    fn a_menu_opened_near_the_right_edge_stays_inside_the_window() {
        let scale = 1.0;
        let roomy = layout(
            [0.0, 9.0, 28.0, 37.0],
            MenuSide::Below,
            (4_000.0, 600.0),
            scale,
            NO_RECENT,
            &mut fake_measure,
        );
        let menu_width = roomy.frame[2] - roomy.frame[0];
        let surface = menu_width + 40.0;
        // The button in the top-right corner, which is where the `⌄` actually is.
        let anchor = [surface - 34.0, 9.0, surface - 6.0, 37.0];
        let layout = layout(
            anchor,
            MenuSide::Below,
            (surface, 600.0),
            scale,
            NO_RECENT,
            &mut fake_measure,
        );
        let frame = layout.frame;
        assert!(
            frame[2] < anchor[2],
            "this fixture depends on a button too near the edge for the menu to \
             hang straight down from it: frame {frame:?} anchor {anchor:?}"
        );
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
            &mut fake_measure,
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
                .any(|label| label.text == powershell_seven()
                    && label.color == palette.menu_item_text)
        );
        assert!(
            hover_labels
                .iter()
                .any(|label| label.text == powershell_seven()
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
                &mut fake_measure,
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
            &mut fake_measure,
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
            .find(|label| label.text == powershell_seven())
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
                &mut fake_measure,
            );
            let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
            assert_eq!(
                layout.frame[3] - layout.frame[1],
                (2.0 * (border + MENU_PADDING_LOGICAL_PX * scale)
                    + (ITEM_HEIGHT_LOGICAL_PX * scale).round() * PROFILES.len() as f32
                    // The `Files pane` section is unconditional, so it is here
                    // even with an empty vault — that asymmetry is the point of
                    // this pin, not an exception to it.
                    + files_block(scale))
                .round(),
                "scale {scale}: the profiles, the files row and the menu's own \
                 padding, and nothing else"
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
                PROFILES.len() + 1,
                "scale {scale}: one mark per profile row, plus the files row's folder"
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
                &mut fake_measure,
            );
            let full = layout(
                anchor(scale),
                MenuSide::Below,
                (960.0 * scale, 600.0),
                scale,
                &vault,
                &mut fake_measure,
            );
            assert_eq!(
                (full.frame[3] - full.frame[1]) - (empty.frame[3] - empty.frame[1]),
                recent_block(scale, vault.len()),
                "scale {scale}: the section's own three blocks and nothing more"
            );

            let rule = full.separator.expect("a filled vault is separated");
            let band = full.section_label.expect("and titled");
            let last_profile = *full.items.last().expect("the profile list");
            // The Recent rule now follows the `Files pane` row rather than the
            // last profile: the menu has three sections, and this one is third.
            assert_eq!(
                rule[1] - full.files_pane[3],
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
            &mut fake_measure,
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
            &mut fake_measure,
        );
        assert_eq!(RECENT_CAPACITY, 8, "the vault's own cap, not a second one");
        assert_eq!(layout.recent.len(), RECENT_CAPACITY);
        assert_eq!(
            layout.frame[3] - layout.frame[1],
            (2.0 * ((FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0)
                + MENU_PADDING_LOGICAL_PX * scale)
                + (ITEM_HEIGHT_LOGICAL_PX * scale).round() * PROFILES.len() as f32
                + files_block(scale)
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
            PROFILES.len() + 1 + RECENT_CAPACITY,
            "one mark per drawn row, the files row included"
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
            &mut fake_measure,
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
        let layout = layout(
            anchor(1.0),
            MenuSide::Below,
            (960.0, 600.0),
            1.0,
            &vault,
            &mut fake_measure,
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
            &mut fake_measure,
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
            &mut fake_measure,
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
                .any(|label| label.text == powershell_seven()
                    && label.color == palette.menu_item_text),
            "while the profile row it is not stays `--ink2`"
        );
    }

    // ── K143-K145: the file row's context menu ─────────────────────────────

    /// PIN — the menu is the three verbs `DESIGN.md` §7.1.3 names, in that
    /// order, with the rule between the row that *opens the file* and the two
    /// that *do something with its path*.
    ///
    /// Order is asserted from the drawn labels rather than from the enum,
    /// because the enum's order is only a promise until something reads it in
    /// that order: a painter that walked `items` backwards would still satisfy
    /// a test that only counted rows.
    #[test]
    fn the_file_menu_draws_three_verbs_with_a_rule_under_the_first() {
        let layout = file_menu_layout(
            [300.0, 200.0],
            (960.0, 600.0),
            1.0,
            "Open preview",
            &mut fake_measure,
        );
        let layer = one_layer(file_menu_build(&layout, "Open preview", None));
        let names: Vec<&str> = layer
            .labels
            .iter()
            .map(|label| label.text.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["Open preview", COPY_PATH_TEXT, INSERT_PATH_TEXT],
            "three rows, top to bottom, and no heading over them"
        );
        assert!(
            layout.separator[1] >= layout.items[0][3] && layout.separator[3] <= layout.items[1][1],
            "the rule lies between the first row and the second"
        );
        assert_eq!(
            layer
                .sprites
                .iter()
                .map(|sprite| sprite.mark)
                .collect::<Vec<_>>(),
            vec![ChromeMark::File, ChromeMark::Copy, ChromeMark::Paste],
            "each verb wears its own glyph — the copy and the paste are not one mark twice"
        );
    }

    // ── the `⌄` open policy (user ruling, 2026-08-16) ───────────────────────

    /// PIN — **a rest of 250ms opens a `⌄`, and 249 does not.**
    ///
    /// The whole of the ruling's first half, stated as a boundary rather than as
    /// "after a while": a hover menu whose threshold drifts is a hover menu that
    /// drops itself in front of a pointer merely passing by, which is the reason
    /// every menu in this build was click-only until now.
    ///
    /// Red gate: start the clock on the frame the pointer arrives *and* fire on
    /// `>` rather than `>=` and the menu opens a frame late for ever; clear
    /// `resting_since` on a repeated `Button` observation and it never opens at
    /// all, because a hand is never perfectly still.
    #[test]
    fn a_chevron_opens_after_a_rest_of_exactly_the_ruled_delay_and_not_before() {
        assert_eq!(CHEVRON_HOVER_OPEN, Duration::from_millis(250));
        let start = Instant::now();
        let mut gate = ChevronGate::default();
        gate.observe(ChevronPointer::Button, false, start);
        assert_eq!(gate.due(start), None);
        assert_eq!(gate.due(start + Duration::from_millis(249)), None);
        assert_eq!(
            gate.due(start + CHEVRON_HOVER_OPEN),
            Some(ChevronAction::Open)
        );
        assert_eq!(
            gate.deadline(),
            Some(start + CHEVRON_HOVER_OPEN),
            "and the loop is told exactly when to wake for it"
        );

        // A hand that moves two pixels inside the button has not left it, and
        // must not restart the clock.
        let mut jittering = ChevronGate::default();
        for step in 0..5 {
            jittering.observe(
                ChevronPointer::Button,
                false,
                start + Duration::from_millis(step * 60),
            );
        }
        assert_eq!(
            jittering.due(start + CHEVRON_HOVER_OPEN),
            Some(ChevronAction::Open),
            "the rest accumulates across moves inside the button"
        );
    }

    /// PIN — **leaving both the button and the menu closes it after the grace,
    /// and being on the menu is not leaving.**
    ///
    /// The middle state is the point. The button and its menu are two rectangles
    /// with `MENU_OFFSET_LOGICAL_PX` of window between them, so a hand travelling
    /// from one to the other is briefly on neither — which the grace covers — and
    /// then on the menu, which must stop the grace outright rather than merely
    /// re-arm it.
    ///
    /// Red gate: treat `Surface` as `Away` and the menu closes 150ms after the
    /// pointer reaches its first row.
    #[test]
    fn a_chevron_menu_closes_after_the_leave_grace_and_the_menu_itself_is_not_away() {
        assert_eq!(CHEVRON_LEAVE_GRACE, Duration::from_millis(150));
        let start = Instant::now();
        let mut gate = ChevronGate::default();
        gate.observe(ChevronPointer::Away, true, start);
        assert_eq!(gate.due(start + Duration::from_millis(149)), None);
        assert_eq!(
            gate.due(start + CHEVRON_LEAVE_GRACE),
            Some(ChevronAction::Close)
        );

        // Crossing the gap and landing on the menu cancels the close outright.
        let mut crossing = ChevronGate::default();
        crossing.observe(ChevronPointer::Away, true, start);
        crossing.observe(
            ChevronPointer::Surface,
            true,
            start + Duration::from_millis(40),
        );
        assert_eq!(crossing.deadline(), None, "no clock is left running");
        assert_eq!(crossing.due(start + Duration::from_secs(10)), None);

        // And a pointer on the button of a menu that is already up owes nothing
        // in either direction — the click is what toggles it from there.
        let mut home = ChevronGate::default();
        home.observe(ChevronPointer::Button, true, start);
        assert_eq!(home.due(start + Duration::from_secs(10)), None);

        // Nothing at all is owed by a window whose pointer is nowhere near a
        // chevron, which is what keeps the loop asleep.
        let mut idle = ChevronGate::default();
        idle.observe(ChevronPointer::Away, false, start);
        assert_eq!(idle.deadline(), None);
    }

    /// PIN (the ruling's own point) — **both chevrons are one policy**: the same
    /// type, the same two constants, and the same answers to the same steps.
    ///
    /// The ruling is "和 tab 那边语义对齐", and the failure it guards against is
    /// not a wrong number — it is two implementations that agree today. Driving
    /// two independently-constructed gates through one script and demanding
    /// identical answers is the strongest statement this level can make; the
    /// window's own half (that both buttons actually *go through* a gate) is
    /// `both_chevrons_are_driven_by_one_policy_and_one_pair_of_constants` in
    /// `main.rs`.
    #[test]
    fn the_two_chevrons_answer_one_policy_with_one_pair_of_constants() {
        let start = Instant::now();
        let script = [
            (ChevronPointer::Button, false, 0),
            (ChevronPointer::Button, false, 100),
            (ChevronPointer::Away, false, 300),
            (ChevronPointer::Button, false, 320),
            (ChevronPointer::Surface, true, 600),
            (ChevronPointer::Away, true, 700),
        ];
        let mut strip = ChevronGate::default();
        let mut head = ChevronGate::default();
        for (pointer, open, at) in script {
            let now = start + Duration::from_millis(at);
            strip.observe(pointer, open, now);
            head.observe(pointer, open, now);
            assert_eq!(strip, head, "the two gates never diverge at {at}ms");
            assert_eq!(strip.due(now), head.due(now));
            assert_eq!(strip.deadline(), head.deadline());
        }
        assert_eq!(
            head.deadline(),
            Some(start + Duration::from_millis(700) + CHEVRON_LEAVE_GRACE),
        );
    }

    // ── the pane head's own menu ────────────────────────────────────────────

    fn pane_menu(submenu_open: bool) -> PaneMenuLayout {
        pane_menu_layout(
            [300.0, 120.0],
            (960.0, 600.0),
            1.0,
            submenu_open,
            &mut fake_measure,
        )
    }

    /// PIN (user ruling, 2026-08-16): **the menu is a picker and five verbs,
    /// with a rule above the one that destroys.**
    ///
    /// The order is the ruling's own, and it is an order of *commitment*: point
    /// at a direction, name a profile, name a folder, repeat this pane, move this
    /// pane, end this pane. The separator's position is the claim about reading —
    /// five verbs that make or move a pane, then one that ends one, because a
    /// destructive verb flush against constructive ones is a verb the hand finds
    /// by overshooting.
    ///
    /// Red gate: put the rule under the picker and the menu claims the four verbs
    /// below it are a different kind of thing from the diagram above; drop the
    /// picker out of `ALL` and the keyboard walk silently starts at `Split with`.
    #[test]
    fn the_pane_menu_is_a_picker_and_five_verbs_with_a_rule_above_the_close() {
        assert_eq!(
            PaneMenuRow::ALL,
            [
                PaneMenuRow::Picker,
                PaneMenuRow::SplitWith,
                PaneMenuRow::NewInFolder,
                PaneMenuRow::Duplicate,
                PaneMenuRow::MoveToNewTab,
                PaneMenuRow::ClosePane,
            ]
        );
        let layout = pane_menu(false);
        let layer = one_layer(pane_menu_build(
            &layout,
            None,
            None,
            &equipped(),
            &mut fake_measure,
        ));
        let names: Vec<&str> = layer
            .labels
            .iter()
            .map(|label| label.text.as_str())
            .collect();
        assert_eq!(
            names,
            vec![
                PICKER_CAPTION_TEXT,
                SPLIT_WITH_TEXT,
                NEW_IN_FOLDER_TEXT,
                DUPLICATE_PANE_TEXT,
                MOVE_TO_NEW_TAB_TEXT,
                CLOSE_PANE_TEXT,
            ],
            "the caption under the diagram, then five rows, and no heading over them"
        );
        let close = layout.item(PaneMenuRow::ClosePane);
        let above = layout.item(PaneMenuRow::MoveToNewTab);
        assert!(
            layout.separator[1] >= above[3] && layout.separator[3] <= close[1],
            "the rule lies between the last constructive verb and `Close pane`"
        );
        assert!(
            layout.item(PaneMenuRow::Picker)[3] <= layout.item(PaneMenuRow::SplitWith)[1],
            "and the picker is the first entry, above every word"
        );
    }

    /// PIN — **the picker is a pane with four zones around it, and the four are
    /// disjoint and reach the pane's own edges.**
    ///
    /// The reach is the claim that matters. A slab is ten logical pixels thick
    /// with three of air between it and the pane, and a hit test that answered
    /// only the drawn slab would leave a three-pixel dead band around a control
    /// that is already the smallest thing in the menu — so each slab's press area
    /// crosses its own gap, and no two of them can claim the same point.
    ///
    /// Red gate: hit-test the drawn rectangles and the gap answers `Surface`,
    /// which lights nothing and does nothing; grow both neighbours across one
    /// gap and two zones claim the same pixel.
    #[test]
    fn the_pickers_four_zones_are_disjoint_and_leave_no_dead_band_at_the_pane() {
        let layout = pane_menu(false);
        let pane = layout.picker_pane;
        for zone in SplitZone::ALL {
            let drawn = layout.zone(zone);
            assert!(drawn[2] > drawn[0] && drawn[3] > drawn[1]);
            let mid = (
                f64::from((drawn[0] + drawn[2]) / 2.0),
                f64::from((drawn[1] + drawn[3]) / 2.0),
            );
            assert_eq!(
                pane_menu_hit(&layout, mid.0, mid.1),
                Some(PaneMenuHit::Zone(zone)),
                "the middle of {zone:?}'s slab is {zone:?}"
            );
        }
        // The air between the pane and each slab belongs to that slab.
        let just_outside = [
            (
                f64::from(pane[2]) + 1.0,
                f64::from((pane[1] + pane[3]) / 2.0),
            ),
            (
                f64::from((pane[0] + pane[2]) / 2.0),
                f64::from(pane[3]) + 1.0,
            ),
            (
                f64::from(pane[0]) - 1.0,
                f64::from((pane[1] + pane[3]) / 2.0),
            ),
            (
                f64::from((pane[0] + pane[2]) / 2.0),
                f64::from(pane[1]) - 1.0,
            ),
        ];
        for (zone, (x, y)) in SplitZone::ALL.iter().zip(just_outside) {
            assert_eq!(
                pane_menu_hit(&layout, x, y),
                Some(PaneMenuHit::Zone(*zone)),
                "the gap beside the pane is {zone:?}'s, not a dead band"
            );
        }
        // And the pane itself is not a zone: it stands for what is already there.
        assert_eq!(
            pane_menu_hit(
                &layout,
                f64::from((pane[0] + pane[2]) / 2.0),
                f64::from((pane[1] + pane[3]) / 2.0),
            ),
            Some(PaneMenuHit::Surface),
        );
    }

    /// PIN — **`Left` and `Up` are the same two axes with the arriving pane
    /// first**, which is `Edit::SplitSeat`'s own `leading` flag.
    ///
    /// Stated here rather than only in the window, because it is the whole of
    /// what those two zones *are*: the layout crate has carried the flag since
    /// the tree existed, so no new tree edit was owed and the only thing that
    /// could go wrong is this mapping.
    #[test]
    fn the_pickers_left_and_up_zones_put_the_arriving_pane_first() {
        assert_eq!(SplitZone::Right.axis(), Axis::Row);
        assert_eq!(SplitZone::Left.axis(), Axis::Row);
        assert_eq!(SplitZone::Down.axis(), Axis::Col);
        assert_eq!(SplitZone::Up.axis(), Axis::Col);
        assert!(!SplitZone::Right.leading());
        assert!(!SplitZone::Down.leading());
        assert!(
            SplitZone::Left.leading(),
            "a shell on the left is a shell inserted before the one it came from"
        );
        assert!(SplitZone::Up.leading());
    }

    /// PIN — **the picker is a compass inside a list**: arrows aim the zones,
    /// and the list is left by aiming twice in the same direction.
    ///
    /// Both halves are load-bearing. Without the compass the four zones are
    /// unreachable from the keyboard, and the ruling's whole point is that a
    /// direction should be *pointed at*. Without the walk-out the picker is a
    /// trap: `↓` would aim for ever and the five verbs below would be
    /// unreachable.
    ///
    /// Red gate: leave the picker on the first `↓` and the `Down` zone can never
    /// be aimed at; never leave it and the rest of the menu is keyboard-dead.
    #[test]
    fn the_pane_menus_keyboard_walk_aims_the_picker_and_then_leaves_it() {
        use PaneMenuHover as H;
        let rows = PROFILES.len();
        // The list is entered at whichever end the key names.
        assert_eq!(
            H::step(None, MenuStep::Down, rows),
            Some(H::Zone(SplitZone::Right))
        );
        assert_eq!(
            H::step(None, MenuStep::Up, rows),
            Some(H::Row(PaneMenuRow::ClosePane))
        );
        // The compass: an arrow that names a zone other than the lit one aims
        // at it, from wherever the highlight happens to be.
        for (from, step, zone) in [
            (SplitZone::Right, MenuStep::Left, SplitZone::Left),
            (SplitZone::Right, MenuStep::Up, SplitZone::Up),
            (SplitZone::Right, MenuStep::Down, SplitZone::Down),
            (SplitZone::Left, MenuStep::Right, SplitZone::Right),
            (SplitZone::Up, MenuStep::Down, SplitZone::Down),
            (SplitZone::Down, MenuStep::Up, SplitZone::Up),
        ] {
            assert_eq!(
                H::step(Some(H::Zone(from)), step, rows),
                Some(H::Zone(zone)),
                "{step:?} from {from:?} aims at {zone:?}"
            );
        }
        // Aiming where the highlight already points is not a movement: it is
        // the request to leave, and sideways there is nowhere to go.
        assert_eq!(
            H::step(Some(H::Zone(SplitZone::Right)), MenuStep::Right, rows),
            None
        );
        assert_eq!(
            H::step(Some(H::Zone(SplitZone::Left)), MenuStep::Left, rows),
            None
        );
        // Aiming twice in the same direction walks out — downward only, because
        // the picker is the first entry and the top of a list clamps.
        assert_eq!(
            H::step(Some(H::Zone(SplitZone::Down)), MenuStep::Down, rows),
            Some(H::Row(PaneMenuRow::SplitWith))
        );
        assert_eq!(
            H::step(Some(H::Zone(SplitZone::Up)), MenuStep::Up, rows),
            None,
            "and the top of the list clamps, as every walk in this window does"
        );
        // Back in from below, landing on the zone nearest the row it came from.
        assert_eq!(
            H::step(Some(H::Row(PaneMenuRow::SplitWith)), MenuStep::Up, rows),
            Some(H::Zone(SplitZone::Down))
        );
        // The flat part of the walk clamps at the bottom.
        assert_eq!(
            H::step(Some(H::Row(PaneMenuRow::ClosePane)), MenuStep::Down, rows),
            Some(H::Row(PaneMenuRow::ClosePane))
        );
        // And the submenu has a walk of its own, clamped at both ends.
        assert_eq!(
            H::step(Some(H::Submenu(0)), MenuStep::Up, rows),
            Some(H::Submenu(0))
        );
        assert_eq!(
            H::step(Some(H::Submenu(0)), MenuStep::Down, rows),
            Some(H::Submenu(1))
        );
        assert_eq!(
            H::step(Some(H::Submenu(rows - 1)), MenuStep::Down, rows),
            Some(H::Submenu(rows - 1))
        );
    }

    /// PIN — a press on each entry is answered as that entry, the menu's own
    /// padding swallows, and a point outside is nobody's.
    ///
    /// Red gate: hit-test the frame before the rows and every press anywhere in
    /// the menu becomes `Surface` — a menu that lights up and does nothing.
    #[test]
    fn the_pane_menu_answers_a_press_on_each_of_its_verbs() {
        let layout = pane_menu(false);
        for row in PaneMenuRow::TEXT_ROWS {
            let rect = layout.item(row);
            let (x, y) = (
                f64::from((rect[0] + rect[2]) / 2.0),
                f64::from((rect[1] + rect[3]) / 2.0),
            );
            assert_eq!(pane_menu_hit(&layout, x, y), Some(PaneMenuHit::Row(row)));
        }
        assert_eq!(
            pane_menu_hit(
                &layout,
                f64::from(layout.frame[0] + 2.0),
                f64::from(layout.frame[1] + 1.0)
            ),
            Some(PaneMenuHit::Surface),
            "the border and padding swallow rather than fall through"
        );
        assert_eq!(
            pane_menu_hit(
                &layout,
                f64::from(layout.frame[0]) - 1.0,
                f64::from(layout.frame[1]) - 1.0
            ),
            None,
            "and a point outside is the window's again"
        );
    }

    /// PIN — the pane menu clamps on both axes, [`file_menu_layout`]'s rule.
    ///
    /// A pane head can be the bottom head of a tall stack and the right-hand
    /// head of a wide one, so both edges are reachable. An unclamped drop puts
    /// every verb under the window's own edge, where the menu is visible and
    /// unusable.
    #[test]
    fn the_pane_menu_raised_in_the_corner_is_pulled_back_inside_on_both_axes() {
        let surface = (960.0, 600.0);
        let layout = pane_menu_layout([950.0, 596.0], surface, 1.0, false, &mut fake_measure);
        assert!(
            layout.frame[2] <= surface.0 && layout.frame[3] <= surface.1,
            "the whole menu is inside the window: {:?}",
            layout.frame
        );
        assert!(layout.frame[0] >= 0.0 && layout.frame[1] >= 0.0);
        for item in layout.items {
            assert!(
                item[3] <= surface.1,
                "and every row it drew is on screen: {item:?}"
            );
        }
    }

    /// PIN — the picker's block is the sum of what it draws, and it lands on the
    /// ruling's ~92px.
    ///
    /// The point of asserting the *derivation* rather than the number is that the
    /// number is a consequence: change the slab thickness and this test says so,
    /// where a hard-coded 92 would go on passing while the diagram overflowed its
    /// own block.
    #[test]
    fn the_pickers_block_is_the_air_the_diagram_and_the_caption_added_up() {
        let block = picker_block_logical_px();
        assert_eq!(
            block,
            PICKER_PADDING_TOP_LOGICAL_PX
                + picker_diagram_height_logical_px()
                + SECTION_LABEL_PADDING_TOP_LOGICAL_PX
                + SECTION_LABEL_LINE_LOGICAL_PX
                + SECTION_LABEL_PADDING_BOTTOM_LOGICAL_PX
                + PICKER_PADDING_BOTTOM_LOGICAL_PX
        );
        assert_eq!(
            block.round(),
            92.0,
            "the ruling asks for a row about 92px tall"
        );
        assert_eq!(picker_diagram_height_logical_px(), 60.0);
        assert_eq!(picker_diagram_width_logical_px(), 74.0);
    }

    /// PIN — **a hovered zone is washed in the accent and outlined in it**, and
    /// nothing else in the diagram moves.
    ///
    /// The wash is what says "the new pane lands here"; the outline is what says
    /// "here". A hovered zone that only changed its outline would be invisible at
    /// ten pixels, and one that filled solid would read as a pane that already
    /// exists.
    #[test]
    fn a_hovered_zone_takes_the_accent_as_a_wash_and_as_its_outline() {
        let layout = pane_menu(false);
        let palette = chrome_palette();
        let lit = one_layer(pane_menu_build(
            &layout,
            Some(PaneMenuHover::Zone(SplitZone::Down)),
            None,
            &equipped(),
            &mut fake_measure,
        ));
        let zone = layout.zone(SplitZone::Down);
        let inside = |quad: &OverlayQuad| {
            quad.rect[0] >= zone[0] - 0.5
                && quad.rect[2] <= zone[2] + 0.5
                && quad.rect[1] >= zone[1] - 0.5
                && quad.rect[3] <= zone[3] + 0.5
        };
        let accents: Vec<f32> = lit
            .quads
            .iter()
            .filter(|quad| inside(quad) && quad.color == palette.accent)
            .map(|quad| quad.alpha)
            .collect();
        assert!(
            accents.iter().any(|alpha| *alpha > PICKER_ZONE_WASH_ALPHA),
            "the outline is the accent at full strength: {accents:?}"
        );
        assert!(
            accents
                .iter()
                .any(|alpha| (*alpha - PICKER_ZONE_WASH_ALPHA).abs() < 0.001),
            "and the face is the accent at 15%: {accents:?}"
        );
        // The other three keep the menu's own hairline.
        let dark = one_layer(pane_menu_build(
            &layout,
            None,
            None,
            &equipped(),
            &mut fake_measure,
        ));
        assert!(
            !dark
                .quads
                .iter()
                .any(|quad| inside(quad) && quad.color == palette.accent),
            "an unhovered zone carries no accent at all"
        );
    }

    // ── the submenu and the safety triangle (queue item #53) ────────────────

    /// PIN — **the submenu is the profile list, hung beside its heading, and the
    /// pane's own profile is marked.**
    ///
    /// It is the *same* list the tab strip's `⌄` offers, because the ruling asks
    /// for "profiles 二级菜单" and a second list would be a second answer to
    /// "which shells does this product offer". A profile this machine cannot
    /// start is greyed rather than dropped, on `hit`'s own precedent.
    #[test]
    fn the_split_with_submenu_is_the_profile_list_with_this_panes_own_marked() {
        let layout = pane_menu(true);
        let frame = layout
            .submenu_frame()
            .expect("an open submenu has a frame of its own");
        let heading = layout.item(PaneMenuRow::SplitWith);
        assert!(
            frame[0] >= heading[0],
            "it hangs beside its heading, not under it"
        );
        assert!(frame[1] <= heading[1] && frame[3] > heading[1]);

        let layers = pane_menu_build(&layout, None, Some(1), &equipped(), &mut fake_measure);
        let [_parent, child]: [OverlayLayer; 2] = layers
            .try_into()
            .expect("a menu with a child draws two layers");
        let names: Vec<&str> = child
            .labels
            .iter()
            .map(|label| label.text.as_str())
            .filter(|text| *text != CURRENT_PROFILE_HINT_TEXT)
            .collect();
        assert_eq!(
            names,
            (0..PROFILES.len()).map(title).collect::<Vec<_>>(),
            "every profile the strip offers, in the strip's own order"
        );
        assert_eq!(
            child
                .labels
                .iter()
                .filter(|label| label.text == CURRENT_PROFILE_HINT_TEXT)
                .count(),
            1,
            "and exactly one row says it is the one this pane is running"
        );
        assert_eq!(
            child
                .sprites
                .iter()
                .map(|sprite| sprite.mark)
                .collect::<Vec<_>>(),
            PROFILES
                .iter()
                .map(|profile| profile.mark)
                .collect::<Vec<_>>(),
            "each row wears its own profile's mark"
        );

        // A press on a submenu row is answered as that profile, and the child
        // wins the pixels where the two frames overlap.
        for index in 0..PROFILES.len() {
            let rect = layout.submenu.as_ref().unwrap().items[index];
            assert_eq!(
                pane_menu_hit(
                    &layout,
                    f64::from((rect[0] + rect[2]) / 2.0),
                    f64::from((rect[1] + rect[3]) / 2.0),
                ),
                Some(PaneMenuHit::Submenu(index)),
            );
        }
    }

    /// PIN — a machine with no Git Bash greys that row rather than hiding it,
    /// and the mark greys with the word.
    #[test]
    fn a_submenu_row_this_machine_cannot_start_is_drawn_and_not_offered() {
        let layout = pane_menu(true);
        let layers = pane_menu_build(&layout, None, Some(0), &bare(), &mut fake_measure);
        let child = layers.last().expect("the child layer");
        assert_eq!(
            child
                .labels
                .iter()
                .filter(|l| l.text != CURRENT_PROFILE_HINT_TEXT)
                .count(),
            PROFILES.len(),
            "no row is dropped — a missing row looks like a row nobody designed"
        );
        assert!(
            child.sprites.iter().any(|sprite| sprite.grayscale),
            "and the ones this machine cannot start are greyed"
        );
    }

    /// PIN (**queue item #53, closed**) — the safety triangle holds a pointer
    /// travelling toward the submenu and releases one going anywhere else.
    ///
    /// The three claims are the whole rule:
    ///
    /// * a move that lands inside the wedge between the pointer's last position
    ///   and the submenu's near edge is a move *toward* the submenu, whatever row
    ///   it happens to be over;
    /// * a move that lands outside it is not, however close to the submenu it
    ///   ends up;
    /// * and the near edge is chosen by which side the submenu is on, so a child
    ///   that had to flip left because the window's edge was close is protected
    ///   the same way.
    ///
    /// Red gate: take the apex from the *current* position instead of the last
    /// one and the triangle is a point that contains nothing, so every diagonal
    /// steals the highlight — which is the behaviour this closes.
    #[test]
    fn the_safe_triangle_holds_a_pointer_aimed_at_the_submenu_and_releases_one_that_is_not() {
        // A submenu standing to the right, its near edge from y=100 to y=200.
        let submenu = [200.0, 100.0, 380.0, 200.0];
        let from = [100.0, 110.0];
        assert!(
            safe_triangle_holds(from, [150.0, 140.0], submenu),
            "halfway to the middle of the near edge, across whatever rows lie there"
        );
        assert!(
            safe_triangle_holds(from, [199.0, 199.0], submenu),
            "and a shallow aim at the far bottom corner is still an aim"
        );
        assert!(
            !safe_triangle_holds(from, [150.0, 260.0], submenu),
            "straight down, past the submenu's bottom, is a move at another row"
        );
        assert!(
            !safe_triangle_holds(from, [120.0, 90.0], submenu),
            "and up and away from it is not an aim at it either"
        );
        assert!(
            !safe_triangle_holds(from, [150.0, 140.0], [200.0, 100.0, 200.0, 100.0]),
            "a submenu with no area cannot be aimed at"
        );

        // The same wedge with the child flipped to the left of the hand.
        let flipped = [20.0, 100.0, 200.0, 200.0];
        let hand = [300.0, 110.0];
        assert!(safe_triangle_holds(hand, [250.0, 140.0], flipped));
        assert!(!safe_triangle_holds(hand, [250.0, 260.0], flipped));

        assert_eq!(SUBMENU_SAFE_HOLD, Duration::from_millis(300));
    }

    // ── the `×` in a menu row (user ruling, 2026-08-16) ─────────────────────

    /// PIN — **every `×` in a menu row is struck in a 10px box, and every other
    /// mark keeps its 15**, in whichever menu it appears.
    ///
    /// A deliberate deviation from the mock-up, whose `#i-close` runs edge to
    /// edge of its own `viewBox`: struck at the column's full fifteen the cross
    /// out-weighs the folder and the copy glyph beside it, which are shapes
    /// drawn inside a box with margins of their own.
    ///
    /// Red gate: apply the rule to `TabClose` alone and a menu that reaches for
    /// `PaneClose` — the same drawing under another name — gets the heavy cross
    /// back with nothing to say so.
    #[test]
    fn a_menu_rows_close_glyph_is_struck_ten_wide_and_every_other_mark_fifteen() {
        assert_eq!(ITEM_MARK_CLOSE_LOGICAL_PX, 10.0);
        for close in [
            ChromeMark::WindowClose,
            ChromeMark::TabClose,
            ChromeMark::PaneClose,
        ] {
            assert_eq!(item_mark_logical_px(close), ITEM_MARK_CLOSE_LOGICAL_PX);
        }
        for other in [
            ChromeMark::Folder,
            ChromeMark::Copy,
            ChromeMark::Split,
            ChromeMark::SplitRight,
            ChromeMark::SplitDown,
            ChromeMark::Float,
        ] {
            assert_eq!(
                item_mark_logical_px(other),
                ITEM_MARK_LOGICAL_PX,
                "{other:?} keeps the column's own size"
            );
        }

        // And it is true of the drawing, not merely of the table: the `×` in the
        // pane menu measures ten and stays centred in the same column the
        // fifteen-pixel marks are centred in.
        let layout = pane_menu(false);
        let layer = one_layer(pane_menu_build(
            &layout,
            None,
            None,
            &equipped(),
            &mut fake_measure,
        ));
        let cross = layer
            .sprites
            .iter()
            .find(|sprite| sprite.mark == ChromeMark::TabClose)
            .expect("`Close pane` wears a cross");
        let folder = layer
            .sprites
            .iter()
            .find(|sprite| sprite.mark == ChromeMark::Folder)
            .expect("`New terminal in folder…` wears a folder");
        assert_eq!(cross.rect[2] - cross.rect[0], 10.0);
        assert_eq!(folder.rect[2] - folder.rect[0], 15.0);
        // Within half a pixel of each other, which is as centred as two boxes of
        // different parity can be: both are snapped to whole device pixels — a
        // mark on a subpixel is a resampled mark — and 10 and 15 cannot both
        // land symmetrically on the same column. Half a pixel is the rounding,
        // not a drift.
        let centre = |rect: [f32; 4]| (rect[0] + rect[2]) / 2.0;
        assert!(
            (centre(cross.rect) - centre(folder.rect)).abs() <= 0.5,
            "both are centred in the one 14px column, so the names line up:              {} against {}",
            centre(cross.rect),
            centre(folder.rect),
        );
    }

    // ── the commit graph's branch filter (T2/T3, v2 ③) ─────────────────────

    /// T2 — the menu is a radio, the repository's own branches, a divider and two
    /// checkboxes, in that order; and a press on each of them is answered as that
    /// row.
    #[test]
    fn the_filter_menu_lists_the_branches_between_its_two_fixed_ends() {
        let branches = vec!["main".to_owned(), "side".to_owned()];
        let rows = git_filter_rows(&branches);
        assert_eq!(
            rows,
            vec![
                GitFilterRow::All,
                GitFilterRow::Branch("main".to_owned()),
                GitFilterRow::Branch("side".to_owned()),
                GitFilterRow::Remotes,
                GitFilterRow::Tags,
            ],
        );
        assert_eq!(git_filter_text(&GitFilterRow::All), "All branches");
        assert_eq!(
            git_filter_text(&GitFilterRow::Remotes),
            GIT_FILTER_REMOTES_TEXT
        );

        let anchor = [300.0, 40.0, 420.0, 62.0];
        let layout =
            git_filter_menu_layout(anchor, (960.0, 600.0), 1.0, rows.clone(), &mut fake_measure);
        assert_eq!(layout.rows(), rows.as_slice());
        // It hangs from the button's own bottom edge, not from a pointer.
        assert_eq!(layout.frame[1], anchor[3]);
        for (row, rect) in layout.rows().iter().zip(&layout.items) {
            let (x, y) = (
                f64::from((rect[0] + rect[2]) / 2.0),
                f64::from((rect[1] + rect[3]) / 2.0),
            );
            assert_eq!(git_filter_menu_hit(&layout, x, y), Some(Some(row.clone())));
        }
        assert_eq!(
            git_filter_menu_hit(
                &layout,
                f64::from(layout.frame[0] + 2.0),
                f64::from(layout.frame[1] + 1.0)
            ),
            Some(None),
            "the border and padding swallow rather than fall through"
        );
        assert_eq!(
            git_filter_menu_hit(
                &layout,
                f64::from(layout.frame[0]) - 1.0,
                f64::from(layout.frame[1]) - 1.0
            ),
            None
        );
        // The divider stands between the branches and the two flags — where the
        // menu stops being about *which history* and starts being about *which
        // names*.
        let last_branch = layout.items[rows.len() - 3];
        let first_flag = layout.items[rows.len() - 2];
        assert!(layout.separator[1] >= last_branch[3]);
        assert!(layout.separator[3] <= first_flag[1]);
    }

    /// T2 — a row's mark says whether it is on, and the radio and the checkboxes
    /// say it with two different idioms.
    #[test]
    fn the_filter_menus_marks_say_which_rows_are_on() {
        let mut filter = crate::git_graph::GraphFilter::default();
        assert!(git_filter_row_on(&GitFilterRow::All, &filter));
        assert!(git_filter_row_on(&GitFilterRow::Remotes, &filter));
        assert!(git_filter_row_on(&GitFilterRow::Tags, &filter));
        assert!(!git_filter_row_on(
            &GitFilterRow::Branch("main".to_owned()),
            &filter
        ));

        // A filled dot for the radio when it is on, a ring when it is not: the
        // Git page's own G35 idiom, where a filled mark is a state and an
        // outlined one is an offer.
        assert!(matches!(
            git_filter_mark(&GitFilterRow::All, true),
            Some(ChromeMark::ControlPill { .. })
        ));
        assert!(matches!(
            git_filter_mark(&GitFilterRow::All, false),
            Some(ChromeMark::ControlPillRing { .. })
        ));
        // A tick for a checkbox that is on, and **nothing at all** for one that
        // is off — an empty box drawn as empty space.
        assert_eq!(
            git_filter_mark(&GitFilterRow::Tags, true),
            Some(ChromeMark::Check)
        );
        assert_eq!(git_filter_mark(&GitFilterRow::Tags, false), None);

        filter.toggle_branch("main");
        assert!(!git_filter_row_on(&GitFilterRow::All, &filter));
        assert!(git_filter_row_on(
            &GitFilterRow::Branch("main".to_owned()),
            &filter
        ));
        assert_eq!(
            git_filter_mark(&GitFilterRow::Branch("main".to_owned()), true),
            Some(ChromeMark::Check)
        );
    }

    /// PIN — the caller names the first row, so the menu cannot promise a
    /// preview for a file that is going to the system's own handler.
    #[test]
    fn the_first_row_says_where_this_particular_file_is_going() {
        for open_text in ["Open preview", "Open"] {
            let layout = file_menu_layout(
                [300.0, 200.0],
                (960.0, 600.0),
                1.0,
                open_text,
                &mut fake_measure,
            );
            let layer = one_layer(file_menu_build(&layout, open_text, None));
            assert_eq!(
                layer.labels.first().map(|label| label.text.as_str()),
                Some(open_text)
            );
        }
    }

    /// PIN — a menu raised in the bottom-right corner is pulled whole back
    /// inside the window, on **both** axes.
    ///
    /// The red gate: the root menu clamps only horizontally, because a button on
    /// the top strip cannot be near the bottom. A file row can be — it is the
    /// last row of a tall column — and an unclamped drop puts all three verbs
    /// under the window's edge, where the menu is visible and unusable.
    #[test]
    fn a_menu_raised_in_the_corner_is_pulled_back_inside_on_both_axes() {
        let surface = (960.0, 600.0);
        let layout = file_menu_layout(
            [950.0, 596.0],
            surface,
            1.0,
            "Open preview",
            &mut fake_measure,
        );
        assert!(layout.frame[2] <= surface.0 - MENU_EDGE_MARGIN_LOGICAL_PX);
        assert!(layout.frame[3] <= surface.1 - MENU_EDGE_MARGIN_LOGICAL_PX);
        assert!(layout.frame[0] >= MENU_EDGE_MARGIN_LOGICAL_PX);
        assert!(layout.frame[1] >= MENU_EDGE_MARGIN_LOGICAL_PX);
        assert!(
            file_menu_hit(
                &layout,
                f64::from(layout.items[2][0] + 1.0),
                f64::from((layout.items[2][1] + layout.items[2][3]) / 2.0),
            ) == Some(Some(FileMenuRow::InsertPath)),
            "and the row that would have fallen off is still the one that answers"
        );
    }

    /// PIN — rows answer, the body swallows, outside is nobody's; and the rule
    /// is body, so a press on a hairline commits no verb.
    #[test]
    fn the_file_menu_answers_a_press_on_each_of_its_three_rows() {
        let layout = file_menu_layout(
            [300.0, 200.0],
            (960.0, 600.0),
            1.0,
            "Open",
            &mut fake_measure,
        );
        let middle = |rect: [f32; 4]| {
            (
                f64::from((rect[0] + rect[2]) / 2.0),
                f64::from((rect[1] + rect[3]) / 2.0),
            )
        };
        for (row, rect) in FileMenuRow::ALL.iter().zip(layout.items) {
            let (x, y) = middle(rect);
            assert_eq!(file_menu_hit(&layout, x, y), Some(Some(*row)));
        }
        let (x, y) = middle(layout.separator);
        assert_eq!(file_menu_hit(&layout, x, y), Some(None));
        assert_eq!(
            file_menu_hit(
                &layout,
                f64::from(layout.frame[0] - 4.0),
                f64::from(layout.frame[1] - 4.0)
            ),
            None
        );
    }

    /// PIN — the keyboard walk stops at both ends instead of wrapping round,
    /// which is the law the tree beside it already keeps (D45).
    #[test]
    fn the_file_menus_keyboard_walk_clamps_at_both_ends() {
        assert_eq!(FileMenuRow::step(None, true), FileMenuRow::Open);
        assert_eq!(FileMenuRow::step(None, false), FileMenuRow::InsertPath);
        assert_eq!(
            FileMenuRow::step(Some(FileMenuRow::Open), false),
            FileMenuRow::Open,
            "up from the first row stays on the first row"
        );
        assert_eq!(
            FileMenuRow::step(Some(FileMenuRow::InsertPath), true),
            FileMenuRow::InsertPath,
            "and down from the last stays on the last"
        );
        assert_eq!(
            FileMenuRow::step(Some(FileMenuRow::Open), true),
            FileMenuRow::CopyPath
        );
        assert_eq!(
            FileMenuRow::step(Some(FileMenuRow::InsertPath), false),
            FileMenuRow::CopyPath
        );
    }

    // ── v2 ④: the git context menus ────────────────────────────────────────

    fn commit_target(can_compare: bool, compare_ready: bool) -> GitMenuTarget {
        GitMenuTarget::Commit {
            hash: "a1b2c3d4e5f6".to_owned(),
            short: "a1b2c3d".to_owned(),
            subject: "the row tells its whole story".to_owned(),
            can_compare,
            compare_ready,
        }
    }

    fn change_target(group: crate::git::GitGroup) -> GitMenuTarget {
        GitMenuTarget::Change {
            path: "src/main.rs".to_owned(),
            group,
            untracked: group == crate::git::GitGroup::Untracked,
            renamed_from: None,
        }
    }

    /// PIN (v2 ④) — **each target offers the verbs its ruling allows, in order,
    /// and nothing else.**
    ///
    /// This is the whole menu specification as a table. The two things it is
    /// pointed at are the two ways a context menu goes wrong: a verb that
    /// appears where it cannot work (a `Rename…` on a tag), and a verb that
    /// quietly appears where the ruling says it may not (anything at all on a
    /// remote beyond the two rows M10 allows).
    ///
    /// MUTATION: add a `Fetch` row to the remote's list and it goes red on the
    /// vector; give the panel's commit a compare row and it goes red on the
    /// `can_compare: false` case.
    #[test]
    fn each_git_menu_target_offers_the_verbs_its_ruling_allows() {
        use GitMenuRow as R;
        assert_eq!(
            git_menu(&commit_target(true, true)).rows,
            vec![
                R::Checkout,
                R::CreateBranchHere,
                R::CreateTagHere,
                R::CopyHash,
                R::CopySubject,
                R::CompareWithSelected,
                R::CompareWithWorkingTree,
            ]
        );
        assert_eq!(
            git_menu(&commit_target(true, false)).rows,
            vec![
                R::Checkout,
                R::CreateBranchHere,
                R::CreateTagHere,
                R::CopyHash,
                R::CopySubject,
                R::CompareWithWorkingTree,
            ],
            "with nothing open there is no near end, so the row is not offered at all"
        );
        assert_eq!(
            git_menu(&commit_target(false, true)).rows,
            vec![
                R::Checkout,
                R::CreateBranchHere,
                R::CreateTagHere,
                R::CopyHash,
                R::CopySubject,
            ],
            "the panel has no compare mode to enter, so it offers neither comparison"
        );
        for current in [false, true] {
            assert_eq!(
                git_menu(&GitMenuTarget::LocalBranch {
                    name: "main".to_owned(),
                    current,
                })
                .rows,
                vec![R::Checkout, R::RenameBranch, R::DeleteBranch, R::CopyName],
                "the list does not move when HEAD does — only what is offered changes"
            );
        }
        assert_eq!(
            git_menu(&GitMenuTarget::Remote {
                name: "origin/main".to_owned(),
            })
            .rows,
            vec![R::CheckoutTracking, R::CopyName],
            "M10: no fetch, no pull, no delete-the-remote-branch"
        );
        assert_eq!(
            git_menu(&GitMenuTarget::Tag {
                name: "v1.0".to_owned(),
            })
            .rows,
            vec![R::Checkout, R::DeleteTag, R::CopyName]
        );
        assert_eq!(
            git_menu(&change_target(crate::git::GitGroup::Staged)).rows,
            vec![
                R::Unstage,
                R::Discard,
                R::OpenDiff,
                R::RevealInExplorer,
                R::CopyPath,
            ],
            "a row under STAGED means the index, so the verb it offers is the index's"
        );
        for group in [
            crate::git::GitGroup::Changes,
            crate::git::GitGroup::Untracked,
        ] {
            assert_eq!(
                git_menu(&change_target(group)).rows[0],
                R::Stage,
                "{group:?} is on the other side of the index"
            );
        }
        assert_eq!(
            git_menu(&GitMenuTarget::Uncommitted {
                compare_ready: true,
            })
            .rows,
            vec![R::CompareWithSelected]
        );
        assert!(
            git_menu(&GitMenuTarget::Uncommitted {
                compare_ready: false,
            })
            .rows
            .is_empty(),
            "with nothing to compare against the working tree's row raises no menu at all"
        );
    }

    /// PIN (v2 ④) — **one rule falls between the verbs and the readings**, in
    /// every menu that has both, and it is derived rather than placed.
    ///
    /// MUTATION: put `Copy hash` above `Add tag here…` in the commit's list and
    /// the separator lands in the middle of the writes, which this catches — the
    /// order and the rule are one fact.
    #[test]
    fn the_git_menus_rule_falls_where_the_writing_stops() {
        for target in [
            commit_target(true, true),
            GitMenuTarget::LocalBranch {
                name: "main".to_owned(),
                current: false,
            },
            GitMenuTarget::Remote {
                name: "origin/main".to_owned(),
            },
            GitMenuTarget::Tag {
                name: "v1.0".to_owned(),
            },
            change_target(crate::git::GitGroup::Changes),
        ] {
            let menu = git_menu(&target);
            let at = menu
                .separator_after
                .unwrap_or_else(|| panic!("{target:?} has both kinds of row"));
            assert!(
                menu.rows[..=at].iter().all(|row| row.writes()),
                "{target:?}: everything above the rule changes the repository"
            );
            assert!(
                menu.rows[at + 1..].iter().all(|row| !row.writes()),
                "{target:?}: and everything below it only reads"
            );
        }
        assert_eq!(
            git_menu(&GitMenuTarget::Uncommitted {
                compare_ready: true,
            })
            .separator_after,
            None,
            "a menu with one reading in it has nothing to divide"
        );
    }

    /// PIN (v2 ④) — **the branch you are standing on offers neither a checkout
    /// nor a delete**, and both are drawn rather than hidden.
    ///
    /// Drawn, because a menu whose rows move depending on where `HEAD` is is a
    /// menu you cannot learn the shape of. Not offered, because one is pointless
    /// and the other is a promise git will always break.
    ///
    /// MUTATION: return `true` for `DeleteBranch` on the current branch and the
    /// menu starts offering a row whose only possible outcome is a red card.
    #[test]
    fn the_branch_you_are_standing_on_offers_neither_a_checkout_nor_a_delete() {
        let current = GitMenuTarget::LocalBranch {
            name: "main".to_owned(),
            current: true,
        };
        let other = GitMenuTarget::LocalBranch {
            name: "side".to_owned(),
            current: false,
        };
        for row in git_menu(&current).rows {
            let allowed = !matches!(row, GitMenuRow::Checkout | GitMenuRow::DeleteBranch);
            assert_eq!(git_menu_row_available(row, &current), allowed, "{row:?}");
            assert!(
                git_menu_row_available(row, &other),
                "{row:?} is available on every other branch"
            );
        }
        // And a keyboard walk steps over the two, rather than landing on a row
        // that answers nothing.
        let rows = git_menu(&current).rows;
        assert_eq!(
            git_menu_step(&rows, &current, None, true),
            Some(GitMenuRow::RenameBranch),
            "the first row a walk can land on is the first one that works"
        );
        assert_eq!(
            git_menu_step(&rows, &current, Some(GitMenuRow::RenameBranch), true),
            Some(GitMenuRow::CopyName)
        );
        assert_eq!(
            git_menu_step(&rows, &current, Some(GitMenuRow::CopyName), true),
            Some(GitMenuRow::CopyName),
            "and the bottom clamps, as every walk in this window does"
        );
        assert_eq!(
            git_menu_step(&rows, &current, None, false),
            Some(GitMenuRow::CopyName),
            "from nowhere, a step back offers the last row"
        );
        assert_eq!(
            git_menu_step(&rows, &other, None, true),
            Some(GitMenuRow::Checkout),
            "on any other branch the walk starts at the top"
        );
    }

    /// PIN (v2 ④) — a press lands on the row it looks like, the body swallows,
    /// **and a row that cannot do what it says answers nothing at all.**
    #[test]
    fn the_git_menu_answers_a_press_on_each_row_that_can_act() {
        let target = GitMenuTarget::LocalBranch {
            name: "main".to_owned(),
            current: true,
        };
        let look = GitMenuLook {
            target: &target,
            hover: None,
            prompt: None,
        };
        let layout = git_menu_layout(
            [300.0, 200.0],
            (960.0, 600.0),
            1.0,
            &look,
            &mut fake_measure,
        );
        for item in &layout.items {
            let (x, y) = (
                f64::from((item.rect[0] + item.rect[2]) / 2.0),
                f64::from((item.rect[1] + item.rect[3]) / 2.0),
            );
            assert_eq!(
                git_menu_hit(&layout, x, y),
                Some(if item.available { Some(item.row) } else { None }),
                "{:?} is {}",
                item.row,
                if item.available {
                    "offered"
                } else {
                    "drawn only"
                }
            );
        }
        assert_eq!(
            git_menu_hit(&layout, f64::from(layout.frame[0] - 4.0), 200.0),
            None,
            "outside is nobody's"
        );
        // The rule is body: a press on the hairline commits no verb.
        let separator = layout
            .separator
            .expect("a branch menu has both kinds of row");
        assert_eq!(
            git_menu_hit(
                &layout,
                f64::from((separator[0] + separator[2]) / 2.0),
                f64::from((separator[1] + separator[3]) / 2.0),
            ),
            Some(None)
        );
    }

    /// PIN (v2 ④) — a git menu raised in the corner is pulled whole back inside
    /// the window, on both axes, and the row that would have fallen off still
    /// answers.
    ///
    /// The file menu's own ruling, and it matters more here: this menu is raised
    /// on the last row of a graph that fills a pane, which is exactly the corner.
    #[test]
    fn a_git_menu_raised_in_the_corner_is_pulled_back_inside_on_both_axes() {
        let surface = (960.0, 600.0);
        let target = commit_target(true, true);
        let look = GitMenuLook {
            target: &target,
            hover: None,
            prompt: None,
        };
        let layout = git_menu_layout([955.0, 598.0], surface, 1.0, &look, &mut fake_measure);
        assert!(layout.frame[2] <= surface.0 - MENU_EDGE_MARGIN_LOGICAL_PX);
        assert!(layout.frame[3] <= surface.1 - MENU_EDGE_MARGIN_LOGICAL_PX);
        assert!(layout.frame[0] >= MENU_EDGE_MARGIN_LOGICAL_PX);
        assert!(layout.frame[1] >= MENU_EDGE_MARGIN_LOGICAL_PX);
        let last = *layout.items.last().expect("seven rows");
        assert_eq!(last.row, GitMenuRow::CompareWithWorkingTree);
        assert_eq!(
            git_menu_hit(
                &layout,
                f64::from(last.rect[0] + 1.0),
                f64::from((last.rect[1] + last.rect[3]) / 2.0),
            ),
            Some(Some(GitMenuRow::CompareWithWorkingTree))
        );
    }

    /// PIN (v2 ④) — **the menu becomes the prompt**: the rows go, a captioned
    /// field stands in their place, and a name git would refuse grows a red line
    /// under it rather than a card somewhere else.
    ///
    /// MUTATION: keep the rows under the field and the menu answers presses on
    /// verbs while a name is being typed — which is a popup with two keyboards.
    #[test]
    fn a_git_prompt_replaces_the_rows_and_grows_a_line_when_the_name_is_wrong() {
        let target = commit_target(true, true);
        let caption = GitPromptKind::CreateBranch.caption("a1b2c3d");
        assert_eq!(caption, "New branch at a1b2c3d");
        let clean = GitPromptLook {
            kind: GitPromptKind::CreateBranch,
            caption: &caption,
            text: "feature",
            before_caret: "feature",
            preedit: "",
            fault: None,
        };
        let look = GitMenuLook {
            target: &target,
            hover: None,
            prompt: Some(clean),
        };
        let layout = git_menu_layout(
            [300.0, 200.0],
            (960.0, 600.0),
            1.0,
            &look,
            &mut fake_measure,
        );
        assert!(
            layout.items.is_empty(),
            "the verbs are gone while the field is up"
        );
        let rects = layout.prompt.expect("the prompt is laid out");
        assert!(rects.hint.is_none(), "a name git takes needs no line");
        assert!(
            rects.field[1] >= rects.caption[3],
            "the caption stands over the field"
        );
        assert_eq!(
            git_menu_hit(&layout, 310.0, f64::from(rects.field[1] + 2.0)),
            Some(None),
            "every press inside the prompt is the menu's own and commits nothing"
        );

        let bad = GitPromptLook {
            fault: Some(crate::git::RefNameFault::Space),
            text: "my branch",
            before_caret: "my branch",
            ..clean
        };
        let look = GitMenuLook {
            target: &target,
            hover: None,
            prompt: Some(bad),
        };
        let taller = git_menu_layout(
            [300.0, 200.0],
            (960.0, 600.0),
            1.0,
            &look,
            &mut fake_measure,
        );
        let rects = taller.prompt.expect("the prompt is laid out");
        let hint = rects.hint.expect("a refused name says why");
        assert!(hint[1] >= rects.field[3], "and it says so under the field");
        assert!(
            taller.frame[3] > layout.frame[3],
            "the menu grows by exactly that line rather than covering the field"
        );
        let layer = one_layer(git_menu_build(&taller, &look));
        assert!(
            layer
                .labels
                .iter()
                .any(|label| label.text == crate::git::RefNameFault::Space.sentence()),
            "and the line is drawn: {:?}",
            layer.labels.iter().map(|l| &l.text).collect::<Vec<_>>()
        );
        assert!(
            layer.labels.iter().any(|label| label.text == "my branch"),
            "with what was typed still visible above it"
        );
    }

    /// PIN (v2 ④) — the three rows that end in `…` are the three that ask again,
    /// and no others.
    ///
    /// The ellipsis is a promise the platform's convention already makes; this
    /// keeps the promise and the behaviour one fact.
    #[test]
    fn the_rows_that_ask_again_are_exactly_the_rows_whose_names_say_so() {
        for row in [
            GitMenuRow::Checkout,
            GitMenuRow::CreateBranchHere,
            GitMenuRow::CreateTagHere,
            GitMenuRow::RenameBranch,
            GitMenuRow::DeleteBranch,
            GitMenuRow::DeleteTag,
            GitMenuRow::CheckoutTracking,
            GitMenuRow::Stage,
            GitMenuRow::Unstage,
            GitMenuRow::Discard,
            GitMenuRow::OpenDiff,
            GitMenuRow::RevealInExplorer,
            GitMenuRow::CopyPath,
            GitMenuRow::CopyHash,
            GitMenuRow::CopySubject,
            GitMenuRow::CopyName,
            GitMenuRow::CompareWithSelected,
            GitMenuRow::CompareWithWorkingTree,
        ] {
            assert_eq!(
                row.prompt().is_some(),
                row.text().ends_with('\u{2026}'),
                "{row:?} says {:?}",
                row.text()
            );
        }
        assert_eq!(GitPromptKind::RenameBranch.caption("main"), "Rename main");
        assert_eq!(
            GitPromptKind::CreateTag.caption("a1b2c3d"),
            "New tag at a1b2c3d"
        );
        for kind in [
            GitPromptKind::CreateBranch,
            GitPromptKind::CreateTag,
            GitPromptKind::RenameBranch,
        ] {
            assert!(!kind.placeholder().is_empty());
        }
    }
    // ── ticket #62: the terminal's own context menu ─────────────────────────

    /// PIN (ticket #62) — **the row list is `docs/DESIGN.md` §7.1.6 plus S3's
    /// `Find…`, in that order, with the rule in the one place it belongs.**
    ///
    /// The order is asserted as a whole list rather than as a set of neighbour
    /// pairs, because the ruling is a sentence and not a collection of
    /// constraints: 「Copy、Paste、Select all、──、Clear screen、Clear
    /// scrollback…、Restart shell…」, with `Find…` booked in after `Select all`
    /// by §7.1.5d's landing note. A list this test agreed with only pairwise
    /// would let `Find…` slide below the rule and stay green.
    ///
    /// MUTATION: move `Find…` past the separator and the second assertion names
    /// the row that is now standing among the destroyers.
    #[test]
    fn the_terminal_menus_rows_are_the_designs_order_with_find_above_the_rule() {
        use TermMenuRow as R;
        assert_eq!(
            TERM_MENU_ROWS,
            [
                R::Copy,
                R::Paste,
                R::SelectAll,
                R::Find,
                R::ClearScreen,
                R::ClearScrollback,
                R::RestartShell,
            ]
        );
        assert_eq!(
            TERM_MENU_ROWS[TERM_MENU_SEPARATOR_AFTER],
            R::Find,
            "the rule falls after the last row that only reads the pane"
        );
        assert_eq!(
            TERM_MENU_ROWS[TERM_MENU_SEPARATOR_AFTER + 1],
            R::ClearScreen,
            "and the first row below it is the first that changes something"
        );
        // The three that ask again before they act, and no others: the platform
        // convention the git menu's prompts already keep.
        for row in TERM_MENU_ROWS {
            assert_eq!(
                matches!(row, R::Find | R::ClearScrollback | R::RestartShell),
                row.text().ends_with('\u{2026}'),
                "{row:?} says {:?}",
                row.text()
            );
        }
        assert_eq!(R::Copy.text(), "Copy");
        assert_eq!(R::Paste.text(), "Paste");
        assert_eq!(R::SelectAll.text(), "Select all");
        assert_eq!(R::ClearScreen.text(), "Clear screen");
        // `Find…` is the one row with no mark, and it is a decision rather than
        // an omission — see `TermMenuRow::mark`.
        for row in TERM_MENU_ROWS {
            assert_eq!(row.mark().is_none(), row == R::Find, "{row:?}");
        }
    }

    /// PIN (ticket #62, item 4) — **three rows can be greyed and no others**:
    /// `Copy` without a selection, `Find…` on the alternate screen, and `Restart
    /// shell…` while a restart is in flight.
    ///
    /// `Paste` is asserted *available on an empty clipboard* rather than left
    /// unmentioned, because "always enabled" is the decision this slice made and
    /// not a gap in it: the cheap availability query is unwrapped on this
    /// platform, so the row is offered and a paste with nothing to paste is a
    /// no-op. A later slice that wraps `IsClipboardFormatAvailable` will have to
    /// come here to change it.
    ///
    /// MUTATION: grey `Copy` on the wrong side of the bit and the first block
    /// goes red; drop any of the three arms and the sweep below names the row
    /// that has started answering for a pane it cannot answer for.
    #[test]
    fn only_copy_find_and_restart_are_ever_greyed_on_the_terminal_menu() {
        use TermMenuRow as R;
        let idle = TermMenuSubject::default();
        let selected = TermMenuSubject {
            has_selection: true,
            ..idle
        };
        let restarting = TermMenuSubject {
            restart_in_flight: true,
            ..idle
        };

        let on_alt_screen = TermMenuSubject {
            can_search: false,
            ..idle
        };

        assert!(!term_menu_row_available(R::Copy, idle));
        assert!(term_menu_row_available(R::Copy, selected));
        assert!(term_menu_row_available(R::RestartShell, idle));
        assert!(!term_menu_row_available(R::RestartShell, restarting));
        // `Find…` is greyed on the one screen a search cannot be addressed to,
        // and offered on every other pane — including an empty one, whose honest
        // answer is `0/0` rather than "no".
        assert!(term_menu_row_available(R::Find, idle));
        assert!(!term_menu_row_available(R::Find, on_alt_screen));

        // The sweep: the four rows nothing can grey stay offered on every pane
        // this menu can be raised over, including the alternate screen.
        for subject in [idle, selected, restarting, on_alt_screen] {
            for row in TERM_MENU_ROWS {
                if matches!(row, R::Copy | R::Find | R::RestartShell) {
                    continue;
                }
                assert!(
                    term_menu_row_available(row, subject),
                    "{row:?} answers whatever the pane is doing"
                );
            }
        }
    }

    /// PIN (ticket #62, item 5) — **the keyboard walk steps over the rows that
    /// answer nothing, and clamps at both ends.**
    ///
    /// Clamped rather than cyclic, which is [`FileMenuRow::step`]'s ruling and
    /// the tree's: one window must not hold two ideas of what the bottom of a
    /// list does. From nowhere, a step forwards offers the first *available* row
    /// — which on a pane with no selection is `Paste`, not the greyed `Copy` the
    /// list starts with.
    ///
    /// MUTATION: walk `TERM_MENU_ROWS` instead of the filtered list and the walk
    /// lands on a row a press would fall straight through.
    #[test]
    fn the_terminal_menus_walk_skips_the_greyed_rows_and_stops_at_both_ends() {
        use TermMenuRow as R;
        let idle = TermMenuSubject::default();
        assert_eq!(term_menu_step(None, idle, true), Some(R::Paste));
        assert_eq!(term_menu_step(None, idle, false), Some(R::RestartShell));
        assert_eq!(
            term_menu_step(Some(R::Paste), idle, false),
            Some(R::Paste),
            "the top of the walk is the top of what is walkable"
        );
        assert_eq!(
            term_menu_step(Some(R::RestartShell), idle, true),
            Some(R::RestartShell),
            "and the bottom clamps rather than wrapping"
        );

        let mid_restart = TermMenuSubject {
            has_selection: true,
            restart_in_flight: true,
            ..idle
        };
        assert_eq!(term_menu_step(None, mid_restart, true), Some(R::Copy));
        assert_eq!(
            term_menu_step(Some(R::ClearScrollback), mid_restart, true),
            Some(R::ClearScrollback),
            "a restart in flight makes Clear scrollback the last walkable row"
        );
        assert_eq!(
            term_menu_step(None, mid_restart, false),
            Some(R::ClearScrollback)
        );
    }

    /// PIN (ticket #62) — **a menu raised in the corner is pulled back inside on
    /// both axes, and its last row is still pressable.**
    ///
    /// The git menu's own test, on this list, and the second half is the half
    /// that matters: a frame clamped into the window with rows still laid out
    /// from the original point would be a menu that *looks* right and answers
    /// presses seven rows away.
    #[test]
    fn a_terminal_menu_raised_in_the_corner_is_pulled_back_inside_on_both_axes() {
        let surface = (960.0, 600.0);
        let look = TermMenuLook::default();
        let layout = term_menu_layout([958.0, 599.0], surface, 1.0, &look, &mut fake_measure);
        let frame = layout.frame;
        assert!(frame[2] <= surface.0 - MENU_EDGE_MARGIN_LOGICAL_PX);
        assert!(frame[3] <= surface.1 - MENU_EDGE_MARGIN_LOGICAL_PX);
        assert!(frame[0] >= MENU_EDGE_MARGIN_LOGICAL_PX);
        assert!(frame[1] >= MENU_EDGE_MARGIN_LOGICAL_PX);

        let last = *layout.items.last().expect("the last row is laid out");
        assert_eq!(last.row, TermMenuRow::RestartShell);
        assert!(last.rect[3] <= frame[3], "the last row is inside the frame");
        assert_eq!(
            term_menu_hit(
                &layout,
                f64::from(last.rect[0] + 1.0),
                f64::from((last.rect[1] + last.rect[3]) / 2.0),
            ),
            Some(Some(TermMenuRow::RestartShell))
        );
    }

    /// PIN (ticket #62) — **the rule stands between the fourth row and the
    /// fifth, and a greyed row is drawn but not offered.**
    ///
    /// The two facts are one test because they are the same claim about the
    /// layout being the authority: what the hit test answers has to agree with
    /// what the painter put there, and the separator's own band is the gap that
    /// proves the rows below it were pushed down rather than merely drawn over.
    ///
    /// MUTATION: let `term_menu_hit` answer greyed rows and the pointer lights a
    /// `Copy` that a press does nothing with.
    #[test]
    fn the_terminal_menus_rule_separates_the_two_halves_and_greyed_rows_are_not_offered() {
        let look = TermMenuLook::default();
        let layout = term_menu_layout(
            [200.0, 120.0],
            (960.0, 600.0),
            1.0,
            &look,
            &mut fake_measure,
        );
        let row = |wanted: TermMenuRow| {
            *layout
                .items
                .iter()
                .find(|item| item.row == wanted)
                .expect("every row is laid out")
        };
        let find = row(TermMenuRow::Find);
        let clear = row(TermMenuRow::ClearScreen);
        let rule = layout.separator;
        assert!(
            find.rect[3] <= rule[1] && rule[3] <= clear.rect[1],
            "the rule stands in the gap between the two halves"
        );

        let copy = row(TermMenuRow::Copy);
        assert!(!copy.available, "no selection, so Copy cannot answer");
        assert_eq!(
            term_menu_hit(
                &layout,
                f64::from(copy.rect[0] + 1.0),
                f64::from((copy.rect[1] + copy.rect[3]) / 2.0),
            ),
            Some(None),
            "the pointer falls through a greyed row onto the menu's own body"
        );
        assert_eq!(
            term_menu_hit(&layout, 1.0, 1.0),
            None,
            "and misses the menu entirely outside it"
        );

        // Drawn all the same, which is the other half of "greyed, not hidden":
        // one label per entry, whatever each of them can answer.
        let layers = term_menu_build(&layout, &look);
        assert_eq!(layers.len(), 1);
        assert_eq!(
            layers[0].labels.len(),
            TERM_MENU_ROWS.len(),
            "every row is painted, including the ones that cannot answer"
        );
    }
}
