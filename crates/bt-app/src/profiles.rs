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
    sync::{
        Arc, Mutex, OnceLock, RwLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};

use bt_layout::Axis;
use bt_persist::{
    CandidateV1, MarkV1, NamedStartAtV1, NamedStartingDirV1, PROFILES_SCHEMA_VERSION,
    ProfileEntryV1, ProfilesV1, ProgramV1, ResolutionV1, StartAtV1, StartingDirV1,
};
use bt_pty::{ShellEnvironment, resolve_powershell_seven};
use bt_render::{
    ChromeLabel, ChromeLabelWeight, ChromePalette, FLOAT_WINDOW_BORDER_LOGICAL_PX,
    FLOAT_WINDOW_SHADOW_LOGICAL_PX, OverlayQuad, Travel, chrome_palette, rounded_overlay_fill,
};

use crate::{
    LeafId,
    icons::ActionIcon,
    marks::{ChromeMark, ChromeSprite, MarkColour, OverlayLayer},
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
/// **How big a mark is drawn inside that column** —
/// [`crate::icons::MarkSlot::Menu`], asked of the mark.
///
/// It was two constants and a list of enum names until the icon block: a `15`
/// for most marks and a `10` for the four the title bar lends a menu, with those
/// four written out by name. **The list was the bug.** `#i-minus` is cut exactly
/// as `#i-plus` is — `marks.rs` says so in as many words, "the pair are one
/// drawing minus a stroke: same ten-unit box, same 1.2 weight, same round cap"
/// — and `#i-plus` was on the list while `#i-minus` was not, so `Unstage` drew
/// a stroke of `1.80` where `Stage` drew `1.20` and out-weighed every neighbour
/// in its own menu by half again. `#i-chev` and the resize grip were off it for
/// the same reason: nobody had thought of them the day the list was written.
///
/// The slot asks [`ChromeMark::draws_edge_to_edge`] instead — a fact about
/// where a drawing's ink stops — and derives the two boxes from the ink the two
/// families carry rather than from two hand-picked numbers. Both user rulings
/// the old list was written for (2026-08-16, the `×`; 2026-08-19,
/// `Enter focus mode`'s `#i-max`) are the same ruling under that derivation, and
/// so is the `#i-minus` nobody got round to reporting.
///
/// It answers `[width, height]` rather than one number, because a mark that is
/// not square is now fitted at its own aspect — see the slot for why that is
/// the other half of the pane head's problem.
fn item_mark_box_logical_px(mark: ChromeMark) -> [f32; 2] {
    crate::icons::MarkSlot::Menu.mark_box_logical_px(mark)
}
/// `.default-hint { margin-left: auto; font-size: 11px; color: var(--ink3) }`.
///
/// Two annotations ride in this one slot: the profile list's `default`, and a
/// recent row's `agoLabel` (mock-up 7428/7432). They are the same declaration in the
/// same place, so they are the same number here.
const HINT_FONT_LOGICAL_PX: f32 = 11.0;
fn hint_text() -> &'static str {
    crate::i18n::Text::ProfileHintDefault.text()
}
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
fn unavailable_hint_text() -> &'static str {
    crate::i18n::Text::ProfileHintUnavailable.text()
}

/// **The `˅` menu's second section: one row, and what it is for** (H113,
/// mock-up 7417-7423).
///
/// Every row above it makes a **tab**; this one adds a **pane** to the tab you
/// are already in, and that is a different enough verb that the mock-up puts a
/// rule between them rather than letting it read as a fifth profile.
fn files_pane_text() -> &'static str {
    crate::i18n::Text::ProfileFilesPane.text()
}
/// The annotation that keeps the row honest about the difference.
///
/// It rides the same `.default-hint` slot as `default` and `3m ago`, and it is
/// doing the same job: saying the one thing about the row that its caption does
/// not. Without it "Files pane" sits under four rows that all open new tabs and
/// looks like the fifth.
fn files_pane_hint_text() -> &'static str {
    crate::i18n::Text::ProfileFilesPaneHint.text()
}

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
fn recent_section_label() -> &'static str {
    crate::i18n::Text::ProfileRecentSection.text()
}

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
/// **Owned data since §7.1.6c-6, and that is the whole of slice 5a's
/// foundation.** It was five `Copy` structs of `&'static` fields in a `const`
/// array, which is exactly the right shape for a table nobody can change and
/// exactly the wrong one for a table that has a settings page. What replaces it
/// is [`ProfileTable`]: the shipped five ([`shipped`]) merged with
/// `profiles.json`'s departures and whatever profiles the user has made.
///
/// The old constant survives as the *seed*, not as the table: it is what a
/// machine with no file gets, what "restore this profile" compares against, and
/// where [`Self::compared_title`] comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    pub id: String,
    /// The **shipped** title, kept for byte-comparison and never drawn.
    ///
    /// This is the string the integration scripts announce — `folio.ps1` sends
    /// `$PSVersionTable.PSEdition`, which is `PowerShell` or `Windows
    /// PowerShell` whatever the row is called in this window — and it is what a
    /// pane head compares an OSC 0/2 announcement against before deciding the
    /// shell is merely echoing its launcher.
    ///
    /// **It is not a setting, it is a protocol constant** (plan §1.4). It does
    /// not appear on any surface and cannot be edited. A profile of the user's
    /// own has none: no script this build ships will ever announce a name this
    /// build did not choose, so there is no second string to compare.
    ///
    /// The comparison set is `{compared_title} ∪ {display_title}` — see
    /// [`announces_this_profile`]. Both, because after a rename neither the
    /// script's word nor the user's own may leak onto a head as a program title.
    pub compared_title: Option<String>,
    /// The name every surface draws.
    ///
    /// [`title`] is this plus whatever qualifier the machine supplied, and that
    /// composed string is what a tab, a rail, a pane head, a picker and a
    /// window title all show.
    ///
    /// Not translated: `PowerShell 7`, `WSL`, `Git Bash` and `Command Prompt`
    /// are product names, and a Chinese window spells them the same way an
    /// English one does (§G S103) — which is why the shipped defaults live in
    /// this table and not in `i18n.rs`, pinned there by
    /// `no_profile_title_has_been_pulled_into_the_language_table`.
    pub display_title: String,
    /// A profile's icon is its mark, not a letter that happens to be in its
    /// prompt — the mock-up says so in as many words at `const mark`.
    ///
    /// The five shipped marks are not this product's to repaint (S98/S31: the
    /// blue is Microsoft's and the orange is Ubuntu's), and a profile duplicated
    /// from a built-in inherits the mark it really is — a copy of PowerShell is
    /// a PowerShell, and the mark is telling the truth. The eight struck colours
    /// a profile drawn from nothing wears are the editor's, one slice on.
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
    pub args: Vec<String>,
    /// What this profile sets in its sessions' environment, over what the
    /// terminal sets for itself.
    ///
    /// **Three layers, and this one is written last** (plan §1.7, landed in
    /// §7.1.6c-6c): the environment this window inherited, then the terminal's
    /// own declarations (`TERM_PROGRAM`, `TERM_PROGRAM_VERSION`, `COLORTERM`,
    /// `TERM`, the `FORCE_HYPERLINK` declaration, `PROMPT` for `cmd`,
    /// `BT_SHELL_INTEGRATION` for a bash), then these. A row here therefore
    /// **wins**, `TERM_PROGRAM` included, and that is the ruling rather than an
    /// oversight: a profile's environment is the most specific sentence anybody
    /// says about its sessions, and `hyperlink_declaration`'s own rule already
    /// is that whoever set the variable has answered the question. `BT_SHELL`
    /// surviving as a debugging back door (Q4) says the same thing — this
    /// machine belongs to the person using it.
    ///
    /// **An empty value takes the variable away from this profile's sessions**
    /// — measured on the real machine rather than assumed, because it is the
    /// operating system's answer and not this terminal's: an environment block
    /// entry whose value is empty removes the name instead of binding it to the
    /// empty string, so a child of a profile carrying `FOO=` has no `FOO` at
    /// all, *including* when the window itself inherited one. The storage
    /// therefore needs no third state to spell "remove": clearing a value box
    /// is that, and it is also what a reader clearing a value box means.
    /// A row with an empty **name** is not a variable at all and never reaches
    /// a child (`crate::shell_integration::layer_profile_environment`).
    pub env: Vec<(String, String)>,
    /// Where a leaf of this profile opens when nothing else says.
    ///
    /// The mock-up has no such field: it has one `HOME` constant (line 2632) that
    /// every profile shares, because every one of its profiles is a fiction that
    /// never starts a process. A real one has to say *whose* home, and the answer
    /// is not the same kind of thing for all four — see [`StartingDir`].
    ///
    /// **Not editable, and that is not this slice's boundary but the field's own
    /// nature** (plan §1.6, met again one slice on). Which channel a launcher
    /// listens on is a fact about the program — `wsl.exe` takes `--cd ~` and a
    /// Windows shell takes a working directory — and a reader who picked the
    /// wrong one would get a WSL tab standing in `/mnt/c/Users/…`, which is a
    /// real directory and not the one that shell opens in. What the editor
    /// offers instead is [`Self::start_at`], which is a different question.
    pub starting_dir: StartingDir,
    /// Which of three answers a new leaf takes when it is asked where to open —
    /// **the editor's question**, and the three items the mock-up's `Starting
    /// directory` combo carries.
    ///
    /// A second field beside [`Self::starting_dir`] rather than three more
    /// variants of it, because they are two questions and only one of them is
    /// anybody's to answer. `starting_dir` says *where this profile's home is
    /// and how its launcher is told about it*; this says whether a new tab takes
    /// the folder of the pane it was opened beside, always goes home, or always
    /// goes to one named place. Folding them together would have made "which
    /// flag does the launcher take" a thing a picker could get wrong.
    ///
    /// [`StartAt::Inherit`] is what every shipped profile carries and what every
    /// leaf in this window has always done, so a table nobody has edited spawns
    /// byte for byte what it spawned before this field existed.
    pub start_at: StartAt,
    /// Which spelling of a path this profile's shell speaks — see
    /// [`PathNamespace`]. What makes a directory inherited from another pane
    /// either translatable or honestly refused.
    pub paths: PathNamespace,
    /// What this profile's title has to name before it is unambiguous here.
    pub qualifier: Qualifier,
    /// Which shell-integration script this profile is served by, if any —
    /// derived from the program, or named outright. See [`IntegrationChoice`],
    /// and [`served_by`] for the resolved answer every other module wants.
    pub integration: IntegrationChoice,
    /// Kept out of the pickers.
    ///
    /// A built-in cannot be deleted — a row that is missing looks exactly like a
    /// row that was never designed, which is the sentence this module already
    /// writes about a shell the machine does not have — so hiding is the whole
    /// of what "I do not want to see this" can mean here. A hidden profile is
    /// still a profile: a seat already on disk restarts through its own
    /// `profile_id` and is untouched by this.
    pub hidden: bool,
    /// Whether this build shipped it, or the user made it.
    ///
    /// **Availability is deliberately not here.** Whether this machine can start
    /// a profile is [`ProfilePrograms`]'s answer and stays there: it is a fact
    /// about a filesystem probed once, not a field of the table, and a copy of
    /// it on the row would be a second place for the same question to be
    /// answered — with the copy going stale exactly when a program is installed
    /// while the window is open.
    pub origin: Origin,
}

/// Where a profile came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// One of the shipped five. Every field may be overridden but the colour,
    /// each override is undone one profile at a time, and it can be hidden but
    /// never deleted.
    Builtin,
    /// The user's own. It has no shipped answer to fall back to, so its entry in
    /// `profiles.json` carries everything about it.
    User,
}

/// Where a profile's shell stands when it is not told.
///
/// Two shapes, because "home" is not one fact here. Three of these profiles run
/// as Windows processes and take their starting directory the way every Windows
/// process does — as a working directory handed to `CreateProcess`. WSL's shell
/// does not: `wsl.exe` is a *launcher*, its working directory is a Windows path
/// that the distribution sees through `/mnt`, and the Linux home it should open
/// in has no Windows spelling at all. Handing `C:\Users\Alice` to a WSL tab lands
/// it in `/mnt/c/Users/Alice` — a real directory, and not the one a shell opens
/// in when you start it yourself.
///
/// So the enum carries the *form* the answer takes rather than a path. That is
/// what keeps this from being a special case bolted onto the spawn path: a
/// profile states how it is told where to start, and the spawn reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// rather than a trick: verified on this machine to answer `/home/alice`
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
        flag: String,
        /// What the flag is given when nothing has been inherited — the shell's
        /// own `$HOME`, which has no Windows spelling to hand over instead.
        home: String,
    },
}

/// Where a new leaf of a profile opens — the three answers the editor offers,
/// and the one question about a starting directory that is the reader's.
///
/// [`StartingDir`] above is the *form* an answer takes when a profile has to
/// name its own home; this is what happens **before** that question is reached.
/// The mock-up's combo writes the three `The current pane's folder`, `Home` and
/// `Choose a folder…`, and the first is selected on a profile nobody has
/// touched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartAt {
    /// The folder the pane it was opened beside is standing in, when there is
    /// one and this profile can spell it; this profile's own home when there is
    /// not.
    ///
    /// **Today's behaviour and the shipped value**, which is what makes this
    /// field free: a table nobody has edited answers exactly what it answered
    /// before the field existed. [`cwd_for_spawn`] is the "can this profile
    /// spell it" half and it is unchanged — a pair of namespaces that cannot
    /// cross still falls through to the profile's own home rather than to a
    /// guess.
    Inherit,
    /// This profile's own home, whatever the pane it was opened beside was
    /// standing in.
    ///
    /// Not the same sentence as [`Self::Inherit`] with nothing to inherit: this
    /// one *refuses* an inheritance that exists, which is what somebody who
    /// keeps one profile pinned to a home directory is asking for.
    Home,
    /// This place, always — a folder chosen through the system's own picker.
    ///
    /// Held in the namespace the picker speaks, which is Windows', and
    /// translated into the profile's at spawn through [`translate_cwd`] rather
    /// than at the moment it was chosen. Storing the translation would freeze
    /// it: a profile whose program later changes from `pwsh.exe` to `wsl.exe`
    /// changes which namespace it speaks, and a path converted on the day it was
    /// browsed to would then be written in the wrong one.
    Fixed(PathBuf),
}

/// How a profile's executable is located on the machine.
///
/// Two shapes rather than one because they answer to different authorities.
/// [`Self::PowerShellSeven`] defers to a resolution *order* that is already ruled
/// and already tested (`bt_pty::resolve_powershell_seven`: `BT_SHELL`, then a
/// `pwsh` probe); [`Self::FirstOf`] is a list of places to look, in order, for a
/// program that either is on this machine or is not.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    FirstOf(Vec<ProgramCandidate>),
    /// This program, at this path, and no search at all.
    ///
    /// What a profile of the user's own says when somebody has typed or browsed
    /// to an executable. A duplicate of a built-in does **not** collapse into
    /// this: it clones the built-in's own resolution, because `pwsh` follows
    /// `BT_SHELL` and then a probe, and a copy frozen to whatever that answered
    /// on the day it was made would quietly stop being a copy the first time the
    /// original moved.
    Path(PathBuf),
}

/// One place to look for a profile's executable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgramCandidate {
    /// `%VARIABLE%\tail` — an environment variable and a path under it.
    ///
    /// Never a bare relative path: "wherever this process happens to be
    /// standing" is not a place a shell lives.
    Under { variable: String, tail: String },
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
    BesideOnPath { anchor: String, tail: String },
}

/// Which shell-integration script a profile is served by, and **how it gets
/// there** — the two answers are not the same, and the difference is what the
/// honest-capability matrix is made of.
///
/// **[`Self::None`] arrived with §7.1.6c-6 and does not undo that reasoning.**
/// The variant was refused while every profile in the table was one this build
/// shipped, and each of those five has a way in; what differs is how far it
/// reaches, and the variants say so — a distinction a blanket `None` would have
/// flattened, by spelling "we found no door" and "the door is only wide enough
/// for one marker" the same way. A profile of the user's own running an
/// arbitrary executable is the case that reopens it, and it reopens it honestly
/// rather than by widening one of the other three: handing `--init-file` to a
/// program that is not a bash makes it a filename to open.
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
    /// No door at all — nothing is dot-sourced, no argument is added and no
    /// `PROMPT` is written.
    ///
    /// **The degradation needs no invention**: a screen that never sees OSC 133
    /// keeps the cursor/WRAPLINE heuristics byte for byte, and a session that
    /// never sees OSC 7 leaves the relative path undetected rather than guessing
    /// a directory. Both are existing, implemented, documented conventions
    /// (`docs/shell-integration.md`, "Authority and fallback"), which is why
    /// this variant costs three arms and no new mechanism.
    None,
}

/// Which door serves a profile's sessions, **as the editor's picker holds it**
/// (plan §1.6, §3.3) — derived from the program, or named outright.
///
/// Two states rather than one more [`Integration`] variant, because they answer
/// different questions and only one of them survives an edit to the `Program`
/// field: `Auto` is a *rule* — whatever this row runs, serve it the way that
/// family is served — while a named answer is a decision that outlives the
/// program it was made about. A row that stored the derived value would forget
/// which of the two it was the moment somebody pointed it somewhere else, and
/// the next `Program` edit would either silently keep a door that no longer fits
/// or silently throw away a choice.
///
/// **Every shipped profile is `Auto`**, and that is the derivation proving
/// itself rather than five constants standing beside it: `derive_integration`
/// reproduces the five doors this build has always opened, pinned by
/// `auto_derives_the_door_every_shipped_profile_has_always_had`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegrationChoice {
    /// Read the program's own file name — [`derive_integration`].
    Auto,
    /// This door, whatever the program becomes.
    Named(Integration),
}

/// Which door a program's file name asks for (plan §1.6).
///
/// **The file name and not a probe.** What serves a shell is decided by which
/// family it belongs to, and the name is the only thing about a program this
/// dialog can read without starting it — `--init-file` handed to something that
/// is not a bash is a filename it will try to open, and there is no way to ask
/// first that does not involve running it.
///
/// The stem rather than the whole leaf, so that `bash` and `bash.exe` are one
/// answer: a WSL distribution's login shell has no extension and a Windows one
/// does, and the family is the same family.
///
/// Anything this list has not heard of gets [`Integration::None`], which is the
/// honest answer and a whole one: a screen that never sees OSC 133 keeps the
/// cursor/WRAPLINE heuristics byte for byte, and a session that never sees
/// OSC 7 leaves the relative path undetected rather than guessing.
#[must_use]
pub fn derive_integration(program: &ProgramSource) -> Integration {
    let leaf = match program {
        // Not a file name at all — it is `BT_SHELL`, then a `pwsh` probe, and
        // every path it can resolve to is a PowerShell 7.
        ProgramSource::PowerShellSeven => return Integration::PowerShellOptIn,
        ProgramSource::Path(path) => path.file_name().map(std::ffi::OsStr::to_string_lossy),
        // Every candidate of one shipped row names one program family — the four
        // Git Bash places are four `bash.exe`s — so the first is the family, and
        // a row whose candidates disagreed would be a row that could not say what
        // it starts either.
        ProgramSource::FirstOf(candidates) => candidates
            .first()
            .map(|candidate| match candidate {
                ProgramCandidate::Under { tail, .. }
                | ProgramCandidate::BesideOnPath { tail, .. } => tail.as_str(),
            })
            .map(|tail| {
                std::borrow::Cow::Borrowed(tail.rsplit(['\\', '/']).next().unwrap_or_default())
            }),
    };
    let Some(leaf) = leaf else {
        return Integration::None;
    };
    let stem = leaf
        .rsplit_once('.')
        .map_or(leaf.as_ref(), |(stem, _)| stem)
        .to_ascii_lowercase();
    match stem.as_str() {
        "pwsh" | "powershell" => Integration::PowerShellOptIn,
        "bash" | "sh" | "zsh" | "wsl" => Integration::BashInitFile,
        "cmd" => Integration::CmdPrompt,
        _ => Integration::None,
    }
}

/// One row's door, resolved — the answer every caller outside the editor wants.
#[must_use]
pub fn served_by(profile: &Profile) -> Integration {
    match profile.integration {
        IntegrationChoice::Auto => derive_integration(&profile.program),
        IntegrationChoice::Named(named) => named,
    }
}

/// The profile whose PSReadLine is the one this product can repair.
///
/// Named rather than spelled at the two places that compare against it: the
/// invitation's trigger (`create_leaf_session`) and `psreadline.rs`'s own
/// header. `pwsh` ships a PSReadLine new enough to anchor itself and every
/// other profile is not a PowerShell at all, so this id is the whole of the
/// feature's audience.
pub const WINDOWS_POWERSHELL_ID: &str = "winps";

/// The five profiles this build ships, freshly built.
///
/// **The seed, not the table.** It is what a machine with no `profiles.json`
/// gets, in this order; it is what a built-in's `Restore all defaults` compares
/// against; and it is where every built-in's [`Profile::compared_title`] comes
/// from. What the window actually reads is [`table`], which is this merged with
/// the file.
///
/// A function and no longer a `const`, because the rows own their strings now
/// (§7.1.6c-6). The cost is five allocations at each call and the call sites are
/// startup, a restore and a test — the alternative, a second `&'static` struct
/// standing beside the owned one, would have made every row of this table exist
/// twice and put the next person who edits one in front of two places to edit.
#[must_use]
pub fn shipped() -> Vec<Profile> {
    vec![
        Profile {
            id: "pwsh".to_owned(),
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
            compared_title: Some("PowerShell 7".to_owned()),
            display_title: "PowerShell 7".to_owned(),
            mark: ChromeMark::ProfilePowerShell,
            program: ProgramSource::PowerShellSeven,
            // The flag this terminal has always passed, now said by the profile that
            // means it rather than by the spawn path every profile goes through.
            args: vec!["-NoLogo".to_owned()],
            env: Vec::new(),
            starting_dir: StartingDir::WindowsHome,
            start_at: StartAt::Inherit,
            paths: PathNamespace::Windows,
            qualifier: Qualifier::None,
            integration: IntegrationChoice::Auto,
            hidden: false,
            origin: Origin::Builtin,
        },
        Profile {
            id: WINDOWS_POWERSHELL_ID.to_owned(),
            // The qualifier was always this row's real name rather than one the list
            // invented; the version is the ruling above, and 5.1 is where this product
            // ends rather than where it happens to be.
            compared_title: Some("Windows PowerShell 5.1".to_owned()),
            display_title: "Windows PowerShell 5.1".to_owned(),
            // The same mark. The mock-up has one PowerShell symbol and drew no
            // second one, and there is nothing to invent: both rows start a
            // PowerShell, the blue tile is what "a PowerShell is here" looks like,
            // and the titles already say which. A second glyph would be this list
            // asserting a visual distinction the family does not have.
            mark: ChromeMark::ProfilePowerShell,
            // Not `PowerShellSeven`, and not a bare name either: this is the one
            // shell that is *part of Windows*, so it is named where Windows keeps
            // it. That is what lets [`fallback_profile()`] be this row — the probe
            // finds it on every Windows there is, so the floor under every other
            // profile is never itself greyed.
            program: ProgramSource::FirstOf(vec![ProgramCandidate::Under {
                variable: "SystemRoot".to_owned(),
                tail: r"System32\WindowsPowerShell\v1.0\powershell.exe".to_owned(),
            }]),
            args: vec!["-NoLogo".to_owned()],
            env: Vec::new(),
            starting_dir: StartingDir::WindowsHome,
            start_at: StartAt::Inherit,
            paths: PathNamespace::Windows,
            qualifier: Qualifier::None,
            // The same script, and it already handles this shell: `folio.ps1`
            // is written for 5.1 and 7 alike, and the PSReadLine 2.0.0 anchor repair
            // 5.1 needs is an existing no-op sentinel rather than a second code path.
            integration: IntegrationChoice::Auto,
            hidden: false,
            origin: Origin::Builtin,
        },
        Profile {
            id: "wsl".to_owned(),
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
            compared_title: Some("WSL".to_owned()),
            display_title: "WSL".to_owned(),
            mark: ChromeMark::ProfileUbuntu,
            program: ProgramSource::FirstOf(vec![ProgramCandidate::Under {
                variable: "SystemRoot".to_owned(),
                tail: r"System32\wsl.exe".to_owned(),
            }]),
            args: Vec::new(),
            env: Vec::new(),
            // The one profile whose home is not a Windows directory.
            starting_dir: StartingDir::LauncherFlag {
                flag: "--cd".to_owned(),
                home: "~".to_owned(),
            },
            start_at: StartAt::Inherit,
            paths: PathNamespace::Wsl,
            qualifier: Qualifier::WslDistribution,
            integration: IntegrationChoice::Auto,
            hidden: false,
            origin: Origin::Builtin,
        },
        Profile {
            id: "gitbash".to_owned(),
            compared_title: Some("Git Bash".to_owned()),
            display_title: "Git Bash".to_owned(),
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
            program: ProgramSource::FirstOf(vec![
                ProgramCandidate::BesideOnPath {
                    anchor: "git.exe".to_owned(),
                    tail: r"bin\bash.exe".to_owned(),
                },
                ProgramCandidate::Under {
                    variable: "ProgramFiles".to_owned(),
                    tail: r"Git\bin\bash.exe".to_owned(),
                },
                ProgramCandidate::Under {
                    variable: "ProgramFiles(x86)".to_owned(),
                    tail: r"Git\bin\bash.exe".to_owned(),
                },
                ProgramCandidate::Under {
                    variable: "LocalAppData".to_owned(),
                    tail: r"Programs\Git\bin\bash.exe".to_owned(),
                },
            ]),
            // `bin\bash.exe` is the MSYS wrapper the Git Bash shortcut itself runs,
            // and `--login -i` is that shortcut's own argument list: `--login` is
            // what sources `/etc/profile` and puts `git` on the path, and without it
            // this would be a bash that cannot find the tool it is named after.
            args: vec!["--login".to_owned(), "-i".to_owned()],
            env: Vec::new(),
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
            start_at: StartAt::Inherit,
            paths: PathNamespace::Windows,
            qualifier: Qualifier::None,
            integration: IntegrationChoice::Auto,
            hidden: false,
            origin: Origin::Builtin,
        },
        Profile {
            id: "cmd".to_owned(),
            compared_title: Some("Command Prompt".to_owned()),
            display_title: "Command Prompt".to_owned(),
            mark: ChromeMark::ProfileCmd,
            program: ProgramSource::FirstOf(vec![ProgramCandidate::Under {
                variable: "SystemRoot".to_owned(),
                tail: r"System32\cmd.exe".to_owned(),
            }]),
            // None. `cmd.exe` has no logo to suppress, and every switch it does take
            // (`/c`, `/k`) would end the session rather than start one.
            args: Vec::new(),
            env: Vec::new(),
            starting_dir: StartingDir::WindowsHome,
            start_at: StartAt::Inherit,
            paths: PathNamespace::Windows,
            qualifier: Qualifier::None,
            integration: IntegrationChoice::Auto,
            hidden: false,
            origin: Origin::Builtin,
        },
    ]
}

/// The profile table this window actually reads: [`shipped`] merged with
/// `profiles.json`, in the file's order, plus whatever the user has made.
///
/// **One table, and it lives here rather than on `Runtime`.** Every consumer of
/// a profile is already a free function in this module taking a `usize`
/// ([`title`], [`spawn_place`], [`revived_cwd`], [`index_of_id`]…) and two of
/// them are in other modules entirely (`restore.rs`, `shell_integration.rs`).
/// Threading a borrowed table through all of that would have made the table an
/// argument of forty signatures to serve one owner; [`crate::i18n::install`] set
/// the precedent for the shape used instead — read the file, install once,
/// before anything that measures a string exists.
///
/// Unlike the language, it can move afterwards: a reorder and a duplicate both
/// rewrite it, so the answer is behind a lock and every change advances
/// [`profile_revision`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileTable {
    profiles: Vec<Profile>,
}

impl ProfileTable {
    /// Every row, in the order every surface draws them.
    #[must_use]
    pub fn profiles(&self) -> &[Profile] {
        &self.profiles
    }

    /// How many rows there are — a runtime fact now, and the reason the two
    /// `[T; count()]` arrays this module used to hold became `Vec`s.
    #[must_use]
    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    /// One row, or `None` past the end.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Profile> {
        self.profiles.get(index)
    }

    /// Where a stable id sits **today**, or `None` if nothing holds it.
    #[must_use]
    pub fn position_of_id(&self, id: &str) -> Option<usize> {
        self.profiles.iter().position(|profile| profile.id == id)
    }

    /// The rows a picker offers, as table indices.
    ///
    /// Hidden rows are absent — that is the whole of what hiding means — and the
    /// indices are the **table's**, not the picker's own row numbers. A menu row
    /// that carried its own ordinal would name a different profile the moment
    /// something above it was hidden, and the answer it got wrong would be
    /// silent: the wrong shell would simply start.
    #[must_use]
    pub fn offered(&self) -> Vec<usize> {
        (0..self.profiles.len())
            .filter(|index| !self.profiles[*index].hidden)
            .collect()
    }
}

/// The table in force, and the number that says how many times it has moved.
///
/// **A type and not two loose statics**, and that is what makes the moving parts
/// testable: `cargo test` runs this crate's cases in parallel in one process, so
/// a case that reordered the *process's* table for a microsecond would race
/// every other case that asks how many profiles there are. The tests below build
/// their own [`Registry`] and move that. It is `crate::i18n`'s ruling verbatim —
/// "these build their own `Current` rather than moving the process's, and that
/// is a decision and not a shortcut" — met again one table over.
struct Registry {
    /// `Arc` rather than a guard handed to callers: a draw pass reads a title, a
    /// mark and a command line from three places inside one frame, and a lock
    /// guard alive across all of that is a lock guard alive across a re-entrant
    /// call into this module. A refcount bump per read is much the cheaper half.
    table: RwLock<Arc<ProfileTable>>,
    /// [`crate::i18n::lang_revision`]'s twin, feeding `LayoutKey` for the
    /// identical reason. A profile's name is a **width**: the `˅` menu's rows,
    /// the pane submenu's, the settings combo's column and every tab that falls
    /// back to its profile's name are measured and cached, so a reorder or a
    /// duplicate that did not advance this would be a window drawing yesterday's
    /// widths under today's words.
    revision: AtomicU64,
}

impl Registry {
    fn shipped() -> Self {
        Self {
            table: RwLock::new(Arc::new(ProfileTable {
                profiles: shipped(),
            })),
            revision: AtomicU64::new(0),
        }
    }

    fn table(&self) -> Arc<ProfileTable> {
        self.table
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }

    /// Put a new table in force, and answer whether anything moved.
    ///
    /// **A table equal to the one in force advances nothing.** A probe that
    /// found the same programs, or a press that moved the first row up, has not
    /// changed a width, and a revision that ticked anyway would throw away every
    /// measured string in the window for nothing.
    fn publish(&self, profiles: Vec<Profile>) -> bool {
        let mut held = self
            .table
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if held.profiles == profiles {
            return false;
        }
        *held = Arc::new(ProfileTable { profiles });
        drop(held);
        self.revision.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// [`install`]'s body, over one registry.
    fn install(&self, file: &ProfilesV1) -> Vec<ProfileFault> {
        let (profiles, faults) = merge(shipped(), file);
        self.publish(profiles);
        faults
    }

    /// [`move_profile`]'s body.
    fn move_profile(&self, index: usize, down: bool) -> bool {
        let mut profiles = self.table().profiles.clone();
        if index >= profiles.len() {
            return false;
        }
        let other = if down {
            Some(index + 1).filter(|next| *next < profiles.len())
        } else {
            index.checked_sub(1)
        };
        let Some(other) = other else { return false };
        profiles.swap(index, other);
        self.publish(profiles)
    }

    /// [`duplicate`]'s body.
    fn duplicate(&self, index: usize) -> Option<usize> {
        let mut profiles = self.table().profiles.clone();
        let source = profiles.get(index)?.clone();
        let display_title = copy_title(&source.display_title, &profiles);
        let id = fresh_id(&display_title, &profiles);
        let copy = Profile {
            id,
            // No script this build ships will ever announce a name this build
            // did not choose, so a copy has no second word to be compared
            // against — see `announcement_set`.
            compared_title: None,
            display_title,
            // **A copy carries no machine qualifier**, which is what `compose`
            // already says about every profile of the reader's own: a copy of
            // WSL has pinned whatever distribution it meant in its own
            // arguments, and appending the machine's *default* distribution to
            // it would be a title naming the wrong one. Said here as well as
            // there because the two are one row read twice — before the file is
            // written and after it is read back — and a row that renamed itself
            // across a restart would be the table disagreeing with itself.
            qualifier: Qualifier::None,
            origin: Origin::User,
            hidden: false,
            ..source
        };
        let at = index + 1;
        profiles.insert(at, copy);
        self.publish(profiles);
        Some(at)
    }

    /// Every editor field's body: read the table, change one row, put it back.
    ///
    /// **One door for all of them**, because every field in this dialog writes
    /// the instant it is changed (§7.1.6c-4a: no dirty gate, nothing to save) and
    /// a second spelling of "clone, mutate, publish" is a second place for the
    /// revision to be forgotten. `change` answers whether it changed anything, so
    /// that a field re-writing the value it already held does not tick a
    /// revision and throw away every measured string in the window.
    fn edit(&self, index: usize, change: impl FnOnce(&mut Profile) -> bool) -> bool {
        let mut profiles = self.table().profiles.clone();
        let Some(profile) = profiles.get_mut(index) else {
            return false;
        };
        if !change(profile) {
            return false;
        }
        self.publish(profiles)
    }

    /// [`rename`]'s body.
    fn rename(&self, index: usize, title: &str) -> NameVerdict {
        let title = title.trim();
        if title.is_empty() {
            return NameVerdict::Blank;
        }
        if self
            .table()
            .profiles()
            .iter()
            .enumerate()
            .any(|(other, profile)| other != index && profile.display_title == title)
        {
            return NameVerdict::Taken;
        }
        self.edit(index, |profile| {
            if profile.display_title == title {
                return false;
            }
            profile.display_title = title.to_owned();
            true
        });
        NameVerdict::Written
    }

    /// [`set_colour`]'s body.
    fn set_colour(&self, index: usize, colour: MarkColour) -> bool {
        self.edit(index, |profile| {
            if profile.origin != Origin::User {
                return false;
            }
            let mark = ChromeMark::ProfileGeneric { colour };
            if profile.mark == mark {
                return false;
            }
            profile.mark = mark;
            true
        })
    }

    /// [`set_hidden`]'s body — the two guards over this table's own floor rather
    /// than over the process's, which is what lets a test hide a row without
    /// moving the window's answer to "which profile is the floor".
    fn set_hidden(&self, index: usize, hidden: bool, default: usize) -> bool {
        let floor = self
            .table()
            .position_of_id(WINDOWS_POWERSHELL_ID)
            .unwrap_or(0);
        if hidden && (index == default || index == floor) {
            return false;
        }
        self.edit(index, |profile| {
            if profile.hidden == hidden {
                return false;
            }
            profile.hidden = hidden;
            true
        })
    }

    /// [`delete`]'s body.
    fn delete(&self, index: usize) -> Option<Profile> {
        let mut profiles = self.table().profiles.clone();
        // A built-in cannot be deleted — a row that is missing looks exactly
        // like a row that was never designed — and the floor cannot be, whatever
        // its origin, because a floor with a hole in it is not a floor.
        if profiles.get(index)?.origin != Origin::User {
            return None;
        }
        let removed = profiles.remove(index);
        self.publish(profiles);
        Some(removed)
    }

    /// [`reinsert`]'s body — the Undo toast's other half.
    fn reinsert(&self, profile: Profile, at: usize) -> usize {
        let mut profiles = self.table().profiles.clone();
        let at = at.min(profiles.len());
        profiles.insert(at, profile);
        self.publish(profiles);
        at
    }

    /// [`restore_defaults`]'s body.
    fn restore_defaults(&self, index: usize) -> bool {
        let mut profiles = self.table().profiles.clone();
        let Some(profile) = profiles.get(index) else {
            return false;
        };
        let Some(seed) = shipped()
            .into_iter()
            .find(|shipped| shipped.id == profile.id)
        else {
            return false;
        };
        // The row's *place* is not one of its defaults. Reordering is a decision
        // about the list and restoring is a decision about one profile, and a
        // verb that quietly did both would be one press undoing two things.
        //
        // Nor is `hidden`: a hidden row's foot verb is reached by opening the
        // row that is dimmed in the list, and having it reappear in the picker
        // would be this verb answering a question nobody asked it.
        let hidden = profile.hidden;
        profiles[index] = Profile { hidden, ..seed };
        self.publish(profiles)
    }

    /// [`create`]'s body.
    fn create(&self, template: usize) -> Option<usize> {
        let mut profiles = self.table().profiles.clone();
        let source = profiles.get(template)?.clone();
        let display_title = copy_title(&source.display_title, &profiles);
        let id = fresh_id(&display_title, &profiles);
        let made = Profile {
            id,
            compared_title: None,
            display_title,
            // **A new profile wears the chassis, and a duplicate wears the
            // brand.** The two verbs say different things: `Duplicate` says
            // "another one of these", and its copy really is a PowerShell, so
            // the mark is telling the truth (5a's own ruling). `New profile`
            // takes the default only as a *template* for what to run, and the
            // first thing anybody does with it is point it somewhere else — at
            // which moment a Microsoft blue would be a brand on a program that
            // is not theirs.
            mark: ChromeMark::ProfileGeneric {
                colour: unworn_colour(&profiles),
            },
            origin: Origin::User,
            hidden: false,
            ..source
        };
        // At the end, where `+ New profile` stands. A duplicate lands under its
        // original because that is where the reader who pressed it is looking;
        // nobody is looking at the default profile's row when they press the
        // foot's verb.
        let at = profiles.len();
        profiles.push(made);
        self.publish(profiles);
        Some(at)
    }

    /// [`to_file`]'s body.
    fn to_file(&self) -> ProfilesV1 {
        let shipped = shipped();
        ProfilesV1 {
            schema_version: PROFILES_SCHEMA_VERSION,
            profiles: self
                .table()
                .profiles
                .iter()
                .map(|profile| {
                    entry_for(profile, shipped.iter().find(|seed| seed.id == profile.id))
                })
                .collect(),
        }
    }
}

/// The process's own registry — the table this window reads.
static REGISTRY: OnceLock<Registry> = OnceLock::new();

fn registry() -> &'static Registry {
    REGISTRY.get_or_init(Registry::shipped)
}

/// The table as it stands. Cheap — a refcount bump.
#[must_use]
pub fn table() -> Arc<ProfileTable> {
    registry().table()
}

/// Read one thing out of the table without cloning a row.
fn with_table<R>(read: impl FnOnce(&ProfileTable) -> R) -> R {
    read(&table())
}

/// How many times the table has moved. Into `LayoutKey`, beside `lang_rev`.
#[must_use]
pub fn profile_revision() -> u64 {
    registry().revision()
}

/// What the reader had to refuse, so somebody can be told once.
///
/// `schemes`' register applied to a table: skip the entry, name it, say it once,
/// never crash, never go quiet. A file with two bad rows is two things to fix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileFault {
    /// Neither a built-in id nor a startable profile: an entry the file invented
    /// and gave no program to. A row that cannot start is not a row.
    Unusable { id: String },
    /// A second entry claiming an id an earlier entry already took. The first
    /// wins, because a stable id has to name one thing and the seeds on disk
    /// pointing at it were written when only the first existed.
    Duplicate { id: String },
}

/// Read `profiles.json` onto the shipped table and put the result in force.
///
/// The rules, each with a red gate of its own:
///
/// * **The array is the order.** There is no `order` key — two places saying the
///   same thing drift.
/// * **A built-in entry writes only its differences.** `{ "id": "pwsh" }` is the
///   shipped profile, unchanged, in that position.
/// * **A shipped id the file never names is appended, visible.** That is the
///   only honest answer to a build that grew a sixth built-in: it appears, at
///   the end, not hidden.
/// * **An entry that is neither a built-in nor has a program is dropped and
///   named.** The rest of the file still lands.
/// * **No file at all is the shipped five in shipped order**, byte for byte what
///   this window did before this slice existed. Nothing is written until
///   something is changed — a feature does not announce itself by putting an
///   empty document in everybody's `%APPDATA%`.
pub fn install(file: &ProfilesV1) -> Vec<ProfileFault> {
    registry().install(file)
}

/// [`install`]'s pure half, so the rules can be tested without a process-wide
/// table under them.
fn merge(shipped: Vec<Profile>, file: &ProfilesV1) -> (Vec<Profile>, Vec<ProfileFault>) {
    let mut faults = Vec::new();
    let mut built: Vec<Profile> = Vec::new();
    for entry in &file.profiles {
        if built.iter().any(|profile| profile.id == entry.id) {
            faults.push(ProfileFault::Duplicate {
                id: entry.id.clone(),
            });
            continue;
        }
        let seed = shipped.iter().find(|profile| profile.id == entry.id);
        let Some(profile) = compose(seed, entry) else {
            faults.push(ProfileFault::Unusable {
                id: entry.id.clone(),
            });
            continue;
        };
        built.push(profile);
    }
    for profile in shipped {
        if !built.iter().any(|held| held.id == profile.id) {
            built.push(profile);
        }
    }
    (built, faults)
}

/// One file entry onto one shipped profile, or onto nothing.
fn compose(seed: Option<&Profile>, entry: &ProfileEntryV1) -> Option<Profile> {
    let mut profile = match seed {
        Some(shipped) => shipped.clone(),
        None => Profile {
            id: entry.id.clone(),
            // A profile of the user's own has no shipped name, so there is no
            // second string for an announcement to be compared against.
            compared_title: None,
            display_title: entry.id.clone(),
            // The neutral chassis in its neutral grey — the mock-up's own
            // `#p-shell` is `#p-cmd`'s shape in another fill — because a row
            // that has not said what it is must not borrow somebody's brand to
            // say it. The eight struck colours are the editor's.
            mark: ChromeMark::ProfileCmd,
            program: entry.program.as_ref().map(program_from_file)?,
            args: Vec::new(),
            env: Vec::new(),
            starting_dir: StartingDir::WindowsHome,
            start_at: StartAt::Inherit,
            paths: PathNamespace::Windows,
            // A machine fact, and a profile the user wrote has already pinned
            // whatever distribution it meant in its own arguments. Saying it
            // twice is saying it once and once wrong.
            qualifier: Qualifier::None,
            // **The rule and not an answer.** A row of the reader's own is born
            // pointing at a program, and `Auto` reads that program: a
            // `claude.exe` gets no door, a `bash.exe` gets the init file, and a
            // row repointed tomorrow is served the way tomorrow's program is
            // served. Writing `None` in here would have been a decision nobody
            // made, kept forever.
            integration: IntegrationChoice::Auto,
            hidden: false,
            origin: Origin::User,
        },
    };
    if let Some(title) = &entry.display_title {
        profile.display_title.clone_from(title);
    }
    if let Some(program) = &entry.program {
        profile.program = program_from_file(program);
    }
    if let Some(args) = &entry.args {
        profile.args.clone_from(args);
    }
    if let Some(env) = &entry.env {
        profile.env = env
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
    }
    if let Some(starting_dir) = &entry.starting_dir {
        profile.starting_dir = starting_dir_from_file(starting_dir);
    }
    if let Some(start_at) = &entry.start_at {
        profile.start_at = start_at_from_file(start_at);
    }
    // The five identity colours are not this product's to repaint (S98/S31), so
    // a `mark` key is read for a profile of the user's own and ignored on a
    // built-in — the file cannot say what the dialog will not offer.
    if profile.origin == Origin::User
        && let Some(mark) = &entry.mark
        && let Some(named) = mark_from_file(mark)
    {
        profile.mark = named;
    }
    if let Some(integration) = &entry.integration
        && let Some(named) = integration_from_file(integration)
    {
        profile.integration = named;
    }
    // **Derived for every row, built-in included** (§7.1.6c-6c). It used to be
    // derived for a profile of the reader's own and stated for the five, which
    // was two rules for one fact and left a built-in repointed at another
    // program still translating directories in the namespace of the one it no
    // longer runs. The derivation reproduces all five shipped answers —
    // `the_namespace_every_shipped_profile_states_is_the_one_it_derives` — so
    // what this replaces is a copy and not a decision.
    profile.paths = derived_paths(&profile);
    profile.hidden = entry.hidden;
    Some(profile)
}

/// Which spelling of a path a profile's shell speaks — **derived, never stated**
/// (plan §1.6).
///
/// It is a property of the program and not a taste: choose it wrong and a
/// directory inherited from another pane is silently translated into
/// `/mnt/c/...` or into somewhere that does not exist. Only `wsl.exe` behind a
/// bash init file crosses the namespace, which is why this asks both questions
/// and not either one.
fn derived_paths(profile: &Profile) -> PathNamespace {
    let names_the_launcher = |tail: &str| tail.to_ascii_lowercase().ends_with("wsl.exe");
    let launcher = match &profile.program {
        ProgramSource::Path(path) => path
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("wsl.exe")),
        ProgramSource::FirstOf(candidates) => candidates.iter().any(|candidate| match candidate {
            ProgramCandidate::Under { tail, .. } | ProgramCandidate::BesideOnPath { tail, .. } => {
                names_the_launcher(tail)
            }
        }),
        ProgramSource::PowerShellSeven => false,
    };
    if launcher && served_by(profile) == Integration::BashInitFile {
        PathNamespace::Wsl
    } else {
        PathNamespace::Windows
    }
}

fn program_from_file(program: &ProgramV1) -> ProgramSource {
    match program {
        ProgramV1::Path(path) => ProgramSource::Path(PathBuf::from(path)),
        ProgramV1::Resolution(ResolutionV1::PowerShellSeven) => ProgramSource::PowerShellSeven,
        ProgramV1::Resolution(ResolutionV1::FirstOf { candidates }) => ProgramSource::FirstOf(
            candidates
                .iter()
                .map(|candidate| match candidate {
                    CandidateV1::Under { variable, tail } => ProgramCandidate::Under {
                        variable: variable.clone(),
                        tail: tail.clone(),
                    },
                    CandidateV1::BesideOnPath { anchor, tail } => ProgramCandidate::BesideOnPath {
                        anchor: anchor.clone(),
                        tail: tail.clone(),
                    },
                })
                .collect(),
        ),
    }
}

fn program_to_file(program: &ProgramSource) -> ProgramV1 {
    match program {
        ProgramSource::Path(path) => ProgramV1::Path(path.to_string_lossy().into_owned()),
        ProgramSource::PowerShellSeven => ProgramV1::Resolution(ResolutionV1::PowerShellSeven),
        ProgramSource::FirstOf(candidates) => ProgramV1::Resolution(ResolutionV1::FirstOf {
            candidates: candidates
                .iter()
                .map(|candidate| match candidate {
                    ProgramCandidate::Under { variable, tail } => CandidateV1::Under {
                        variable: variable.clone(),
                        tail: tail.clone(),
                    },
                    ProgramCandidate::BesideOnPath { anchor, tail } => CandidateV1::BesideOnPath {
                        anchor: anchor.clone(),
                        tail: tail.clone(),
                    },
                })
                .collect(),
        }),
    }
}

fn starting_dir_from_file(starting_dir: &StartingDirV1) -> StartingDir {
    match starting_dir {
        StartingDirV1::Named(NamedStartingDirV1::WindowsHome) => StartingDir::WindowsHome,
        StartingDirV1::LauncherFlag { flag, home } => StartingDir::LauncherFlag {
            flag: flag.clone(),
            home: home.clone(),
        },
    }
}

fn starting_dir_to_file(starting_dir: &StartingDir) -> StartingDirV1 {
    match starting_dir {
        StartingDir::WindowsHome => StartingDirV1::Named(NamedStartingDirV1::WindowsHome),
        StartingDir::LauncherFlag { flag, home } => StartingDirV1::LauncherFlag {
            flag: flag.clone(),
            home: home.clone(),
        },
    }
}

fn start_at_from_file(start_at: &StartAtV1) -> StartAt {
    match start_at {
        StartAtV1::Named(NamedStartAtV1::Inherit) => StartAt::Inherit,
        StartAtV1::Named(NamedStartAtV1::Home) => StartAt::Home,
        StartAtV1::Fixed { fixed } => StartAt::Fixed(PathBuf::from(fixed)),
    }
}

fn start_at_to_file(start_at: &StartAt) -> StartAtV1 {
    match start_at {
        StartAt::Inherit => StartAtV1::Named(NamedStartAtV1::Inherit),
        StartAt::Home => StartAtV1::Named(NamedStartAtV1::Home),
        StartAt::Fixed(path) => StartAtV1::Fixed {
            fixed: path.to_string_lossy().into_owned(),
        },
    }
}

/// The wire words for the shipped marks.
///
/// A plain name and not the sprite's Rust spelling: `profiles.json` is read by
/// people, and `ProfileUbuntu` is this build's private word for it. `shell` is
/// the neutral chassis — the one `cmd` wears and the one a profile of the user's
/// own gets — named for what it is rather than for the profile it came from.
/// The neutral chassis's wire word, in both directions — `#p-shell`, the
/// drawing `#p-pwsh` and `#p-cmd` already are twice over.
///
/// One value and not an enum with one variant, because a second chassis would
/// be a second *drawing* and there is not one: the mock-up struck one shape for
/// "a shell of your own" and the eight colours are what tell two of them apart.
/// The key is in the file anyway (plan §1.2's own worked example writes it) so
/// that a second one, if it is ever drawn, arrives as a value rather than as a
/// schema version.
const GENERIC_CHASSIS: &str = "shell";

fn mark_from_file(mark: &MarkV1) -> Option<ChromeMark> {
    match mark {
        MarkV1::Named(name) => match name.as_str() {
            "powershell" => Some(ChromeMark::ProfilePowerShell),
            "ubuntu" => Some(ChromeMark::ProfileUbuntu),
            "git" => Some(ChromeMark::ProfileGit),
            // The word a duplicate of `cmd` writes, and it keeps meaning what it
            // meant in 5a: the Command Prompt's own charcoal panel, because a
            // copy of a Command Prompt really is one. A profile drawn from
            // nothing wears the object form below instead.
            "shell" => Some(ChromeMark::ProfileCmd),
            _ => None,
        },
        MarkV1::Generic { chassis, colour } if chassis == GENERIC_CHASSIS => {
            MarkColour::from_wire(colour).map(|colour| ChromeMark::ProfileGeneric { colour })
        }
        MarkV1::Generic { .. } => None,
    }
}

fn mark_to_file(mark: ChromeMark) -> Option<MarkV1> {
    match mark {
        ChromeMark::ProfilePowerShell => Some(MarkV1::Named("powershell".to_owned())),
        ChromeMark::ProfileUbuntu => Some(MarkV1::Named("ubuntu".to_owned())),
        ChromeMark::ProfileGit => Some(MarkV1::Named("git".to_owned())),
        ChromeMark::ProfileCmd => Some(MarkV1::Named("shell".to_owned())),
        ChromeMark::ProfileGeneric { colour } => Some(MarkV1::Generic {
            chassis: GENERIC_CHASSIS.to_owned(),
            colour: colour.wire().to_owned(),
        }),
        _ => None,
    }
}

/// `"auto"` is a fifth word and not an absent key, because absence already
/// means something else here and means it in two different ways: on a built-in
/// it is "whatever this build ships", and on a row of the reader's own it is
/// "whatever the reader's own defaults are". A rule the reader chose has to be
/// writable, or a row switched back to `Auto` would round-trip as the door it
/// happened to be on.
fn integration_from_file(name: &str) -> Option<IntegrationChoice> {
    match name {
        "auto" => Some(IntegrationChoice::Auto),
        "powershell" => Some(IntegrationChoice::Named(Integration::PowerShellOptIn)),
        "bash" => Some(IntegrationChoice::Named(Integration::BashInitFile)),
        "cmd" => Some(IntegrationChoice::Named(Integration::CmdPrompt)),
        "none" => Some(IntegrationChoice::Named(Integration::None)),
        _ => None,
    }
}

fn integration_to_file(integration: IntegrationChoice) -> &'static str {
    match integration {
        IntegrationChoice::Auto => "auto",
        IntegrationChoice::Named(Integration::PowerShellOptIn) => "powershell",
        IntegrationChoice::Named(Integration::BashInitFile) => "bash",
        IntegrationChoice::Named(Integration::CmdPrompt) => "cmd",
        IntegrationChoice::Named(Integration::None) => "none",
    }
}

/// The table as `profiles.json` would write it — **differences only**.
///
/// A built-in equal to the shipped one in every respect is one key, its id, and
/// that is what lets a later build retune it for everybody who never touched it.
#[must_use]
pub fn to_file() -> ProfilesV1 {
    registry().to_file()
}

fn entry_for(profile: &Profile, seed: Option<&Profile>) -> ProfileEntryV1 {
    let env = (!profile.env.is_empty()).then(|| {
        profile
            .env
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect()
    });
    match seed {
        Some(seed) => ProfileEntryV1 {
            id: profile.id.clone(),
            display_title: (profile.display_title != seed.display_title)
                .then(|| profile.display_title.clone()),
            hidden: profile.hidden,
            program: (profile.program != seed.program).then(|| program_to_file(&profile.program)),
            args: (profile.args != seed.args).then(|| profile.args.clone()),
            env,
            starting_dir: (profile.starting_dir != seed.starting_dir)
                .then(|| starting_dir_to_file(&profile.starting_dir)),
            start_at: (profile.start_at != seed.start_at)
                .then(|| start_at_to_file(&profile.start_at)),
            // A built-in's colour is not the file's to state, because it is not
            // the dialog's to change.
            mark: None,
            integration: (profile.integration != seed.integration)
                .then(|| integration_to_file(profile.integration).to_owned()),
        },
        None => ProfileEntryV1 {
            id: profile.id.clone(),
            display_title: Some(profile.display_title.clone()),
            hidden: profile.hidden,
            program: Some(program_to_file(&profile.program)),
            args: (!profile.args.is_empty()).then(|| profile.args.clone()),
            env,
            starting_dir: Some(starting_dir_to_file(&profile.starting_dir)),
            start_at: Some(start_at_to_file(&profile.start_at)),
            mark: mark_to_file(profile.mark),
            integration: Some(integration_to_file(profile.integration).to_owned()),
        },
    }
}

/// Move one row one place, and say whether anything moved.
///
/// Buttons and not a drag (plan §2.5): this dialog's keyboard model is a Tab
/// order over targets and a drag has no keyboard equivalent; the list is five to
/// ten rows, where a drag's only advantage — moving item thirty to position two
/// — does not arise; and this window's chrome is already dense with drag targets,
/// so a third grammar of dragging inside a modal floating over them is a gesture
/// collision.
pub fn move_profile(index: usize, down: bool) -> bool {
    registry().move_profile(index, down)
}

/// Copy one row, and answer where the copy landed.
///
/// The copy sits **directly under its original**, which is where somebody who
/// just pressed `Duplicate` on a row is looking, and it takes the original's
/// mark because a copy of a PowerShell really is a PowerShell. Its
/// [`Profile::compared_title`] is `None`: no script this build ships will ever
/// announce a name this build did not choose.
pub fn duplicate(index: usize) -> Option<usize> {
    registry().duplicate(index)
}

/// Make a profile of the reader's own from the default as a template, and
/// answer where it landed — the foot's `+ New profile` (plan §2.1).
///
/// Not a blank one: a profile with no program is a row that cannot start, and
/// this block's default state is meant to be foolproof. Not a menu of templates
/// either — every row's `Duplicate` already is that, and one verb behind two
/// doors is the thing this house keeps deleting.
pub fn create(template: usize) -> Option<usize> {
    registry().create(template)
}

/// The first of the eight no profile in this table is wearing, or the first of
/// the eight.
///
/// Deterministic and not random, for `fresh_id`'s opposite reason: an id only
/// has to be *unlikely* to collide, while two rows in the same list wearing the
/// same colour is the exact failure a colour is there to prevent. Walking round
/// after eight is honest — a ninth profile has to share with somebody, and
/// sharing with the oldest is the least surprising choice.
fn unworn_colour(profiles: &[Profile]) -> MarkColour {
    let worn = |candidate: MarkColour| {
        profiles
            .iter()
            .any(|profile| profile.mark == ChromeMark::ProfileGeneric { colour: candidate })
    };
    MarkColour::ALL
        .into_iter()
        .find(|colour| !worn(*colour))
        .unwrap_or(MarkColour::ALL[0])
}

/// What happened to a name the editor's field was asked to write.
///
/// A verdict and not a `bool`, because the two refusals are different sentences
/// and the field has to say which: this dialog writes on every keystroke's worth
/// of change, so a refusal is a state the reader is standing in rather than an
/// error they submitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameVerdict {
    /// The table has it.
    Written,
    /// Nothing, or nothing but spaces. A row has to be nameable — it is what a
    /// tab, a picker and this list all draw — and an empty one would be a blank
    /// line in three surfaces.
    Blank,
    /// Another row already draws exactly this. **Refused rather than allowed**,
    /// and the argument is the identity model's own: a row is told apart by its
    /// mark and its name, and two rows called `PowerShell 7` standing one line
    /// apart in a picker is precisely the failure that made the two PowerShells
    /// carry their versions (user ruling 2026-08-11). The refusal is exact and
    /// not fuzzy — `powershell 7` and `PowerShell  7` are different names and
    /// the reader may mean either — because a rule a person cannot predict is
    /// worse than no rule.
    Taken,
}

/// Rename one row — the editor's `Name` field, which writes
/// [`Profile::display_title`] and never the string an announcement is compared
/// against (§G S103, plan §1.4).
///
/// A built-in is renamed too (user ruling 2026-08-17, Q2 = b): "give PowerShell 7
/// a `-NoProfile`" is the commonest thing anybody does to this table, and forcing
/// a duplicate for it would make two rows called PowerShell — while the
/// uniqueness of a row is half of this product's identity model. Its shipped
/// [`Profile::compared_title`] is untouched and invisible, so `folio.ps1`'s
/// announcement is still recognised as an echo after the rename.
pub fn rename(index: usize, title: &str) -> NameVerdict {
    registry().rename(index, title)
}

/// Point one row at a program on this machine — the editor's `Program` field and
/// its `Browse…`.
///
/// A typed or browsed path is always a [`ProgramSource::Path`], including on a
/// built-in: the shipped resolutions are *orders of search* (`BT_SHELL`, then a
/// probe), and a path is the answer that search was for. The row's capability
/// sentence re-derives from it because [`capability_text`] reads the
/// integration and the namespace rather than the id.
pub fn set_program_path(index: usize, path: &Path) -> bool {
    registry().edit(index, |profile| {
        let program = ProgramSource::Path(path.to_path_buf());
        if profile.program == program {
            return false;
        }
        profile.program = program;
        // A program is what decides which spelling of a path this profile's
        // shell speaks, and the namespace is derived and never stated (plan
        // §1.6) — so it is re-derived here rather than left describing the
        // program that used to be in this row.
        profile.paths = derived_paths(profile);
        true
    })
}

/// Which of the three answers a new leaf of this row takes.
pub fn set_start_at(index: usize, start_at: StartAt) -> bool {
    registry().edit(index, |profile| {
        if profile.start_at == start_at {
            return false;
        }
        profile.start_at = start_at;
        true
    })
}

/// Repaint one row's mark — **a profile of the reader's own only**.
///
/// The five shipped colours are not this product's to repaint (S98/S31: the blue
/// is Microsoft's and the orange is Ubuntu's), which is the same ruling that
/// stopped a custom colour scheme repainting them, and it is enforced here
/// rather than only in the dialog: a rule that lives in a control is a rule a
/// hand-edited file walks around.
pub fn set_colour(index: usize, colour: MarkColour) -> bool {
    registry().set_colour(index, colour)
}

/// The words handed to the program ahead of anything the shell reads.
pub fn set_args(index: usize, args: Vec<String>) -> bool {
    registry().edit(index, |profile| {
        if profile.args == args {
            return false;
        }
        profile.args = args;
        true
    })
}

/// What this row sets in its sessions' environment, over what the terminal sets
/// for itself.
///
/// **Read at spawn, last** (plan §1.7) — see [`Profile::env`] for the three
/// layers and for why a row here beats the terminal's own declaration.
pub fn set_env(index: usize, env: Vec<(String, String)>) -> bool {
    registry().edit(index, |profile| {
        if profile.env == env {
            return false;
        }
        profile.env = env;
        true
    })
}

/// Keep one row out of the pickers, or put it back — the `⋯` menu's `Hide` and
/// `Show`.
///
/// Two rows refuse to be hidden and the refusals are guards rather than
/// politeness (plan §2.4, R5): the **default** cannot be hidden because hiding it
/// leaves no new tab to open, and the **fallback floor** cannot, because every
/// degradation in this product lands on it and a floor that can be taken away is
/// a chain with a hole in the bottom. `default` is passed in rather than read,
/// because which row is the default is `settings.json`'s answer resolved against
/// this machine ([`default_profile`]) and not a fact this table holds.
pub fn set_hidden(index: usize, hidden: bool, default: usize) -> bool {
    registry().set_hidden(index, hidden, default)
}

/// Take one row out of the table and hand it back whole, so an Undo can put it
/// back — the `⋯` menu's `Delete` and the editor's foot verb.
///
/// **Immediate, with an undo, and no confirmation** (plan §2.3, ruling 3): this
/// dialog has no dirty gate to route a question through and every choice in it is
/// written the instant it is made, so what deletion is owed is not a second
/// question but a way back — which is the register `Ctrl+Shift+T` already struck
/// in this product, where a confirmation would be the first modal over a modal.
///
/// The whole row comes back rather than a recipe for rebuilding it, because the
/// one thing that must survive is the [`Profile::id`]: every seed on disk naming
/// this profile is pointing at that string, and a rebuilt row with a fresh
/// suffix would leave all of them degraded to the floor.
pub fn delete(index: usize) -> Option<Profile> {
    registry().delete(index)
}

/// Put a deleted row back where it was, and answer where that turned out to be.
///
/// Clamped to the end rather than refused, because the list may have moved under
/// the toast — a reorder, or a second deletion — and a row that came back at the
/// end is a row that came back.
pub fn reinsert(profile: Profile, at: usize) -> usize {
    registry().reinsert(profile, at)
}

/// Put one built-in back to the table this build ships — the editor's foot verb
/// on a built-in (`Restore all defaults`).
///
/// Its position and its hidden flag are not defaults: both are decisions about
/// the *list* rather than about this profile, and one press undoing two things
/// is what this house keeps taking apart.
pub fn restore_defaults(index: usize) -> bool {
    registry().restore_defaults(index)
}

/// Why a built-in's `Colour` row is dark, interned so the dialog's `Copy`
/// snapshot can carry it.
///
/// It names the profile it is standing on rather than saying "a built-in",
/// because that is what the mock-up writes (`PowerShell's mark is its own`) and
/// because the sentence is about a specific brand: Microsoft's blue and Ubuntu's
/// orange are not this product's to repaint (S98/S31).
#[must_use]
pub fn mark_is_its_own(index: usize) -> &'static str {
    intern(&crate::i18n::profile_mark_is_its_own(title(index)))
}

/// What one door is called on the `Shell integration` picker.
#[must_use]
pub fn integration_name(integration: Integration) -> crate::i18n::Text {
    match integration {
        Integration::PowerShellOptIn => crate::i18n::Text::ProfilesIntegrationPowerShell,
        Integration::BashInitFile => crate::i18n::Text::ProfilesIntegrationBash,
        Integration::CmdPrompt => crate::i18n::Text::ProfilesIntegrationCmd,
        Integration::None => crate::i18n::Text::ProfilesIntegrationNone,
    }
}

/// `Auto (Bash init file)` — that picker's **button** while the row is on the
/// rule, interned so the caption can ride a `Copy` snapshot of the page.
///
/// `None` when the row has named a door: the button then says the word on the
/// item that is ticked, which is what every other picker in this dialog does.
#[must_use]
pub fn integration_auto_label(index: usize) -> Option<&'static str> {
    with_table(|table| {
        let profile = table.get(index)?;
        if profile.integration != IntegrationChoice::Auto {
            return None;
        }
        let door = integration_name(derive_integration(&profile.program)).text();
        Some(intern(&crate::i18n::profile_integration_auto(door)))
    })
}

/// A path the dialog has to draw, interned — the fixed starting folder, which is
/// this profile's value and therefore what its picker's button says.
///
/// Through the same table [`title`] uses, and for its reason: `SettingsValues`
/// is compared for equality every frame and holds no owned strings, so a path
/// that has to reach it has to be `&'static`. The table is keyed on the string
/// itself, so a folder chosen twice is interned once.
#[must_use]
pub fn intern_path(path: &Path) -> &'static str {
    intern(&path.to_string_lossy())
}

/// Whether one row is a profile of the reader's own — which is what decides
/// whether it can be deleted, whether its colour is theirs, and which of the two
/// verbs its editor's foot carries.
#[must_use]
pub fn is_user(index: usize) -> bool {
    with_table(|table| {
        table
            .get(index)
            .is_some_and(|profile| profile.origin == Origin::User)
    })
}

/// One row's `start_at`, `env`, colour and hidden flag, for a dialog that has to
/// draw them.
#[must_use]
pub fn start_at(index: usize) -> StartAt {
    with_table(|table| {
        table
            .get(index)
            .map_or(StartAt::Inherit, |profile| profile.start_at.clone())
    })
}

#[must_use]
pub fn env(index: usize) -> Vec<(String, String)> {
    with_table(|table| {
        table
            .get(index)
            .map(|profile| profile.env.clone())
            .unwrap_or_default()
    })
}

#[must_use]
pub fn hidden(index: usize) -> bool {
    with_table(|table| table.get(index).is_some_and(|profile| profile.hidden))
}

/// The program this row names, spelled out in full — what the editor's `Program`
/// field holds.
///
/// The *resolution* rather than a probe: a built-in nobody has edited says what
/// it would look for, which is what the field must show before it is typed into,
/// and the machine's own answer belongs to [`ProfilePrograms`]. `resolved` is
/// that answer when there is one, because a reader looking at `PowerShell 7`
/// wants the path this machine found rather than the words `BT_SHELL, then a
/// probe`.
#[must_use]
pub fn program_text(index: usize, resolved: Option<&OsStr>) -> String {
    with_table(
        |table| match table.get(index).map(|profile| &profile.program) {
            Some(ProgramSource::Path(path)) => path.to_string_lossy().into_owned(),
            _ => resolved
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
        },
    )
}

/// Split an argument line the way Windows splits a command line — **spaces
/// separate, double quotes group** (plan §3.3).
///
/// The rule is short on purpose and it is stated in the row's own sentence,
/// because an argument box whose quoting rule cannot be said in one line is a box
/// people get wrong. Concretely: runs of whitespace end a word; a `"` opens a
/// group in which whitespace is ordinary and a second `"` closes it; a `""`
/// inside a group is one literal quote, which is `CommandLineToArgvW`'s own rule
/// for the case and the only one a person can discover by trying it. Backslash
/// escaping — `\"` — is deliberately **not** honoured: a Windows path is full of
/// backslashes, and a rule that made `C:\bin\` change the meaning of the next
/// character would break the commonest argument there is.
#[must_use]
pub fn split_arguments(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quoted = false;
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '"' if quoted && characters.peek() == Some(&'"') => {
                characters.next();
                word.push('"');
            }
            '"' => {
                quoted = !quoted;
                started = true;
            }
            character if character.is_whitespace() && !quoted => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            character => {
                word.push(character);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}

/// The same words back as one line, quoted only where [`split_arguments`] would
/// otherwise read two.
///
/// The pair has to round-trip, because the field is written from the table on
/// every visit: a joiner that quoted differently from the splitter would rewrite
/// somebody's arguments the second time they opened the page.
#[must_use]
pub fn join_arguments(words: &[String]) -> String {
    words
        .iter()
        .map(|word| {
            if word.is_empty() || word.chars().any(char::is_whitespace) || word.contains('"') {
                format!("\"{}\"", word.replace('"', "\"\""))
            } else {
                word.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `PowerShell 7 copy`, then `PowerShell 7 copy 2` — the naming the scheme
/// customiser struck a slice earlier, verbatim: **a copy of a copy numbers
/// itself from the original's name**, so duplicating `X copy` gives `X copy 2`
/// and never `X copy copy`.
fn copy_title(source: &str, profiles: &[Profile]) -> String {
    let stem = source
        .rsplit_once(" copy")
        .map_or(source, |(head, tail)| {
            if tail.is_empty() || tail.trim_start().parse::<u32>().is_ok() {
                head
            } else {
                source
            }
        })
        .to_owned();
    let taken = |candidate: &str| {
        profiles
            .iter()
            .any(|profile| profile.display_title == candidate)
    };
    let first = format!("{stem} copy");
    if !taken(&first) {
        return first;
    }
    (2u32..)
        .map(|number| format!("{stem} copy {number}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or(first)
}

/// A stable id for a profile of the user's own: **the name it was made with,
/// slugged, plus four hex digits** — `claude-7f3a`, `powershell-7-copy-91b2`.
///
/// Not the slug alone, because renaming is the commonest edit there is and an
/// identity that moved with the name would strand every seed on disk naming it.
/// Not a bare uuid either: the whole point of a file of its own is that a person
/// can read it, and `a3f1c8e0-…` is a line nobody can read. Computed once at
/// creation and never recomputed.
///
/// The five shipped slugs are reserved words; a collision — with them, or with
/// an id already in the table — retries with another suffix.
fn fresh_id(display_title: &str, profiles: &[Profile]) -> String {
    let mut stem = String::new();
    for character in display_title.chars() {
        if character.is_ascii_alphanumeric() {
            stem.push(character.to_ascii_lowercase());
        } else if !stem.ends_with('-') {
            stem.push('-');
        }
    }
    let stem = stem.trim_matches('-');
    let stem = if stem.is_empty() { "profile" } else { stem };
    let shipped = shipped();
    let taken = |candidate: &str| {
        profiles.iter().any(|profile| profile.id == candidate)
            || shipped.iter().any(|profile| profile.id == candidate)
    };
    let mut seed = suffix_seed();
    loop {
        let candidate = format!("{stem}-{:04x}", seed & 0xffff);
        if !taken(&candidate) {
            return candidate;
        }
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    }
}

/// Four hex digits nobody has to be able to predict, and nobody has to be able
/// to reproduce either.
///
/// The clock and a counter rather than a random-number dependency: the suffix
/// only has to be *unlikely* to collide, and the loop above turns a collision
/// into another try rather than into a bug. Adding a crate to this workspace to
/// draw four hex digits would be paying a supply chain for a tie-break.
fn suffix_seed() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos: u64 = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos().into());
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    nanos
        .wrapping_mul(2_862_933_555_777_941_757)
        .wrapping_add(count.wrapping_mul(3_037_000_493))
}

/// How many rows the table holds.
#[must_use]
pub fn count() -> usize {
    with_table(ProfileTable::len)
}

/// The stable id of one row.
#[must_use]
pub fn id(index: usize) -> String {
    with_table(|table| {
        table
            .get(index)
            .map(|profile| profile.id.clone())
            .unwrap_or_default()
    })
}

/// The name this row is called, before the machine's qualifier is joined on.
///
/// [`title`] is what a surface draws; this is what the editor edits, and the
/// second string in the comparison set.
#[must_use]
pub fn display_title(index: usize) -> String {
    with_table(|table| {
        table
            .get(index)
            .map(|profile| profile.display_title.clone())
            .unwrap_or_default()
    })
}

/// The mark one row wears.
#[must_use]
pub fn mark(index: usize) -> ChromeMark {
    with_table(|table| {
        table
            .get(index)
            .map_or(ChromeMark::ProfileCmd, |profile| profile.mark)
    })
}

/// Which spelling of a path one row's shell speaks.
#[must_use]
pub fn paths(index: usize) -> PathNamespace {
    with_table(|table| {
        table
            .get(index)
            .map_or(PathNamespace::Windows, |profile| profile.paths)
    })
}

/// Which integration script serves one row — **resolved**, which is what every
/// caller outside the editor's own picker is asking.
#[must_use]
pub fn integration(index: usize) -> Integration {
    with_table(|table| table.get(index).map_or(Integration::None, served_by))
}

/// One whole row, cloned — **what the spawn path is handed** (§7.1.6c-6c).
///
/// `shell_command` used to take an index and ask this module four separate
/// questions about it; it takes the row itself now, which makes it a pure
/// function of its arguments and therefore a thing a test can put any profile in
/// front of. The clone is five short strings and a vector, once per tab.
#[must_use]
pub fn row(index: usize) -> Option<Profile> {
    with_table(|table| table.get(index).cloned())
}

/// The same question as the editor's picker holds it: the rule, or the answer.
#[must_use]
pub fn integration_choice(index: usize) -> IntegrationChoice {
    with_table(|table| {
        table
            .get(index)
            .map_or(IntegrationChoice::Auto, |profile| profile.integration)
    })
}

/// Which door serves this row's sessions — the `Shell integration` picker.
///
/// The namespace comes with it, because it has to: `paths` is derived from the
/// program *and* the door (only `wsl.exe` behind a bash init file crosses into
/// the distribution's own spelling), and a row whose door changed without its
/// namespace following would go on translating directories for a shell it is no
/// longer starting.
pub fn set_integration(index: usize, choice: IntegrationChoice) -> bool {
    registry().edit(index, |profile| {
        if profile.integration == choice {
            return false;
        }
        profile.integration = choice;
        profile.paths = derived_paths(profile);
        true
    })
}

/// The arguments one row always passes, ahead of anything the shell reads.
#[must_use]
pub fn args(index: usize) -> Vec<String> {
    with_table(|table| {
        table
            .get(index)
            .map(|profile| profile.args.clone())
            .unwrap_or_default()
    })
}

/// One row of the Settings dialog's **Profiles** page, ready to be laid out.
///
/// Built here and not in `settings.rs` for the reason every other derived answer
/// in this house is built once: the `˅` menu, the pane submenu, the default
/// picker and this page are four surfaces asking the same three questions about
/// a profile — what is it called, can this machine start it, and what does it
/// actually give you — and four spellings of that is four chances for two of
/// them to disagree in front of the same reader.
///
/// It rides into the dialog through `SettingsContent`, exactly as the shortcut
/// page's lines do, which is what keeps [`ProfilePrograms`] — a fact about a
/// filesystem — out of a struct that is otherwise a snapshot of settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileLine {
    /// Which row of the table this is. **The table's index and not the page's**:
    /// they are the same today because the page shows hidden rows too, and they
    /// would come apart the day it did not.
    pub index: usize,
    pub mark: ChromeMark,
    /// What the row is called, qualifier and all — [`title`].
    pub title: &'static str,
    /// The line under the name: what this profile runs, or — on a machine that
    /// does not have it — why it cannot.
    pub command: String,
    /// The honest capability sentence (J85), or `None` for a row that cannot
    /// run at all.
    ///
    /// **An unavailable row drops this line rather than greying it.** A shell
    /// that is not on this machine has no capabilities to report, and the one
    /// sentence the row has room for is the reason it cannot start — which is
    /// `.row.unavailable`'s existing rule applied to a row that happened to have
    /// two lines.
    pub capability: Option<&'static str>,
    /// Whether the `default` badge is on this row. **It reports and is not a
    /// control**: the default is changed on the General page and nowhere else,
    /// because one field with two writers is the thing §7.1.6c-4a just avoided.
    pub is_default: bool,
    /// Whether this is the floor every degradation in the product lands on.
    ///
    /// A fact about the table and not about this profile, exactly as
    /// [`Self::is_default`] is a fact about `settings.json` resolved against
    /// this machine — and it is here for that field's reason: the row menu greys
    /// `Hide` on both, and a menu that asked the table itself would be a second
    /// reader of a rule [`set_hidden`] already enforces.
    pub is_fallback: bool,
    /// Whether the `⋯` offers `Delete` at all. A built-in is hidden, never
    /// deleted, and its menu is simply shorter.
    pub deletable: bool,
    pub hidden: bool,
    pub available: bool,
}

/// Every row of the Profiles page, top to bottom.
///
/// Hidden rows are **here** and absent from [`ProfileTable::offered`], and the
/// asymmetry is the point: this page is where hiding is undone, so a page that
/// honoured hiding would be a page with no way back.
#[must_use]
pub fn page_lines(programs: &ProfilePrograms, default: usize) -> Vec<ProfileLine> {
    let fallback = fallback_profile();
    with_table(|table| {
        table
            .profiles()
            .iter()
            .enumerate()
            .map(|(index, profile)| {
                let available = programs.is_available(index);
                ProfileLine {
                    index,
                    mark: profile.mark,
                    title: title(index),
                    command: if available {
                        command_line(profile, programs.program(index))
                    } else {
                        crate::i18n::profile_not_installed(title(index))
                    },
                    capability: available.then(|| capability_text(profile).text()),
                    is_default: index == default,
                    is_fallback: index == fallback,
                    deletable: profile.origin == Origin::User,
                    hidden: profile.hidden,
                    available,
                }
            })
            .collect()
    })
}

/// `pwsh.exe -NoLogo`, `wsl.exe --cd ~`, `cmd.exe` — what this row starts, in
/// the words the mock-up wrote under each of its names.
///
/// **The leaf and not the path.** The full path is `C:\Program
/// Files\PowerShell\7\pwsh.exe` and the row has about fifty-eight characters;
/// the executable's own name is the half that identifies it, and the half a
/// reader would recognise. The place arguments are appended because for the one
/// profile that has them they are the whole of where it opens — `wsl.exe` alone
/// says nothing about `~`.
fn command_line(profile: &Profile, resolved: Option<&OsStr>) -> String {
    let program = resolved
        .map(Path::new)
        .and_then(Path::file_name)
        .map_or_else(
            || profile.display_title.clone(),
            |name| name.to_string_lossy().into_owned(),
        );
    let mut words = vec![program];
    words.extend(profile.args.iter().cloned());
    if let StartingDir::LauncherFlag { flag, home } = &profile.starting_dir {
        words.push(flag.clone());
        words.push(home.clone());
    }
    words.join(" ")
}

/// **The honest capability sentence** (J85), derived from what actually reaches
/// the shell.
///
/// The authority is `docs/shell-integration.md`'s "What each profile actually
/// gets", and this page does not build a second matrix — it draws one row of
/// that one. Which is why the derivation is over [`Integration`] and
/// [`PathNamespace`] rather than over the profile's id: a duplicate of WSL is
/// not `wsl`, and it gets WSL's sentence because it gets WSL's door.
///
/// Two of the answers carry their condition **in the sentence**, because this
/// page cannot probe for it: whether `folio.ps1` has been dot-sourced, and
/// whether a WSL login lands in bash, are known only to a live session that has
/// already spoken. Said this way each sentence is true in every state and never
/// needs a probe to stay true.
///
/// **Hyperlinks are the third dimension** (§7.1.6c-6c, J85 closed). Every
/// sentence above names them, and four of them would be claiming something this
/// profile has switched off: `FORCE_HYPERLINK=0` in its own environment, or —
/// on a PowerShell — a `TERM_PROGRAM` override, because `folio.ps1` declares
/// links only for a session whose `TERM_PROGRAM` it recognises as this
/// terminal's. So the answer is asked of the one place that knows it
/// ([`crate::shell_integration::declares_hyperlinks`]) and each sentence has a
/// twin that names their absence rather than passing over it.
#[must_use]
pub fn capability_text(profile: &Profile) -> crate::i18n::Text {
    capability_of_parts(
        served_by(profile),
        profile.paths,
        crate::shell_integration::declares_hyperlinks(profile),
        false,
    )
}

/// [`capability_text`] with the parts named, so the editor can ask for the long
/// form of the one sentence that has one.
///
/// `long` is the editor's page and not a second wording of the same fact: the
/// list's third line shares its row with an action run and holds about
/// fifty-eight characters, so `No shell integration` is all that fits there,
/// while the editor's `Shell integration` row has the page's whole width and a
/// reader standing in front of the picker is owed *what* it costs. The other
/// four sentences are one length, because they already say what they have and
/// what they have not.
#[must_use]
pub fn capability_of_parts(
    integration: Integration,
    paths: PathNamespace,
    hyperlinks: bool,
    long: bool,
) -> crate::i18n::Text {
    use crate::i18n::Text;
    match (integration, paths, hyperlinks, long) {
        (Integration::PowerShellOptIn, _, true, _) => Text::CapPowerShell,
        (Integration::PowerShellOptIn, _, false, _) => Text::CapPowerShellNoLinks,
        // The launcher is the difference: a Git Bash is handed its init file and
        // reads it, full stop, while `wsl.exe` hands it to whatever shell the
        // distribution logs the user into.
        (Integration::BashInitFile, PathNamespace::Wsl, true, _) => Text::CapWslBash,
        (Integration::BashInitFile, PathNamespace::Wsl, false, _) => Text::CapWslBashNoLinks,
        (Integration::BashInitFile, PathNamespace::Windows, true, _) => Text::CapFull,
        (Integration::BashInitFile, PathNamespace::Windows, false, _) => Text::CapFullNoLinks,
        (Integration::CmdPrompt, _, true, _) => Text::CapCmd,
        (Integration::CmdPrompt, _, false, _) => Text::CapCmdNoLinks,
        (Integration::None, _, _, false) => Text::CapNone,
        (Integration::None, _, true, true) => Text::CapNoneLong,
        (Integration::None, _, false, true) => Text::CapNoneLongNoLinks,
    }
}

/// The editor's `Shell integration` row: the same sentence the list carries, in
/// the one length the page has room for.
#[must_use]
pub fn capability_in_editor(index: usize) -> crate::i18n::Text {
    with_table(|table| {
        table
            .get(index)
            .map_or(crate::i18n::Text::CapNone, |profile| {
                capability_of_parts(
                    served_by(profile),
                    profile.paths,
                    crate::shell_integration::declares_hyperlinks(profile),
                    true,
                )
            })
    })
}

/// Every spelling of one profile's name that a shell announcing it would use —
/// **the set a pane head compares an OSC 0/2 title against**.
///
/// A shell that merely says its launcher's name has announced nothing, and the
/// head must not promote it to a program title. Which name, though, is now two
/// questions and not one (plan §1.4, and the first thing slice 5a had to land):
///
/// * the **shipped** title, because that is what the integration scripts send —
///   `folio.ps1` writes `PowerShell 7` or `Windows PowerShell 5.1` however the
///   row has since been renamed. Drop this and the family name reappears in
///   front of every pane head the day somebody renames a row;
/// * the **display** title, because a user who renames a row to `七号` is owed
///   the same silence when their own shell echoes it back;
/// * the **qualified** form, because that is the third spelling this window
///   itself shows, and `WSL · Ubuntu` is what a tab reads.
///
/// A profile of the user's own contributes no shipped string: no script this
/// build ships will ever announce a name this build did not choose, so there is
/// no second word to compare.
///
/// `&'static str` throughout, so the set can be handed to a pure function and
/// held for the length of a frame — the two composed spellings are interned by
/// [`title`]'s own table.
#[must_use]
pub fn announcement_set(index: usize) -> Vec<&'static str> {
    let qualified = title(index);
    with_table(|table| {
        table.get(index).map_or_else(
            || vec![qualified],
            |profile| {
                announcement_names(profile, qualified)
                    .iter()
                    .map(|name| intern(name))
                    .collect()
            },
        )
    })
}

/// [`announcement_set`]'s rule, without the interning and without the table —
/// so the rule can be read, and tested, on one profile.
fn announcement_names(profile: &Profile, qualified: &str) -> Vec<String> {
    let mut names = vec![qualified.to_owned()];
    for name in [
        profile.compared_title.as_deref(),
        Some(profile.display_title.as_str()),
    ]
    .into_iter()
    .flatten()
    {
        if !names.iter().any(|held| held == name) {
            names.push(name.to_owned());
        }
    }
    names
}

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
///
/// **A lookup and no longer the literal `1`** (§7.1.6c-6). It was an ordinal
/// into a `const` array, which was exact for as long as nothing could reorder
/// that array; a table the user can reorder turns the same literal into "whoever
/// happens to be second", and a floor that can be walked out from under is a
/// fallback chain with a hole in the bottom. The id is the thing that does not
/// move, so the id is what this asks for.
#[must_use]
pub fn fallback_profile() -> usize {
    with_table(fallback_profile_in)
}

/// The same floor over a table handed in rather than the process's.
///
/// The three answers below are one rule read three ways and they are written
/// against a borrowed table for one reason: a case that moved the *window's*
/// table to ask what happens when a profile disappears would race every other
/// case in this crate. The file can now change under a running window
/// (§7.1.6c-6d), so "what a name resolves to after the table moved" is a
/// question with red gates on it, and a question that cannot be asked of a
/// private table cannot have one.
fn fallback_profile_in(table: &ProfileTable) -> usize {
    table.position_of_id(WINDOWS_POWERSHELL_ID).unwrap_or(0)
}

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
///   has never opened the setting has — is [`fallback_profile()`], which is
///   [`index_of_id`]'s rule and not a second one;
/// * an id naming a profile this machine cannot start is **also**
///   [`fallback_profile()`], and this is the part `index_of_id` cannot do because
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
    with_table(|table| default_profile_in(table, stored, |index| programs.is_available(index)))
}

/// The same three inputs over a table handed in — see [`fallback_profile_in`].
///
/// **This is also the whole of what an external deletion owes the default
/// profile** (§7.1.6c-6d): a hand edit that takes the row away leaves
/// `settings.json` naming an id nothing holds, which is the first of the three
/// cases above and was already answered before the file could move. There is no
/// second rule for a row deleted on disk, and the stored id is left alone there
/// too — putting the entry back puts the default back.
fn default_profile_in(
    table: &ProfileTable,
    stored: &str,
    available: impl Fn(usize) -> bool,
) -> usize {
    table
        .position_of_id(stored)
        .filter(|index| available(*index))
        .unwrap_or_else(|| fallback_profile_in(table))
}

/// Which profile a seed's `profile_id` names, or [`fallback_profile()`] when the
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
    with_table(|table| index_of_id_in(table, id))
}

/// The same rule over a table handed in — see [`fallback_profile_in`].
///
/// **A seat that is already running reaches for this when the file moves under
/// it** (§7.1.6c-6d). A pane holds the *index* of the profile it was born from,
/// and an index is a position in a table somebody may now be reordering in an
/// editor; asking by id across the change is what keeps a running pane wearing
/// its own mark instead of whichever row slid into its slot. When the profile is
/// gone the rule above applies unchanged — the seat costs its shell choice,
/// never the seat.
fn index_of_id_in(table: &ProfileTable, id: &str) -> usize {
    table
        .position_of_id(id)
        .unwrap_or_else(|| fallback_profile_in(table))
}

/// Whether this build has a profile by that name at all.
///
/// The question [`index_of_id`] answers *away*: it folds "this profile" and "no
/// such profile, have the default" into one number, which is right for every
/// caller that needs a profile and wrong for the one caller that needs to know a
/// substitution happened. `M2-restart-shell-contract.md` §3 requires that
/// substitution to be visible — "绝不静默替换" — and a function that answers
/// `fallback_profile()` for a saved `"pwsh"` and a saved `"fish"` alike cannot tell
/// anyone which of the two it was looking at.
#[must_use]
pub fn has_id(id: &str) -> bool {
    with_table(|table| table.position_of_id(id).is_some())
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
    crate::i18n::unavailable_profile_tip(title(profile))
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
    static TITLES: RwLock<Option<(u64, Vec<&'static str>)>> = RwLock::new(None);
    let revision = profile_revision();
    {
        let held = TITLES
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((at, titles)) = held.as_ref()
            && *at == revision
            && let Some(found) = titles.get(profile)
        {
            return found;
        }
    }
    let qualifier = crate::wsl::facts().title_qualifier();
    let composed: Vec<&'static str> = with_table(|table| {
        table
            .profiles
            .iter()
            .map(|entry| intern(&compose_title(entry, qualifier)))
            .collect()
    });
    let mut held = TITLES
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *held = Some((revision, composed));
    held.as_ref()
        .and_then(|(_, titles)| titles.get(profile).copied())
        .unwrap_or("")
}

/// One `&'static str` per distinct composed title this process has ever shown.
///
/// `crate::settings::intern_scheme_name`'s recipe, one module over and for the
/// same reason: the leak is **per name, not per call and not per rebuild**, so
/// the memory this can consume is bounded by how many different names the user's
/// table has held in one session — a handful, for the case it exists for, which
/// is a row being renamed or duplicated. Freeing one would mean proving that no
/// picker, no measurement and no hit test still holds it, which is exactly the
/// proof `&'static str` exists to not have to write.
fn intern(text: &str) -> &'static str {
    static HELD: Mutex<Option<std::collections::BTreeSet<&'static str>>> = Mutex::new(None);
    let mut held = HELD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let names = held.get_or_insert_with(std::collections::BTreeSet::new);
    if let Some(found) = names.get(text) {
        return found;
    }
    let leaked: &'static str = Box::leak(text.to_owned().into_boxed_str());
    names.insert(leaked);
    leaked
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
fn compose_title(profile: &Profile, qualifier: Option<&str>) -> String {
    match (profile.qualifier, qualifier) {
        (Qualifier::WslDistribution, Some(distribution)) => {
            format!("{} · {distribution}", profile.display_title)
        }
        _ => profile.display_title.clone(),
    }
}

/// Which spelling of a filesystem path a profile's shell speaks.
///
/// Not a property of the *program* but of the world it stands in. Three of these
/// profiles are Windows processes whose working directory is a Win32 directory,
/// and they say so in drive letters. WSL's shell lives inside the distribution's
/// own filesystem, where this machine's `D:\Developer` is `/mnt/d/Developer` and
/// `/home/alice` is a place a drive letter cannot reach at all.
///
/// The field exists because a directory travelling between two panes is only
/// meaningful with the namespace it was written in attached, and the pane
/// already carries the one thing that knows it: its profile. Without this,
/// `C:\Users\Alice` inherited into a WSL tab is a string that names nothing, and
/// the pane opens in a place nobody chose while looking as though it worked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathNamespace {
    /// `D:\Developer` — a Win32 path, drive-rooted.
    Windows,
    /// `/mnt/d/Developer`, `/home/alice` — the distribution's own filesystem.
    Wsl,
}

/// `C:\Users\Alice` → `/mnt/c/Users/Alice`, or `None` when this path names
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

/// `/mnt/c/Users/Alice` → `C:\Users\Alice`, or `None` when Windows has no name
/// for this place.
///
/// The inverse is **not total**, and that asymmetry is the whole reason a
/// translation can fail. `/home/alice` is a directory inside the distribution's
/// own root filesystem; the only Windows spelling of it is the
/// `\\wsl.localhost\<distro>\home\alice` share, which is a network path to a
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
    translate_cwd(paths(source), paths(target), cwd?)
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
    /// **The place itself**, written in this profile's own namespace, whichever
    /// of the two channels above ended up carrying it — and `None` only when
    /// this machine could not name one at all.
    ///
    /// Not a duplicate of [`Self::working_directory`]: that field is the half a
    /// *Windows process* can be handed, and it is empty by construction for a
    /// profile whose place goes to a launcher instead. Both fields answer
    /// questions the spawn asks; this one answers a question asked afterwards —
    /// "where is this shell standing, before it has said anything?" — which
    /// `TabState::term_leaf` needs for a leaf that never reported an OSC 7 and
    /// which neither channel alone can answer.
    ///
    /// Computed here rather than at the reader, because here is the only place
    /// that has the profile's table row, the inheritance and the environment at
    /// once. §7.1.4 gives the chain as OSC 7 → the initial cwd → `HOME`, and the
    /// second and third links are both this field: the fall to a home is already
    /// inside the two arms below, so a reader gets one answer instead of a
    /// second copy of the ladder.
    pub directory: Option<PathBuf>,
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
    match paths(profile) {
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
    let (start_at, starting_dir, namespace) = with_table(|table| {
        table.get(profile).map_or_else(
            || {
                (
                    StartAt::Inherit,
                    StartingDir::WindowsHome,
                    PathNamespace::Windows,
                )
            },
            |entry| {
                (
                    entry.start_at.clone(),
                    entry.starting_dir.clone(),
                    entry.paths,
                )
            },
        )
    });
    place_for(&start_at, &starting_dir, namespace, inherited, environment)
}

/// [`spawn_place`]'s pure half — the three answers resolved against one
/// machine, with no table under it.
///
/// Split off for [`merge`]'s reason one function over: the rule is worth pinning
/// without moving the process's own profile table to pin it, and `cargo test`
/// runs this crate's cases in one process.
fn place_for(
    start_at: &StartAt,
    starting_dir: &StartingDir,
    namespace: PathNamespace,
    inherited: Option<PathBuf>,
    environment: &dyn ShellEnvironment,
) -> SpawnPlace {
    // **The reader's question first, the machine's second.** What the editor
    // chose decides whether there is a place at all; the profile's own
    // `starting_dir` decides which of the two channels it travels on, and a
    // place that never arrived falls through to the profile's home exactly as an
    // untranslatable inheritance already did.
    let place = match start_at.clone() {
        StartAt::Inherit => inherited,
        // Not "inherit with nothing to inherit" — this one *refuses* a folder
        // that was there, which is the whole of what the reader asked for.
        StartAt::Home => None,
        // Written in the picker's namespace and crossed into the profile's here,
        // through the one door every crossing in this module goes through. A
        // pair that cannot cross falls to the home below, which is
        // `cwd_for_spawn`'s own rule and not a second one.
        StartAt::Fixed(fixed) => translate_cwd(PathNamespace::Windows, namespace, &fixed),
    };
    match starting_dir {
        StartingDir::WindowsHome => {
            let directory = place.or_else(|| environment.var_os("USERPROFILE").map(PathBuf::from));
            SpawnPlace {
                working_directory: directory.clone(),
                arguments: Vec::new(),
                directory,
            }
        }
        StartingDir::LauncherFlag { flag, home } => {
            let directory = place.unwrap_or_else(|| PathBuf::from(home.clone()));
            SpawnPlace {
                working_directory: None,
                arguments: vec![
                    OsString::from(flag.clone()),
                    directory.clone().into_os_string(),
                ],
                directory: Some(directory),
            }
        }
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
fn git_fallbacks() -> [ProgramCandidate; 3] {
    [
        ProgramCandidate::Under {
            variable: "ProgramFiles".to_owned(),
            tail: r"Git\cmd\git.exe".to_owned(),
        },
        ProgramCandidate::Under {
            variable: "ProgramFiles(x86)".to_owned(),
            tail: r"Git\cmd\git.exe".to_owned(),
        },
        ProgramCandidate::Under {
            variable: "LocalAppData".to_owned(),
            tail: r"Programs\Git\cmd\git.exe".to_owned(),
        },
    ]
}

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
        git_fallbacks().iter().find_map(|candidate| {
            ProfilePrograms::candidate_path(candidate, environment)
                .filter(|path| environment.is_file(path))
        })
    })
}

// `vscode_fallbacks` and `find_vscode` are **retired** (user ruling 2026-08-25:
// 「既然有 default app 了,就把 VS Code 去掉」). They existed for one row of one
// menu — the editor row the 2026-08-24 ruling put under `Open with default app`
// — and the row is gone: the door above it already asks this machine what it has
// registered for the path, which on a developer's machine is very often that
// same editor. A probe kept alive for a row nobody draws is a `is_file` walk at
// start-up answering a question the product no longer asks.
//
// What went with them: the `editor` parameter that `file_menu`, `file_menu_step`
// and `file_menu_layout` all carried, and with it the only fact outside the
// subject that could change the length of one of these lists.

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
    resolved: Vec<Option<OsString>>,
}

impl ProfilePrograms {
    /// Ask the machine, once, what each profile would start.
    #[must_use]
    pub fn probe(environment: &dyn ShellEnvironment) -> Self {
        Self {
            resolved: with_table(|table| {
                table
                    .profiles
                    .iter()
                    .map(|profile| match &profile.program {
                        // A real `None` on a machine with no PowerShell 7, which
                        // is what greys the row rather than starting 5.1 under
                        // 7's name.
                        ProgramSource::PowerShellSeven => resolve_powershell_seven(environment),
                        ProgramSource::FirstOf(candidates) => candidates
                            .iter()
                            .filter_map(|candidate| Self::candidate_path(candidate, environment))
                            .find(|candidate| environment.is_file(candidate))
                            .map(PathBuf::into_os_string),
                        // A path the user named is a path or it is not: there is
                        // no list to walk, and a program that is not there greys
                        // the row exactly as a missing built-in does.
                        ProgramSource::Path(path) => environment
                            .is_file(path)
                            .then(|| path.clone().into_os_string()),
                    })
                    .collect()
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
        candidate: &ProgramCandidate,
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
    /// `New terminal in folder…` — a tab on the default profile, standing in a
    /// folder the system's own chooser is about to ask for (user ruling
    /// 2026-08-20).
    ///
    /// Indexes nothing, for [`Self::FilesPane`]'s reason, and carries no path
    /// either: the row is the *question*, and the answer arrives a modal dialog
    /// later. A variant holding a `PathBuf` would be a row claiming to know
    /// where it was going before anybody said.
    NewInFolder,
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
    /// **The chord on the `Files pane` row**, measured — the one row of this
    /// menu that is also a row of the shortcut table (系统性发现 ②).
    ///
    /// It is the same verb and not merely a similar one: the row calls
    /// `Runtime::toggle_files_pane`, which is exactly what
    /// `shortcuts::Action::FilesPane` dispatches to, and the press arm in
    /// `main.rs` says so in its own comment — 「this one gives the tab you are
    /// in a pane, through the same verb `Ctrl+Shift+B` reaches」.
    ///
    /// Carried on [`PaneMenuLayout::accels`]' reasoning: the frame is measured
    /// with this column reserved in it, so the painter must draw the very string
    /// that was measured.
    files_pane_accel: Option<(String, f32)>,
    /// One row per **offered** profile, top to bottom.
    items: Vec<[f32; 4]>,
    /// Which table row each of [`Self::items`] draws.
    ///
    /// The two are not the same number once anything is hidden, and the answer a
    /// bare ordinal would get wrong here is silent — the wrong shell would
    /// simply start. So the mapping is laid down once, at layout, and the draw
    /// and the hit test both read it rather than each recomputing which rows are
    /// on offer.
    profiles: Vec<usize>,
    /// The `.menu-sep` above the middle section.
    ///
    /// Unconditional where the Recent separator is optional, and the asymmetry
    /// is the mock-up's: a Recent heading over an empty list is a promise the
    /// menu cannot keep, while both rows below this rule are always available —
    /// every tab can be given a files column, and the folder chooser is a
    /// question this window can always ask.
    files_separator: [f32; 4],
    /// The `Files pane` row itself.
    files_pane: [f32; 4],
    /// `New terminal in folder…`, under it (user ruling 2026-08-20).
    new_in_folder: [f32; 4],
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
    /// **Which way this menu grew out of the button that raised it** — the
    /// arrival's four pixels, decided here because this is the one place that
    /// knows both boxes.
    ///
    /// Every menu in this file carries one, and every one of them derives it the
    /// same way: [`Travel::away_from`] against the anchor the placement was
    /// solved from. It is a fact about *this* placement and not about the kind of
    /// menu, which is the whole reason it is a field — the same list drops out of
    /// a chevron near the top of the window and stands on it near the bottom, and
    /// a menu that slid down out of a button underneath it would be four pixels
    /// of a lie about where it came from.
    travel: Travel,
}

impl ProfileMenuLayout {
    /// **Which way this menu grew** — see [`Self::travel`].
    #[must_use]
    pub fn travel(&self) -> Travel {
        self.travel
    }

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
        let recents = self
            .recent
            .iter()
            .zip(menu_rows(recent))
            .enumerate()
            .map(|(index, (rect, entry))| (MenuRow::Recent(index), *rect, recent_tip(&entry.seed)));
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

impl MenuSide {
    /// **Whether the `˅` that opened a menu on this side turns over while it is
    /// up** (§7.1.6e, extended to the new-tab button by user ruling
    /// 2026-08-20).
    ///
    /// The judgement is one sentence about geometry and it was first written for
    /// the pane head's `⌄`: *"翻到 180° 的箭头指向自己菜单之外"* — a turned
    /// arrow claims the list is **down there**, and that claim is true of
    /// [`Self::Below`] and of nothing else. Beside its button, an arrow rotated
    /// 180° points away from the very thing it is announcing, which is worse
    /// than saying nothing: it is a control lying about where its own menu went.
    ///
    /// So the turn is not a property of the strip, of the rail or of the card
    /// column — it is a property of *this* enum, which is why it is written here
    /// and read from here by all four of the places that care: the three
    /// surfaces that draw the arrow, and the tween that would otherwise spend
    /// 140ms of frames arriving at an angle nobody draws.
    #[must_use]
    pub fn turns_the_chevron(self) -> bool {
        matches!(self, Self::Below)
    }
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
    shortcuts: &crate::shortcuts::Shortcuts,
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
    // The second section is one rule and **two** rows, and it is never absent:
    // the two things in this menu that are about a *folder* rather than about a
    // shell — give this tab a column on one, or open a terminal in one you are
    // about to choose (user ruling 2026-08-20).
    let files_block = separator_block + 2.0 * item_height;

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
    let annotation = measure(hint_text(), px(HINT_FONT_LOGICAL_PX))
        .max(measure(unavailable_hint_text(), px(HINT_FONT_LOGICAL_PX)));
    // Measured before the closure below borrows `measure` for the rest of the
    // function, not because the order matters to the layout.
    let files_hint = measure(files_pane_hint_text(), px(HINT_FONT_LOGICAL_PX));
    // The one chord this menu prints, measured before the row it sits on is
    // (系统性发现 ②). Reserved out of the `Files pane` row's own content rather
    // than out of every row: the rows above it are profiles, whose width is the
    // reader's own shell names, and a chord column across all of them would
    // widen the picker on every machine to annotate one line.
    let files_pane_accel = accelerator_of(
        Some(crate::shortcuts::Action::FilesPane),
        false,
        shortcuts,
        scale,
        measure,
    );
    let files_accel_claim = accelerator_claim(std::slice::from_ref(&files_pane_accel), scale);
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
    //
    // `New terminal in folder…` joins them on the same footing and by the same
    // sentence — its caption is one of this module's own strings — and it needs
    // to, because in both languages it is the longest row in the menu. It
    // carries no annotation: the rows above the rule all open a tab and say so
    // with a hint, and this one opens a tab too, so there is no difference for a
    // hint to be about. Its ellipsis is what it has to say, and the caption
    // already says it.
    let files_row = row_content(
        files_pane_text(),
        px(ITEM_FONT_LOGICAL_PX),
        files_hint + files_accel_claim,
    )
    .max(row_content(
        new_in_folder_text(),
        px(ITEM_FONT_LOGICAL_PX),
        0.0,
    ));
    let offered = table().offered();
    let content = offered
        .iter()
        // `title(index)` and not `Profile::display_title`: the qualifier is part
        // of the string the row draws, and on a machine with two distributions
        // it is the longest row in the list.
        .map(|index| row_content(title(*index), px(ITEM_FONT_LOGICAL_PX), annotation))
        .fold(files_row, f32::max);
    let width = (chrome + content)
        .max(px(MENU_MIN_WIDTH_LOGICAL_PX))
        .round();
    let height = (2.0 * (border + padding)
        + item_height * offered.len() as f32
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
    let mut items = Vec::with_capacity(offered.len());
    for _ in 0..offered.len() {
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
    let new_in_folder = [content_left, cursor, content_right, cursor + item_height];
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
        files_pane_accel,
        items,
        profiles: offered,
        files_separator,
        files_pane,
        new_in_folder,
        separator,
        section_label,
        recent: recent_rows,
        travel: Travel::away_from(anchor, frame),
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
    for (row, item) in layout.items.iter().enumerate() {
        if contains(*item, x, y) {
            let index = layout.profiles[row];
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
    // Available on the same terms and for a nearer reason: this row does not
    // start a shell, it opens a question. Which profile answers it is decided
    // after the folder is known, and the default profile is startable by
    // construction (`default_profile` refuses one this machine cannot run).
    if contains(layout.new_in_folder, x, y) {
        return Some(Some(MenuRow::NewInFolder));
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
/// A files locus has no shell, so nothing about it can be missing, and neither
/// has a file (§7.1.6h).
fn recent_is_available(seed: &Seed, programs: &ProfilePrograms) -> bool {
    match seed {
        Seed::Term { profile_id, .. } => programs.is_available(index_of_id(profile_id)),
        Seed::Files { .. } | Seed::Preview { .. } => true,
        // **A window is offered while any one of its tabs can still be opened**
        // (multiwindow slice D). Greying it because one shell of six has gone
        // would refuse the other five, and the window comes back holding what it
        // can — the missing pane's own banner is what says a shell is gone,
        // exactly as it does for a tab restored from the session file.
        Seed::Window { seeds } => seeds.iter().any(|seed| recent_is_available(seed, programs)),
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
#[allow(
    clippy::too_many_arguments,
    reason = "eight, and none of them is this menu's own: the boxes, which \
              shells this machine has, which one is the default, where the \
              pointer is, what the vault remembers, what time it is, and which \
              sites the session has icons for. Every one is a fact the caller \
              holds and this module has no way to ask for — the same division \
              `settings::layout_for_menu` states at length. Bundling them into \
              a struct would move the list rather than shorten it"
)]
pub fn build(
    layout: &ProfileMenuLayout,
    programs: &ProfilePrograms,
    default: usize,
    hover: Option<MenuRow>,
    recent: &[RecentEntry],
    now: SystemTime,
    favicons: &crate::favicon::Favicons,
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

    for (row, item) in layout.items.iter().enumerate() {
        let index = layout.profiles[row];
        let available = programs.is_available(index);
        push_row(
            &Row {
                rect: *item,
                mark: Some(mark(index)),
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
                    (index == default).then(|| hint(hint_text().to_owned()))
                } else {
                    Some(hint(unavailable_hint_text().to_owned()))
                },
                // No row of this menu is a row of the shortcut table (系统性发现 ②).
                accel: None,
                dirty: false,
                hovered: hover == Some(MenuRow::Profile(index)),
                available,
                pin: None,
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
            //
            // Asked of the registry since P2, which is also where it stopped
            // being the *solid* folder: this row is an act, and an act in a
            // column of verbs is struck.
            mark: Some(ActionIcon::OpenFilesPane.mark()),
            name: files_pane_text(),
            hint: Some(hint(files_pane_hint_text().to_owned())),
            // **The one row of this menu that is also a row of the shortcut
            // table** — see [`ProfileMenuLayout::files_pane_accel`]. Every row
            // above it names a profile and every row below names a folder or a
            // recent tab; none of those is a verb the table carries.
            accel: layout.files_pane_accel.clone(),
            dirty: false,
            hovered: hover == Some(MenuRow::FilesPane),
            // A files column needs no program behind it, so there is nothing
            // this machine could be missing and no greyed state to reach.
            available: true,
            pin: None,
        },
        scale,
        palette,
        &mut quads,
        &mut labels,
        &mut sprites,
    );
    push_row(
        &Row {
            rect: layout.new_in_folder,
            // The same generic folder the pane menu's own `New terminal in
            // folder…` wears (`PaneMenuRow::mark`) — literally the same, since
            // P2, because both ask the registry for it. Two rows under one rule
            // both marked with a folder is the section saying what it is: the
            // glyph names the thing being chosen, not the thing being opened.
            mark: Some(ActionIcon::NewTerminalInFolder.mark()),
            name: new_in_folder_text(),
            hint: None,
            // A system chooser is not a verb the shortcut table carries.
            accel: None,
            dirty: false,
            hovered: hover == Some(MenuRow::NewInFolder),
            available: true,
            pin: None,
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
        labels.push(section_label(recent_section_label(), band, scale, palette));
    }

    for (index, (row, entry)) in layout.recent.iter().zip(menu_rows(recent)).enumerate() {
        push_row(
            &Row {
                rect: *row,
                mark: Some(recent_mark(&entry.seed, favicons)),
                name: &recent_label(&entry.seed),
                // Still the age, and deliberately not `not installed`: a Recent
                // row's one annotation answers "when", the grey already answers
                // "can you", and losing the timestamp would cost the row the
                // only thing that orders it against its neighbours.
                hint: Some(hint(ago_label(entry.at, now))),
                // No row of this menu is a row of the shortcut table (系统性发现 ②).
                accel: None,
                dirty: false,
                hovered: hover == Some(MenuRow::Recent(index)),
                available: recent_is_available(&entry.seed, programs),
                pin: None,
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
        mono: false,
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

/// The pin control's own box, at a menu row's trailing edge.
///
/// The window tab's number (`WINDOW_TAB_PIN_GLYPH_LOGICAL_PX`), because it is
/// the window tab's control: the ruling that made pinning a folder possible said
/// in as many words that the pin's vocabulary is the one this product already
/// has, and a pin drawn two sizes in two places would be two vocabularies.
const ROW_PIN_LOGICAL_PX: f32 = 13.0;

/// The pin at the trailing edge of one menu row (user ruling 2026-08-19).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RowPin {
    /// Whether the thing this row names is pinned.
    ///
    /// **State rides on `filled` and never on a different glyph** —
    /// `ChromeMark::Pin`'s own rule, written where the tab strip already obeys
    /// it: regular means *you could pin this*, filled means *it is pinned*.
    filled: bool,
    /// Whether the pointer is on the pin itself rather than merely on the row.
    hovered: bool,
    /// Whether the row is under the pointer at all.
    ///
    /// An unpinned pin is an **offer** and appears with the hand; a pinned one
    /// is a **fact** about the row and stays whether or not anybody is pointing
    /// at it, so that a list of pinned rows reads as pinned at a glance. Same
    /// division the tab strip's pin makes, and for the same reason.
    revealed: bool,
}

/// What the pin claims out of a row's trailing edge.
///
/// **Reserved on every row of a menu that has pins at all**, drawn or not: this
/// is the reservation the note column and the dirty dot already make, and it is
/// what stops a name from shortening under the pointer as a pin fades in.
fn row_pin_claim(scale: f32) -> f32 {
    (ROW_PIN_LOGICAL_PX * scale).round() + ITEM_GAP_LOGICAL_PX * scale
}

/// One row's pin box — **one derivation, two callers**.
///
/// The hit test and the painter ask this and nothing else, which is what makes
/// the glyph somebody is pointing at and the rectangle that answers their press
/// the same rectangle. A second derivation is how a control ends up half a pixel
/// from the thing it draws.
fn row_pin_rect(item: [f32; 4], scale: f32) -> [f32; 4] {
    let size = (ROW_PIN_LOGICAL_PX * scale).round();
    let right = item[2] - ITEM_PADDING_X_LOGICAL_PX * scale;
    let top = ((item[1] + item[3] - size) / 2.0).round();
    [right - size, top, right, top + size]
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
    /// **The chord this row's verb also answers to, and its measured width**
    /// (gesture audit 2026-08-26, 系统性发现 ②).
    ///
    /// The audit's second systemic finding was that this window has two doors
    /// onto a dozen verbs and neither door mentions the other: the hint card is
    /// the keyboard's side of it, and `profiles.rs` had no accelerator column
    /// anywhere, so `Close pane` and `Ctrl+Shift+W` were introduced to a reader
    /// twice, as strangers.
    ///
    /// Small right-aligned text and not key caps, which is what Windows and
    /// every editor on it draws in this slot — a cap here would be a second
    /// control-shaped thing in a row that already has a control column. The ink
    /// is the hint's, and for the hint's stated reason: an accelerator
    /// **reports**, it does not offer.
    ///
    /// The width travels with the words for [`Self::hint`]'s reason exactly: it
    /// is measured once, where the menu is laid out, and both the reservation
    /// and the drawing read that one number.
    ///
    /// `None` on a row whose verb is not a row of the shortcut table, and on
    /// every row wearing a `▸` — see [`accelerator_claim`]. A menu is not the
    /// shortcuts page, so a row with no chord prints nothing rather than the
    /// word for nothing.
    accel: Option<(String, f32)>,
    /// **Unsaved edits — the dot at the row's trailing edge.**
    ///
    /// A fact and not a string, since P1. It reached this row as `●` in the
    /// hint slot, which made the one dot in the window that is a *codepoint* sit
    /// two hundred pixels from three that are geometry: `marks.rs`'s R4 note
    /// says why that cannot stand, and this is the last of the four put right.
    /// The dot is struck at `crate::marks::DIRTY_DOT_LOGICAL_PX`, the same
    /// diameter the two preview heads and the file peek use.
    dirty: bool,
    hovered: bool,
    /// Whether this row can do what it says. A row that cannot is drawn and not
    /// offered — see [`hit`], which is where "not offered" is actually enforced.
    available: bool,
    /// The pin at the trailing edge, on rows that name something keepable.
    ///
    /// `None` on every row that names no such thing — `Browse…`, a profile, a
    /// context-menu verb — and those rows lose no width to a control they do not
    /// have.
    pin: Option<RowPin>,
}

/// **What one menu reserves for its accelerator column**, in physical pixels.
///
/// The widest chord any of `rows` carries, plus the flex row's own gap — or
/// nothing at all when not one of them has a chord, which is most menus in this
/// window and is a fact about the verbs rather than a gap in this mechanism
/// (see [`Row::accel`]).
///
/// **Reserved on every row and not on the ones that wear it**, which is the
/// argument the `▸` indicator has carried since it was written: a menu whose
/// width depended on which rows happened to have chords would change width the
/// day a second row grew one.
fn accelerator_claim(accels: &[Option<(String, f32)>], scale: f32) -> f32 {
    let widest = accels
        .iter()
        .filter_map(|accel| accel.as_ref().map(|(_, width)| *width))
        .fold(0.0_f32, f32::max);
    if widest <= 0.0 {
        return 0.0;
    }
    widest + ITEM_GAP_LOGICAL_PX * scale
}

/// The chord a row's verb answers to, measured, or `None`.
///
/// **A row wearing a `▸` never gets one**, and that is a ruling rather than an
/// omission: the trailing slot holds one thing, and a submenu heading is not a
/// verb a chord could run — it is a question about where, and the chord would
/// be describing the row below it.
fn accelerator_of(
    action: Option<crate::shortcuts::Action>,
    has_submenu: bool,
    shortcuts: &crate::shortcuts::Shortcuts,
    scale: f32,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> Option<(String, f32)> {
    if has_submenu {
        return None;
    }
    let text = shortcuts.accelerator(action?)?;
    let width = measure(&text, HINT_FONT_LOGICAL_PX * scale);
    Some((text, width))
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
    // The mark centred on its own 14px column — the slot's box for whichever
    // family this mark is in, centred in exactly the same column so that a row
    // with a cross and a row with a folder still line their names up. See
    // [`item_mark_box_logical_px`].
    let column_left = item[0] + px(ITEM_PADDING_X_LOGICAL_PX);
    let column_right = column_left + px(ITEM_ICON_COLUMN_LOGICAL_PX);
    if let Some(glyph) = row.mark {
        let [box_width, box_height] = item_mark_box_logical_px(glyph);
        let mark_width = px(box_width).round();
        let mark_height = px(box_height).round();
        let mark_left = ((column_left + column_right - mark_width) / 2.0).round();
        let mark_top = ((item[1] + item[3] - mark_height) / 2.0).round();
        let mut sprite = ChromeSprite::new(
            glyph,
            [
                mark_left,
                mark_top,
                mark_left + mark_width,
                mark_top + mark_height,
            ],
            palette.accent,
        );
        if !row.available {
            sprite.opacity = UNAVAILABLE_MARK_OPACITY;
            sprite.grayscale = true;
        }
        sprites.push(sprite);
    }
    // What the pin has claimed, out of the row's trailing padding and before the
    // hint gets there: the control sits at the very end of the row, so every
    // other trailing thing measures from where the pin stops.
    let pin_claim = row.pin.map_or(0.0, |_| row_pin_claim(scale));
    // What the hint has already claimed, out of the row's trailing padding: its
    // own measured width, and the `gap: 10px` between two flex items. A row with
    // nothing to add gives the name the whole span, which is what every row did
    // before any of them had a hint long enough to collide.
    // The accelerator sits outside the hint, which is the order Windows draws
    // them in: the chord is the row's own trailing annotation and a hint is a
    // second fact about the row's *subject*, so the chord is the further out of
    // the two. They co-occur on no row this build ships; the order is written
    // down so the day they do, the two do not land on each other.
    let accel_claim = row
        .accel
        .as_ref()
        .map_or(0.0, |(_, width)| width + px(ITEM_GAP_LOGICAL_PX));
    let hint_claim = row
        .hint
        .as_ref()
        .map_or(0.0, |(_, width)| width + px(ITEM_GAP_LOGICAL_PX));
    labels.push(ChromeLabel {
        mono: false,
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
            item[2] - px(ITEM_PADDING_X_LOGICAL_PX) - pin_claim - accel_claim - hint_claim,
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
    // **The chord, in the row's trailing padding** (系统性发现 ②). Right-aligned
    // into the whole row the way the hint is, so the two are one idiom and not
    // two, and drawn before the hint so that the further-out claim is the one
    // measured from the row's own edge.
    if let Some((accel, _)) = &row.accel {
        labels.push(ChromeLabel {
            mono: false,
            text: accel.clone(),
            rect: [
                item[0],
                item[1],
                item[2] - px(ITEM_PADDING_X_LOGICAL_PX) - pin_claim,
                item[3],
            ],
            font_size_px: px(HINT_FONT_LOGICAL_PX),
            // The hint's ink, on the hint's own written rule: this reports what
            // else runs this verb, it does not offer anything. It stays the
            // quiet ink under the pointer too — a chord that lit with the row
            // would read as a second thing being offered.
            color: palette.menu_item_hint_text,
            align_right: true,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        });
    }
    if let Some((hint, _)) = &row.hint {
        labels.push(ChromeLabel {
            mono: false,
            text: hint.clone(),
            rect: [
                item[0],
                item[1],
                item[2] - px(ITEM_PADDING_X_LOGICAL_PX) - pin_claim - accel_claim,
                item[3],
            ],
            font_size_px: px(HINT_FONT_LOGICAL_PX),
            // `--ink3` over `--menu`. It used to be `dialog_muted_text`,
            // which is the same ink over `--win` — the settings dialog's
            // surface, not this one. Identical in the light theme, six levels
            // adrift in the dark.
            // `--ink3` over `--menu`, for every hint there is. It was a
            // parameter until P1, with exactly one caller overriding it: the
            // preview switcher's dirty dot, which set it to `--accent` because
            // the dot is `--accent`. The dot is a sprite now and carries its
            // own ink, so what is left is the rule the parameter was an
            // exception to — a hint *reports* rather than *warns*.
            color: palette.menu_item_hint_text,
            align_right: true,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        });
    }
    // The dirty dot, in the cell the hint reserved for it. `--accent`, because
    // it is the same dot the head wears (mock-up 580-582) and the same drawing:
    // `marks::dirty_dot_sprite` is one function and four surfaces.
    if row.dirty {
        let diameter = (crate::marks::DIRTY_DOT_LOGICAL_PX * scale).round();
        let right = item[2] - px(ITEM_PADDING_X_LOGICAL_PX) - pin_claim;
        sprites.push(crate::marks::dirty_dot_sprite(
            [right - diameter, item[1], right, item[3]],
            palette.accent,
            scale,
        ));
    }
    // ── the pin (user ruling 2026-08-19) ────────────────────────────────────
    //
    // Drawn last so it sits over the trailing edge the two labels were just
    // held back from, and drawn at all only when it has something to say: a
    // pinned row wears its pin always, an unpinned one only while the hand is
    // on the row. An unpinned pin on a row nobody is pointing at would be a
    // column of grey pins down a menu, which is a list of offers pretending to
    // be a list of facts.
    if let Some(pin) = row.pin
        && (pin.filled || pin.revealed)
    {
        sprites.push(ChromeSprite::new(
            ChromeMark::Pin { filled: pin.filled },
            row_pin_rect(item, scale),
            // Three inks in one order of precedence, and the same order the row
            // label uses: the pin under the pointer is the one being offered to,
            // a pinned pin is a state and wears the mark ink every other state
            // glyph in this menu wears, and an offer nobody is on yet is the
            // menu's quietest ink.
            if pin.hovered {
                palette.menu_item_text_selected
            } else if pin.filled {
                palette.accent
            } else {
                palette.menu_item_hint_text
            },
        ));
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
/// so it wears the folder the pane is (`#i-folder` in `--accent`, mock-up 7427),
/// and a file wears the file the pane is — the same pairing `pane_mark` already
/// makes one module over, so a row and the head it reopens cannot wear two
/// different glyphs for one thing.
fn recent_mark(seed: &Seed, favicons: &crate::favicon::Favicons) -> ChromeMark {
    match seed {
        Seed::Term { profile_id, .. } => mark(index_of_id(profile_id)),
        Seed::Files { .. } => ActionIcon::FilesSeat.mark(),
        // A page wears the web class's globe and a file wears `#i-file`, through
        // the one door every preview row asks (`docs/DESIGN.md` §7.7 ⑤/⑥) —
        // **and the site's own icon where the session has one.** A vault row has
        // no pane and never did, which is exactly why the store is keyed by site
        // rather than by seat: what a row needs to look one up is a URL, and a
        // row is a URL. A page from a previous session wears the globe until
        // something opens it, because nothing on disk holds an icon (see
        // `favicon`'s module head).
        Seed::Preview { source, path } => crate::marks::preview_row_mark(
            *source == bt_persist::PreviewSourceV1::Url,
            match source {
                bt_persist::PreviewSourceV1::Url => favicons.of_url(path),
                bt_persist::PreviewSourceV1::File => None,
            },
        ),
        // **A window wears a window** (multiwindow slice D), and it is the one
        // row in this list whose mark is not its content's: a row captioned
        // `alpha` with a PowerShell mark says "that shell", and the whole
        // difference here is that pressing it brings back a *window* that had
        // `alpha` in it. `#i-max` is the drawing this product already uses to
        // mean a window — the caption button's own — so the two cannot drift.
        Seed::Window { .. } => ChromeMark::WindowMaximize,
    }
}

/// What a Recent row's tooltip says: the place, in full — and for a window, the
/// fact that it *is* one and how many tabs it had.
///
/// The row itself is one measured line and says a leaf name; the tip is where
/// this list has always put the whole path, and it is therefore where the one
/// word a window row cannot fit belongs.
fn recent_tip(seed: &Seed) -> String {
    match seed {
        Seed::Term { cwd, .. } => cwd.clone(),
        Seed::Files { root } => root.clone(),
        Seed::Preview { path, .. } => path.clone(),
        Seed::Window { seeds } => crate::i18n::recent_window_tip(seeds.len()),
    }
}

/// What a recent row calls itself — mock-up 7431: `r.seed.name || cwdLeaf(r.seed)`.
///
/// Your own name for the tab wins, and the folder it stood in answers when you
/// never gave it one. An empty manual name is not a name: `||` in the mock-up
/// falls through an empty string, and a row captioned with nothing would be a
/// row you cannot tell from the one above it.
fn recent_label(seed: &Seed) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;
    match seed {
        Seed::Term {
            cwd, manual_name, ..
        } => Cow::Borrowed(
            manual_name
                .as_deref()
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| cwd_leaf(cwd)),
        ),
        // A files locus has no name of its own; the mock-up captions it with the
        // same leaf rule applied to its root.
        Seed::Files { root } => Cow::Borrowed(cwd_leaf(root)),
        // And a file is captioned by its own last segment through the same rule,
        // which for a path ending in a file name is that file name — the string
        // a preview head already prints (§7.1.6h).
        // **And a page is captioned by its site**, `host[:port]`, because that is
        // the half of a URL §7.7 ③ calls its identity and the vault stores a
        // place rather than a title. `webnav::site_label` is the one splitter, so
        // the row cannot name a site the switcher key disagrees with.
        Seed::Preview {
            path,
            source: bt_persist::PreviewSourceV1::File,
        } => Cow::Borrowed(cwd_leaf(path)),
        Seed::Preview {
            path,
            source: bt_persist::PreviewSourceV1::Url,
        } => Cow::Owned(crate::webnav::site_label(path)),
        // **A window is captioned by the tab it opened with** (multiwindow slice
        // D) — or, since 2026-08-20, by the first one that can say what it is;
        // see [`Seed::first_tab`] for that and for why the word "window" is in
        // the tip rather than here. A window none of whose tabs can name
        // themselves never reaches the vault (`Seed::names_itself`), so the
        // fallback is a shape nothing constructs; it answers the empty string
        // because that is what an unnameable row already answers above.
        Seed::Window { .. } => seed.first_tab().map_or(Cow::Borrowed(""), recent_label),
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
///
/// One folder may have more than one of these — see [`RootNotes`], which is what
/// a row actually wears.
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
            Self::Home => crate::i18n::Text::RootNoteHome.text(),
            Self::Terminal => crate::i18n::Text::RootNoteTerminal.text(),
            Self::Parent => crate::i18n::Text::RootNoteParent.text(),
        }
    }
}

/// What separates two reasons on one row.
///
/// Punctuation and not a sentence, so it is written here rather than in the
/// string table — the same middle dot `PreviewTruncated` sets between "Read-only"
/// and a size, and it reads the same in both languages.
const ROOT_NOTE_JOIN: &str = " · ";

/// **Every** reason one folder is on the list (user report, 2026-08-19).
///
/// This used to be a single [`RootNote`], kept from whichever reason reached the
/// row first, and that is a bug with a name: when the folder above the root
/// happens to be the folder a shell is standing in — which is the ordinary case,
/// because you cd into a project and then root the column at a subfolder of it —
/// the row was offered as "a terminal is here" and the `parent` badge was
/// dropped. The way *up* was on the menu and unrecognisable, which is
/// indistinguishable from not being there.
///
/// A set and not a second `Option`: three reasons make seven badges, and "which
/// one wins" is a question with no honest answer, because both sentences are
/// true of the folder at the same time.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RootNotes {
    home: bool,
    terminal: bool,
    parent: bool,
}

impl RootNotes {
    /// The reasons in the order the list itself runs — most permanent address
    /// first, most local last — so a row's badge reads the same way round as the
    /// menu it is on.
    const ORDER: [RootNote; 3] = [RootNote::Home, RootNote::Terminal, RootNote::Parent];

    #[must_use]
    pub fn of(note: RootNote) -> Self {
        Self::default().and(note)
    }

    #[must_use]
    pub fn and(mut self, note: RootNote) -> Self {
        self.add(note);
        self
    }

    pub fn add(&mut self, note: RootNote) {
        match note {
            RootNote::Home => self.home = true,
            RootNote::Terminal => self.terminal = true,
            RootNote::Parent => self.parent = true,
        }
    }

    #[must_use]
    pub fn has(self, note: RootNote) -> bool {
        match note {
            RootNote::Home => self.home,
            RootNote::Terminal => self.terminal,
            RootNote::Parent => self.parent,
        }
    }

    /// The badge: every reason this row has, joined.
    #[must_use]
    pub fn text(self) -> String {
        Self::ORDER
            .into_iter()
            .filter(|note| self.has(*note))
            .map(RootNote::text)
            .collect::<Vec<_>>()
            .join(ROOT_NOTE_JOIN)
    }

    /// Every badge this menu could ever print.
    ///
    /// Enumerated rather than reasoned about, because the width reserved for the
    /// note column has to cover the widest badge *possible* and not the widest
    /// badge present — a menu that changed width because a shell moved would
    /// move under the pointer. Seven, because the empty set is not a row.
    pub fn every() -> impl Iterator<Item = Self> {
        (1u8..8).map(|bits| Self {
            home: bits & 1 != 0,
            terminal: bits & 2 != 0,
            parent: bits & 4 != 0,
        })
    }
}

/// One place the menu offers to point a column at.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootChoice {
    pub path: String,
    pub notes: RootNotes,
    /// Whether the user said to keep this one (user ruling 2026-08-19).
    ///
    /// A flag on the row rather than a second list, because the menu is one list
    /// with a heading in it — [`apply_pins`] puts the kept rows at the front and
    /// this is what tells the layout where the heading and the hairline go, and
    /// what tells the painter which pins are filled.
    pub pinned: bool,
}

/// **The kept folders first, then everything this window found** (user ruling
/// 2026-08-19).
///
/// One list and not two, for the reason the profile menu's Recent section is in
/// the same list as the profiles above it: a heading is a heading, and two lists
/// would mean two index spaces for one press to be resolved against.
///
/// A kept folder that this window *also* found — it is home, or a shell is
/// standing in it, or it is the folder above — is **lifted**, not copied: it
/// keeps every badge it earned and appears once, at the top. That is the same
/// "PINNED 与 MRU 不留双副本" rule the preview switcher obeys, said about
/// folders, and it is why this takes the whole [`RootChoice`] rather than only
/// its path.
///
/// A kept folder this window did not find is offered with no badge at all: it is
/// on the list because you put it there, and the section it is in says so.
#[must_use]
pub fn apply_pins(choices: Vec<RootChoice>, pinned: &[String]) -> Vec<RootChoice> {
    let mut rest = choices;
    let mut kept: Vec<RootChoice> = Vec::with_capacity(pinned.len());
    for path in pinned {
        let mut row = match rest.iter().position(|choice| &choice.path == path) {
            Some(index) => rest.remove(index),
            None => RootChoice {
                path: path.clone(),
                notes: RootNotes::default(),
                pinned: false,
            },
        };
        row.pinned = true;
        kept.push(row);
    }
    kept.extend(rest);
    kept
}

/// How many rows at the front of a menu's list are the kept ones.
///
/// Read off the list rather than carried beside it, so a layout and a painter
/// handed the same rows cannot disagree about where the heading goes.
fn pinned_run(choices: &[RootChoice]) -> usize {
    choices.iter().take_while(|choice| choice.pinned).count()
}

/// The places worth offering, in the mock-up's own order (E54).
///
/// **Home, then wherever the shells are standing, then one level up.** The order
/// is not alphabetical and is not most-recent-first: it runs from the most
/// permanent address this machine has to the most local one, so the list reads
/// the same on every window whatever the shells happen to be doing.
///
/// De-duplicated on the path, and a folder that arrives twice keeps its place
/// and **collects the second reason** rather than dropping it (user report,
/// 2026-08-19): a home directory a terminal is standing in is offered once, at
/// home's own position, saying both things.
#[must_use]
pub fn root_choices(root: &str, home: Option<&str>, cwds: &[String]) -> Vec<RootChoice> {
    let mut list: Vec<RootChoice> = Vec::new();
    let mut add = |path: &str, note: RootNote| {
        let path = path.trim();
        if path.is_empty() {
            return;
        }
        // The position is the *first* reason's and the badge is all of them.
        // Moving the row to the later reason's place would make the menu
        // re-order itself every time a shell moved, which is the thing the fixed
        // permanent-to-local order exists to prevent.
        if let Some(seen) = list.iter_mut().find(|choice| choice.path == path) {
            seen.notes.add(note);
            return;
        }
        list.push(RootChoice {
            path: path.to_owned(),
            notes: RootNotes::of(note),
            pinned: false,
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

/// What a point in the root menu is over: a row, or the pin at the end of one
/// (user ruling 2026-08-19).
///
/// Two verbs on one rectangle would be a menu where "go here" and "keep this"
/// were told apart by how far right you happened to click, decided twice — so
/// they are told apart once, here, and every caller downstream reads a verb
/// rather than a coordinate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootMenuHit {
    /// The row itself: point the column at this folder.
    Row(RootMenuRow),
    /// The pin: keep this folder, or stop keeping it. The menu stays open,
    /// because the row you just pinned has to be seen moving to the top.
    Pin(RootMenuRow),
}

impl RootMenuHit {
    /// Which row this is about, whichever half of it was hit.
    #[must_use]
    pub fn row(self) -> RootMenuRow {
        match self {
            Self::Row(row) | Self::Pin(row) => row,
        }
    }
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
    hover: Option<RootMenuHit>,
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

    pub fn set_hover(&mut self, hover: Option<RootMenuHit>) -> bool {
        let hover = self.open.and(hover);
        let changed = self.hover != hover;
        self.hover = hover;
        changed
    }

    pub fn hover(self) -> Option<RootMenuHit> {
        self.hover
    }
}

/// Every rectangle the root menu draws and hit-tests.
#[derive(Clone, Debug, PartialEq)]
pub struct RootMenuLayout {
    scale: f32,
    frame: [f32; 4],
    /// The `PINNED` heading, and the hairline under the rows it heads — both
    /// absent when nothing is pinned, because a heading over nothing is a
    /// section the reader has to work out is empty.
    pinned_label: Option<[f32; 4]>,
    pinned_separator: Option<[f32; 4]>,
    /// The `OPEN FOLDER` heading — absent when *its* section is empty, which is
    /// the sentence above applied to the other one. It used to be
    /// unconditional, and it could be: it was the menu's own title over the only
    /// list there was. Since a kept folder can be lifted out of that list
    /// (2026-08-19), the list can be emptied by pinning everything in it, and a
    /// heading standing over the hairline below it would be the menu telling the
    /// reader there is something there.
    label: Option<[f32; 4]>,
    /// Every row of the list, kept ones first. One vector and one index space —
    /// see [`apply_pins`], which is what puts them in this order.
    items: Vec<[f32; 4]>,
    /// The hairline above `Browse…` — unconditional, because the row below it is
    /// unconditional too. The profile menu's is an `Option` only because the
    /// Recent section it introduces can be empty.
    browse_separator: [f32; 4],
    browse: [f32; 4],
    /// Which way it grew — [`ProfileMenuLayout::travel`]'s field, derived the
    /// same way from this menu's own anchor.
    travel: Travel,
}

impl RootMenuLayout {
    /// Which way this menu grew out of the root button.
    #[must_use]
    pub fn travel(&self) -> Travel {
        self.travel
    }

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
    // pointer. Since 2026-08-19 a row may wear more than one badge, so the
    // widest is measured over every combination and not over the three reasons.
    let note = RootNotes::every()
        .map(|notes| measure(&notes.text(), px(HINT_FONT_LOGICAL_PX)))
        .fold(0.0, f32::max);
    let chrome = 2.0 * (border + padding) + 2.0 * px(ITEM_PADDING_X_LOGICAL_PX);
    // Every folder row carries a pin, so every folder row reserves one — the
    // same reservation the note makes, one control further out.
    let pin = row_pin_claim(scale);
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
                + pin
        })
        .fold(0.0, f32::max);
    // `Browse…` is measured with the rest rather than assumed to fit: it is a
    // translated string one day, and a row that overflowed the box it was not
    // counted into would be clipped by the very menu it belongs to.
    let content = content.max(
        px(ITEM_ICON_COLUMN_LOGICAL_PX)
            + px(ITEM_GAP_LOGICAL_PX)
            + measure(browse_text(), px(ITEM_FONT_LOGICAL_PX)),
    );
    let width = (chrome + content)
        .clamp(
            px(ROOT_MENU_MIN_WIDTH_LOGICAL_PX),
            px(ROOT_MENU_MIN_WIDTH_LOGICAL_PX + RECENT_ITEM_MAX_WIDTH_LOGICAL_PX),
        )
        .round();
    // The kept rows and their own heading, above everything, when there are any
    // (user ruling 2026-08-19: "排 home 之上,与下方段之间一条既有 hairline").
    let kept = pinned_run(choices);
    let found = choices.len() - kept;
    // **The rule between the two sections is drawn only when there are two.**
    // Keeping every place this window found leaves nothing under it, and a
    // hairline with nothing under it would sit directly on `Browse…`'s own — two
    // rules touching, which is a section boundary drawn twice for one boundary.
    let kept_block = if kept == 0 {
        0.0
    } else {
        section_block + item_height * kept as f32 + if found == 0 { 0.0 } else { separator_block }
    };
    let found_block = if found == 0 {
        0.0
    } else {
        section_block + item_height * found as f32
    };
    let height =
        (2.0 * (border + padding) + kept_block + found_block + separator_block + item_height)
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
    let mut items = Vec::with_capacity(choices.len());
    let hairline = |cursor: f32| {
        [
            content_left,
            cursor + separator_margin,
            content_right,
            cursor + separator_margin + separator_thickness,
        ]
    };
    let (pinned_label, pinned_separator) = if kept == 0 {
        (None, None)
    } else {
        let band = [content_left, cursor, content_right, cursor + section_block];
        cursor += section_block;
        for _ in 0..kept {
            items.push([content_left, cursor, content_right, cursor + item_height]);
            cursor += item_height;
        }
        let rule = (found > 0).then(|| {
            let rule = hairline(cursor);
            cursor += separator_block;
            rule
        });
        (Some(band), rule)
    };
    let label = (found > 0).then(|| {
        let band = [content_left, cursor, content_right, cursor + section_block];
        cursor += section_block;
        band
    });
    for _ in 0..found {
        items.push([content_left, cursor, content_right, cursor + item_height]);
        cursor += item_height;
    }
    let browse_separator = hairline(cursor);
    cursor += separator_block;
    let browse = [content_left, cursor, content_right, cursor + item_height];
    RootMenuLayout {
        scale,
        frame,
        pinned_label,
        pinned_separator,
        label,
        items,
        browse_separator,
        browse,
        travel: Travel::away_from(anchor, frame),
    }
}

/// What a point is over, with the same three answers [`hit`] gives and for the
/// same reasons.
#[must_use]
pub fn root_menu_hit(layout: &RootMenuLayout, x: f64, y: f64) -> Option<Option<RootMenuHit>> {
    let (x, y) = (x as f32, y as f32);
    for (index, item) in layout.items.iter().enumerate() {
        if contains(*item, x, y) {
            let row = RootMenuRow::Choice(index);
            // The pin is inside the row, so it is asked about first: a point on
            // the pin is on the row too, and the narrower answer is the true one.
            return Some(Some(if contains(row_pin_rect(*item, layout.scale), x, y) {
                RootMenuHit::Pin(row)
            } else {
                RootMenuHit::Row(row)
            }));
        }
    }
    if contains(layout.browse, x, y) {
        // No pin: `Browse…` names no folder yet, so there is nothing to keep.
        return Some(Some(RootMenuHit::Row(RootMenuRow::Browse)));
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
    hover: Option<RootMenuHit>,
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
    // The kept rows and their heading, above the list this window derived (user
    // ruling 2026-08-19).
    if let Some(band) = layout.pinned_label {
        labels.push(section_label(pinned_section_label(), band, scale, palette));
    }
    if let Some(band) = layout.label {
        labels.push(section_label(root_section_label(), band, scale, palette));
    }

    for (index, (item, choice)) in layout.items.iter().zip(choices).enumerate() {
        let note = choice.notes.text();
        let width = measure(&note, px(HINT_FONT_LOGICAL_PX));
        let row = RootMenuRow::Choice(index);
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
                // No row of this menu is a row of the shortcut table (系统性发现 ②).
                accel: None,
                dirty: false,
                hovered: hover.map(RootMenuHit::row) == Some(row),
                available: true,
                pin: Some(RowPin {
                    filled: choice.pinned,
                    hovered: hover == Some(RootMenuHit::Pin(row)),
                    revealed: hover.map(RootMenuHit::row) == Some(row),
                }),
            },
            scale,
            palette,
            &mut quads,
            &mut labels,
            &mut sprites,
        );
    }

    if let Some(rule) = layout.pinned_separator {
        quads.push(OverlayQuad {
            rect: rule,
            color: palette.menu_border,
            alpha: separator_alpha(palette.menu_border),
        });
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
            //
            // Which is also why it is the **struck** rendition since P2 while
            // they keep the solid one: they are places and this is a question.
            mark: Some(ActionIcon::BrowseForFolder.mark()),
            name: browse_text(),
            // No note. Every row above answers "why is this offered?"; this one is
            // offered because nothing else was, and a hint saying so would be the
            // menu apologising for itself.
            hint: None,
            // No row of this menu is a row of the shortcut table (系统性发现 ②).
            accel: None,
            dirty: false,
            hovered: hover == Some(RootMenuHit::Row(RootMenuRow::Browse)),
            // The system always has a folder picker; there is no machine on which
            // this row is a promise the window cannot keep.
            available: true,
            // No pin. This row names no folder — it names the question — and a
            // pin on it would have nothing to keep.
            pin: None,
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
fn root_section_label() -> &'static str {
    crate::i18n::Text::RootSection.text()
}

/// The heading over the kept rows, in **both** menus that have them.
fn pinned_section_label() -> &'static str {
    crate::i18n::Text::PinnedSection.text()
}

/// The escape hatch's own words (mock-up 5151).
///
/// The ellipsis is load-bearing and is the real character rather than three
/// dots: it is the platform convention for "this opens something that will ask
/// you again", which is exactly the promise that separates this row from the
/// rows above it, each of which commits the moment it is pressed.
fn browse_text() -> &'static str {
    crate::i18n::Text::RootBrowse.text()
}

/// `.root-menu { min-width: 190px }` (mock-up 629).
///
/// Its own constant rather than the profile picker's 180: the mock-up gives the
/// two menus different floors, and this one is wider because every row in it is
/// a directory name followed by a note, where the picker's rows are short shell
/// names. Sharing the number made the root menu 10px narrower than drawn.
const ROOT_MENU_MIN_WIDTH_LOGICAL_PX: f32 = 190.0;

// ── the file row's context menu (K143/K145) ─────────────────────────────────

/// **Which kind of tree row a right press landed on** (user ruling 2026-08-25).
///
/// The menu used to refuse a folder outright — `Runtime::open_file_menu`'s own
/// note said "all three verbs are about a *file*", and while the three were
/// `Open preview / Copy path / Insert path into terminal` that was true. The
/// ruling ends it: a folder has a path, so two of those three were always
/// answerable over one, and it has two verbs of its own that no other surface
/// offers at the row — the fold, and a shell standing in it.
///
/// **A snapshot, and the fold is why.** `expanded` is read once when the menu
/// comes up and never again, on [`crate::TermMenuState`]'s own rule: the row's
/// first verb *is* the fold, so pressing it changes the answer while the menu is
/// still on screen, and a menu that re-asked every frame would rewrite its own
/// first row under a hand already moving toward it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileMenuSubject {
    /// A file row.
    File,
    /// A folder row, with the fold it was standing in when the menu came up.
    Folder { expanded: bool },
    /// **A path a preview's breadcrumb row names** (user ruling 2026-08-24) —
    /// the `Open ⌄` pill's document.
    ///
    /// The third face, and the one that is not a tree row at all. It is on this
    /// enum rather than in a menu of its own because 「系统默认程序 / 在 files 列
    /// 中定位」are verbs about a *path*, and a window that grew a second list for
    /// them would be a window where the two could come to disagree. What it does
    /// not share is the row set: there is no `Open preview` on a surface that
    /// *is* the preview, no fold on something that is not a row, and no
    /// `Reveal in files column` on the column itself — see [`file_menu`].
    ///
    /// It used to be the `…` chip's face as well; that is [`Self::FoldedPath`]
    /// now, and the reason is written there.
    Document,
    /// **The levels a folded breadcrumb is standing in for** (user ruling
    /// 2026-08-25) — the `…` chip's own list.
    ///
    /// The fourth face, and the only one whose rows are *places* rather than
    /// verbs. It exists because the chip used to raise [`Self::Document`] on the
    /// deepest folder the fold was hiding, and that was the wrong sentence twice
    /// over: it offered `Copy path` and `Insert path` for **one** of the several
    /// folders behind the chip without saying which, and it answered a control
    /// that means *there are folders here you cannot see* with a menu that never
    /// names them. Windows Explorer's own breadcrumb `…` is the reference the
    /// ruling gave: it lists the hidden levels, and pressing one goes there.
    ///
    /// `levels` is how many the fold is hiding, which is all this face needs to
    /// know to say how many rows it has; **what** they are called is the look's
    /// [`FileMenuLook::crumbs`], because a name is a string and this enum is
    /// `Copy`.
    FoldedPath { levels: usize },
}

/// One row of the menu a tree row raises under the pointer, or a preview's
/// breadcrumb under its `Open ⌄`.
///
/// **A flat list across every subject rather than an enum per subject**, which
/// is [`GitMenuRow`]'s own reasoning: `Copy path` and `Insert path into
/// terminal` mean the same thing on all three faces, the runtime's dispatch is
/// one `match`, and two variants for one verb would be two code paths to keep in
/// step. Which rows a face actually shows is [`file_menu`]'s answer and not this
/// enum's — the type is shared, the lists are not.
///
/// The mock-up's `Save as…` (8088) is still not here: it is conditional on a
/// *terminal artefact* and is raised from the inline-image path, not from a row
/// of the tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileMenuRow {
    /// Open the file on the preview seat — the same verb its double click has.
    ///
    /// A tree's file row only. The breadcrumb's menu is raised *by* a preview,
    /// so this door leads to where the reader already is; its way in is
    /// [`Self::OpenWith`].
    Open,
    /// Hand the path to whatever this machine has registered for it (user
    /// rulings 2026-08-24 and 2026-08-25).
    ///
    /// **One row, two wordings**, which is [`Self::Fold`]'s arrangement and its
    /// reason: on a tree row it is the *second* door, under `Open preview`, and
    /// says `Open with default app`; on a breadcrumb it is the *first* and only
    /// door, so it says `Open in default app` — see [`Self::text`].
    OpenWith,
    // `OpenWithEditor` is **retired** (user ruling 2026-08-25: 「既然有 default
    // app 了,就把 VS Code 去掉」). It was the one row here that was not offered on
    // every machine, and the whole reason this menu's length was a function of
    // anything but its subject. The door above it — `Open with default app` —
    // hands the path to whatever this machine has registered for it, which on the
    // machine that had the editor row is very often that editor; a second row
    // naming one program by name was this window picking a favourite.
    /// Unfold the folder, or fold it — one row wearing the face of the state it
    /// is standing in.
    Fold,
    /// A tab of the default profile, standing in this folder.
    NewTerminal,
    /// **Change the name this row has on the disk** (B5, user ruling
    /// 2026-08-25).
    ///
    /// A tree row only — a file's or a folder's. The `Document` face's name is
    /// changed by double-clicking the last segment of the breadcrumb the pill
    /// stands on, which is the same editor one surface over, and the
    /// `FoldedPath` face carries no verb about any one of the folders it hides.
    ///
    /// It is a *door* rather than a path question, so it stands above the rule:
    /// [`Self::hands_out_the_path`] is about handing the text of a path
    /// somewhere, and this changes the thing the path points at.
    Rename,
    // `ShowInFiles` is **retired** (user ruling 2026-08-25). It was the
    // breadcrumb face's answer to "where does this live", and the breadcrumb it
    // was hung under grew the same answer four times over: every segment of the
    // path stands the column on that level, softly, without leaving the tree.
    // A row that repeats the row above it is one surface saying a thing twice.
    // The verb itself is not lost — `Runtime::locate_folder_in_files_column` is
    // what the segments press, and it is the one this row used to reach.
    CopyPath,
    InsertPath,
    /// Show the row in Explorer: a file selected inside its folder, a folder
    /// opened as itself.
    Reveal,
    /// **One level a folded breadcrumb is standing in for** (user ruling
    /// 2026-08-25) — a row of [`FileMenuSubject::FoldedPath`]'s list.
    ///
    /// It carries an index into [`FileMenuLook::crumbs`] and not a depth into
    /// the path, because the two are not the same order: the ruling asks for
    /// **deepest first** (「由深到浅排,最近的隐藏级在最上,一路到根方向」),
    /// which is Explorer's own order and the reverse of the row the chip stands
    /// in. Numbering the rows by the list they are drawn from is what keeps the
    /// paint, the hit test and the press from each having their own opinion
    /// about which way up it is; turning the list round is done once, by
    /// whoever builds the look.
    Crumb(usize),
}

/// **Everything one file menu needs that a [`FileMenuSubject`] cannot carry.**
///
/// [`GitMenuLook`]'s shape and its reason: two of these rows say a word that is
/// a fact about the *machine* or about the *path*, not about the menu, and a
/// `Copy` enum cannot hold a string. Bundled rather than passed one by one
/// because the layout, the paint and the walk all need the same set and three
/// argument lists that can drift apart is exactly the bug this menu already had
/// once, when the editor row's caption and the flag that offered it were two
/// parameters.
#[derive(Clone, Copy, Debug)]
pub struct FileMenuLook<'a> {
    pub subject: FileMenuSubject,
    /// The names of the folded levels, **deepest first** — the rows of
    /// [`FileMenuSubject::FoldedPath`]. Empty for every other face.
    pub crumbs: &'a [String],
    /// The mark `New terminal here` wears: **the default profile's own shape,
    /// in this column's ink** (user rulings 2026-08-25, twice).
    ///
    /// It is passed in rather than read here because "which profile is the
    /// default" is a setting, and this module's table does not hold the setting —
    /// `Runtime::default_profile` does, and it is the one reader every other
    /// caller of that question already goes through.
    ///
    /// **A line rendition and not the coloured mark** (the second ruling of the
    /// day, on the first's drawing): the shape is kept — the row is still about
    /// the shell it is about to open — and the *style* joins the eight thin
    /// monochrome glyphs it stands in a column with. See
    /// [`ChromeMark::in_line`], which is where the mark loses its colours, and
    /// `every_file_menu_row_wears_the_columns_own_ink`, which is what stops one
    /// arriving here with them still on.
    pub terminal: ChromeMark,
}

impl FileMenuRow {
    /// What the row says, wearing this look.
    ///
    /// The look is a parameter rather than the caller's string because a menu
    /// that took its words from the call site is a menu whose wording can
    /// disagree with what the press does. **Two rows' wording turns on the
    /// subject** — the fold, which is a toggle, and the default-app door, which
    /// is a second door on a tree row and the only one on a breadcrumb — and one
    /// row's is a name the look is carrying, which is the folded level's own.
    ///
    /// A folded level whose name the look has not got says nothing, and that is
    /// the honest answer: `file_menu` mints exactly as many `Crumb` rows as the
    /// subject declares levels, so an empty string here means the two were built
    /// from different lists.
    #[must_use]
    pub fn text<'a>(self, look: &FileMenuLook<'a>) -> &'a str {
        match self {
            Self::Open => crate::i18n::Text::FileMenuOpenPreview.text(),
            // `with` on a tree row, where it stands under `Open preview` and the
            // preposition is doing the contrasting; `in` on a breadcrumb, where
            // it is the first row of the list an `Open ⌄` pill opened and has
            // nothing above it to contrast with.
            Self::OpenWith => {
                if matches!(look.subject, FileMenuSubject::Document) {
                    crate::i18n::Text::FileMenuOpenDefaultApp.text()
                } else {
                    crate::i18n::Text::FileMenuOpenWith.text()
                }
            }
            Self::Fold => {
                if matches!(look.subject, FileMenuSubject::Folder { expanded: true }) {
                    crate::i18n::Text::FolderMenuCollapse.text()
                } else {
                    crate::i18n::Text::FolderMenuExpand.text()
                }
            }
            Self::NewTerminal => crate::i18n::Text::FolderMenuNewTerminal.text(),
            Self::Rename => crate::i18n::Text::FileMenuRename.text(),
            Self::CopyPath => copy_path_text(),
            Self::InsertPath => insert_path_text(),
            Self::Reveal => reveal_in_explorer_text(),
            Self::Crumb(at) => look.crumbs.get(at).map_or("", String::as_str),
        }
    }

    /// Whether this row **hands the row's path to somewhere else**, which is
    /// what the separator divides on.
    ///
    /// A property of the row rather than a position in a list — [`GitMenuRow`]'s
    /// `writes` read for this menu's own division. Mock-up 8089 drew the rule
    /// under `Open` and named what it separates: *what this row is* above, *what
    /// its path is* below. Stated as a property, the rule survives a menu with
    /// two rows above the line instead of one, and cannot be broken by adding a
    /// row in the wrong place — which is exactly what happened when the menu
    /// grew from three rows to nine across three faces.
    ///
    /// **[`Self::Reveal`] is below the rule on a tree row and above it on a
    /// breadcrumb** (user ruling 2026-08-25), which is why this takes the
    /// subject at all — and it is a difference in what the row *is* on the two
    /// faces rather than a placement bolted on.
    ///
    /// On a tree row the reader is already looking at a list of files with this
    /// one in it, so "open the folder it lives in" is a third thing to do *with
    /// the path*, beside copying it and typing it. On a breadcrumb there is no
    /// such list — the pane is showing the file's contents — so the same verb is
    /// the face's answer to "where does this live", which is a way in. It
    /// inherited that place from `Show in files column`, the row it replaced,
    /// which stood above the rule for exactly this reason.
    #[must_use]
    pub fn hands_out_the_path(self, subject: FileMenuSubject) -> bool {
        match self {
            Self::CopyPath | Self::InsertPath => true,
            Self::Reveal => !matches!(subject, FileMenuSubject::Document),
            _ => false,
        }
    }

    /// The mark in the row's icon column — **the row's verb, asked of
    /// [`ActionIcon`]**, which is the one table that answers what a verb looks
    /// like. What stays here is why each row *is* that verb.
    fn mark(self, look: &FileMenuLook<'_>) -> ChromeMark {
        match self {
            Self::Open => ActionIcon::OpenFile.mark(),
            // `#i-external` — the bare arrow that means *this content leaves the
            // window*, which is exactly what the preview head's `↗` already uses
            // it to say about the same act.
            Self::OpenWith => ActionIcon::OpenWith.mark(),
            // The tree's own triangle at the angle the tree turns it to, so the
            // mark turns with the word. Not a second glyph for a second kind of
            // disclosure: a reader who has opened a folder in this window has
            // already learned what a turned triangle means.
            Self::Fold => ActionIcon::FoldFolder.turned(
                if matches!(look.subject, FileMenuSubject::Folder { expanded: true }) {
                    1.0
                } else {
                    0.0
                },
            ),
            // **The terminal this row makes, and not the folder it is standing
            // in** (user ruling 2026-08-25). It wore `#i-folder` until then, on
            // the reasoning that the pane menu's `New terminal in folder…` wears
            // one too — and that reasoning was about the wrong half of the two
            // rows. The pane menu's row ends in three dots because it asks
            // *which folder* before anything happens, so a folder is what it is
            // about; this row already knows the folder, because you right-clicked
            // it, and what it makes is a shell. So it wears the mark that shell
            // wears everywhere else this window draws one — the default profile's
            // own, the same glyph its tab and its pane head carry.
            //
            // **The glyph, and not its colours** (the same day's second ruling).
            // A tab and a pane head *identify* a session, which is what a
            // profile mark's fixed colours are for; a menu row is a verb in a
            // column of verbs, and the one thing every other glyph in that
            // column has in common is that it is struck in the row's own ink.
            // The look's own note carries the argument; the mark arrives here
            // already stripped, because `Runtime::file_menu_look` is the one
            // place that knows which profile is the default.
            Self::NewTerminal => look.terminal,
            // The pencil, which is this window's one glyph for *you are about to
            // write this yourself* — the shortcut page already spends it on the
            // row a chord is being recorded into. Not the row's own kind: a
            // folder row and a file row take this verb identically, and a mark
            // that changed with the subject would be saying which kind of thing
            // is being renamed, which the row above it already said.
            Self::Rename => ActionIcon::RenameFile.mark(),
            Self::CopyPath => ActionIcon::CopyPath.mark(),
            Self::InsertPath => ActionIcon::InsertPath.mark(),
            // The Git row's `Reveal in Explorer` wears this one, for the reason
            // its string is shared: it is the same verb, so it is the same entry
            // in the registry.
            Self::Reveal => ActionIcon::RevealInFolder.mark(),
            // A shut folder, which is what every one of these levels is: a place
            // in the tree the reader has not got open in front of them. The tree
            // draws its own shut folders with this glyph, so the chip's list
            // reads as a run of folders rather than as a run of verbs.
            Self::Crumb(_) => ActionIcon::FolderObject.mark(),
        }
    }
}

/// The rows one subject offers, and where the rule falls.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileMenu {
    pub rows: Vec<FileMenuRow>,
    /// The index the separator is drawn **under**. Derived from
    /// [`FileMenuRow::hands_out_the_path`] rather than written down per subject,
    /// exactly as [`GitMenu::separator_after`] is.
    pub separator_after: Option<usize>,
}

/// What this subject's menu holds.
///
/// **Every face is its ways in, then the rule, then what its path is**, and that
/// shape is the ruling rather than a symmetry to be admired: the rows above the
/// line are what this thing *is* (open it, or fold it, and then the other doors
/// this kind has), and the rows below are what its *path* is. Which questions
/// live below the line is the same wherever the menu is raised; which doors live
/// above it is what the three faces disagree about, and each one's list is here
/// rather than at its call site:
///
/// - **[`FileMenuSubject::File`]** — the tree's file row. Two doors: this
///   window's preview seat and the machine's default program. Then
///   `Copy path / Insert path / Reveal in Explorer`. No `Show in files column`:
///   the press that raised it *was* in the files column.
/// - **[`FileMenuSubject::Folder`]** — the tree's folder row. Two verbs no other
///   surface offers at the row — the fold, and a shell standing in it — and then
///   the same three path questions. A folder has no preview seat.
/// - **[`FileMenuSubject::Document`]** — a preview's breadcrumb. No preview door
///   (the preview is what asked), so the default program is the first row, then
///   `Show in files column`, which is this face's answer to "where does this
///   live" because the column is somewhere else from here.
/// - **[`FileMenuSubject::FoldedPath`]** — the `…` chip. The one face with no
///   verbs in it at all: as many rows as the fold is hiding levels, deepest
///   first, and no rule, because there is nothing here to divide *what this is*
///   from *what its path is* — every row is a place.
///
/// **Nothing outside the subject changes the length of a list any more** (user
/// ruling 2026-08-25). The editor row was the one thing that did, and it is
/// retired; what is left is the property the original three-row menu had and
/// briefly lost — every reader of this table gets the same table for the same
/// subject, so no reader can index it with a number another reader computed.
#[must_use]
pub fn file_menu(subject: FileMenuSubject) -> FileMenu {
    let mut rows = Vec::with_capacity(6);
    match subject {
        FileMenuSubject::File => {
            rows.push(FileMenuRow::Open);
            rows.push(FileMenuRow::OpenWith);
            rows.push(FileMenuRow::Rename);
        }
        FileMenuSubject::Folder { .. } => {
            rows.push(FileMenuRow::Fold);
            rows.push(FileMenuRow::NewTerminal);
            rows.push(FileMenuRow::Rename);
        }
        // **`Reveal in Explorer` and not `Show in files column`** (user ruling
        // 2026-08-25). Two rulings of the same day met on this row. The
        // breadcrumb the pill stands on is now段段可点 with soft semantics, so
        // "take me to this level of the files column" is already offered four
        // times over in the row above the menu, and a list that repeated it
        // would be one surface saying the same thing twice. And the verb this
        // face was refused — Explorer — was refused because "the pane's own foot
        // already carries it one band up"; the page's foot is gone and the
        // file's foot retired into the breadcrumb, so that reason has expired
        // and the verb has nowhere else to live.
        FileMenuSubject::Document => {
            rows.push(FileMenuRow::OpenWith);
            rows.push(FileMenuRow::Reveal);
        }
        // The whole list, and then out: a folded level is not a path question
        // and has no path questions of its own here. Asking `Copy path` about
        // *one* of several hidden folders is what this face was invented to stop
        // — see [`FileMenuSubject::FoldedPath`].
        FileMenuSubject::FoldedPath { levels } => {
            rows.extend((0..levels).map(FileMenuRow::Crumb));
            return FileMenu {
                rows,
                separator_after: None,
            };
        }
    }
    rows.push(FileMenuRow::CopyPath);
    rows.push(FileMenuRow::InsertPath);
    // Explorer is a tree row's third path question, below the rule with the
    // other two. On a breadcrumb it is a *way in* and it is already above the
    // rule — see the `Document` arm and [`FileMenuRow::hands_out_the_path`].
    if !matches!(subject, FileMenuSubject::Document) {
        rows.push(FileMenuRow::Reveal);
    }
    let separator_after = rows
        .iter()
        .position(|row| row.hands_out_the_path(subject))
        .filter(|at| *at > 0 && *at < rows.len())
        .map(|at| at - 1);
    FileMenu {
        rows,
        separator_after,
    }
}

/// The row `steps` away, stopping at the ends rather than wrapping round.
///
/// Clamped, not cyclic, because the tree this menu was raised from clamps too
/// (D45): one window should not hold two different ideas of what the bottom of a
/// list does. From nowhere, a step in either direction lands on the end it came
/// from — pressing Up on a fresh menu offers the last row, which is the
/// convention every platform menu keeps.
///
/// Every row of this menu can do what it says, so unlike [`git_menu_step`] there
/// is nothing here to skip: the refusals these verbs *can* meet (a program the
/// tree will not run, a shell that has gone) happen after the press and are
/// spoken then, which is the same answer the double click gives.
///
/// **The walk is over the rows this menu is actually showing**, which is what
/// taking a subject rather than the enum's whole vocabulary buys: a keyboard
/// that stepped over every variant would offer a fold on a file and a folded
/// level on a menu that has no fold behind it.
///
/// `None` for a subject with no rows, which is the one thing that can happen
/// without a bug: `FoldedPath { levels: 0 }`. `Runtime::open_file_menu` refuses
/// to raise such a menu at all, so nothing this window draws can ask — and
/// answering it here rather than indexing into an empty list is what keeps that
/// refusal from being the only thing between this walk and a panic.
#[must_use]
pub fn file_menu_step(
    subject: FileMenuSubject,
    current: Option<FileMenuRow>,
    forwards: bool,
) -> Option<FileMenuRow> {
    let rows = file_menu(subject).rows;
    let last = rows.len().checked_sub(1)?;
    let Some(current) = current else {
        return Some(if forwards { rows[0] } else { rows[last] });
    };
    // **A hover this list does not hold does not wedge the walk.** It can happen
    // — every face has rows the others have not — and the answer is the end the
    // step was travelling towards, which is *not* what a fresh menu answers: a
    // hand already moving down is asking for the row after the one it thinks it
    // is on, and the honest last-resort answer to that is the bottom.
    let at = rows
        .iter()
        .position(|found| *found == current)
        .unwrap_or(if forwards { last } else { 0 });
    let next = if forwards {
        (at + 1).min(last)
    } else {
        at.saturating_sub(1)
    };
    Some(rows[next])
}

/// One drawn row of the file menu.
///
/// The row and its rectangle are **paired**, so that the paint and the hit test
/// cannot disagree about which row is where on a machine that has one row more
/// than another.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FileMenuItem {
    row: FileMenuRow,
    rect: [f32; 4],
}

/// Every rectangle the file menu draws and hit-tests.
#[derive(Clone, Debug, PartialEq)]
pub struct FileMenuLayout {
    scale: f32,
    frame: [f32; 4],
    /// The rows this menu is showing, each with the rectangle it was placed at.
    items: Vec<FileMenuItem>,
    /// The rule under the last row that acts on the row itself — mock-up 8089,
    /// which separates *what this row is* from *what its path is*.
    separator: Option<[f32; 4]>,
    /// Which way it grew — [`ProfileMenuLayout::travel`]'s field.
    travel: Travel,
}

impl FileMenuLayout {
    /// Which way this menu grew out of the press that raised it.
    #[must_use]
    pub fn travel(&self) -> Travel {
        self.travel
    }
}

/// **The anchor a menu raised at the pointer grew from**: the point itself.
///
/// A press has no rectangle, and where it landed is the whole of what raised the
/// menu — so the anchor is that point with no width and no height, which
/// [`Travel::away_from`] reads exactly as it reads a button. Four of this
/// window's eight menus are raised this way and all four say so through here,
/// rather than each spelling the degenerate rectangle out.
fn pressed_at(point: [f32; 2]) -> [f32; 4] {
    [point[0], point[1], point[0], point[1]]
}

/// `Insert path into terminal` — the widest row of either menu, and the reason
/// the menu is measured rather than given a fixed width.
pub fn insert_path_text() -> &'static str {
    crate::i18n::Text::FileMenuInsertPath.text()
}
pub fn copy_path_text() -> &'static str {
    crate::i18n::Text::FileMenuCopyPath.text()
}
/// **One verb, three menus** — see [`crate::i18n::Text::MenuRevealInExplorer`].
#[must_use]
pub fn reveal_in_explorer_text() -> &'static str {
    crate::i18n::Text::MenuRevealInExplorer.text()
}
// `show_in_files_text` retired with `FileMenuRow::ShowInFiles` (user ruling
// 2026-08-25), and `i18n::Text::FileMenuShowInFiles` with it.

/// The menu hung under the point a row was right-clicked at.
///
/// **A point, not a widget.** Every other popup in this window hangs off a
/// button and must therefore re-find that button after a re-render (E59/E60).
/// This one is raised at the pointer, so the anchor is a coordinate that no
/// re-layout can move or destroy — which is also why it does not need the live
/// re-measure the root menu pays for on every frame.
#[must_use]
pub fn file_menu_layout(
    point: [f32; 2],
    surface: (f32, f32),
    scale: f32,
    look: &FileMenuLook<'_>,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> FileMenuLayout {
    let px = |value: f32| value * scale;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let padding = px(MENU_PADDING_LOGICAL_PX);
    let item_height = px(ITEM_HEIGHT_LOGICAL_PX).round();
    let separator_thickness = (SEPARATOR_THICKNESS_LOGICAL_PX * scale).round().max(1.0);
    let separator_margin = px(SEPARATOR_MARGIN_Y_LOGICAL_PX).round();
    let separator_block = 2.0 * separator_margin + separator_thickness;

    let menu = file_menu(look.subject);
    let chrome = 2.0 * (border + padding) + 2.0 * px(ITEM_PADDING_X_LOGICAL_PX);
    let row_width = |text: &str, measure: &mut dyn FnMut(&str, f32) -> f32| {
        px(ITEM_ICON_COLUMN_LOGICAL_PX)
            + px(ITEM_GAP_LOGICAL_PX)
            + measure(text, px(ITEM_FONT_LOGICAL_PX))
    };
    let content = menu.rows.iter().fold(0.0f32, |wide, row| {
        wide.max(row_width(row.text(look), measure))
    });
    let width = (chrome + content)
        .max(px(FILE_MENU_MIN_WIDTH_LOGICAL_PX))
        .round();
    #[allow(clippy::cast_precision_loss)]
    let rows_height = menu.rows.len() as f32 * item_height;
    let height = (2.0 * (border + padding)
        + rows_height
        + menu.separator_after.map_or(0.0, |_| separator_block))
    .round();

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
    let mut items = Vec::with_capacity(menu.rows.len());
    let mut separator = None;
    for (at, row) in menu.rows.iter().enumerate() {
        items.push(FileMenuItem {
            row: *row,
            rect: [content_left, cursor, content_right, cursor + item_height],
        });
        cursor += item_height;
        // The rule goes under the last row that is not yet a path question,
        // which is where it has always gone: it used to be "under `Open`"
        // because `Open` was the only one above it.
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
    FileMenuLayout {
        scale,
        frame,
        items,
        separator,
        travel: Travel::away_from(pressed_at(point), frame),
    }
}

/// What a point is over, with the same three answers the other menus give.
#[must_use]
pub fn file_menu_hit(layout: &FileMenuLayout, x: f64, y: f64) -> Option<Option<FileMenuRow>> {
    let (x, y) = (x as f32, y as f32);
    for item in &layout.items {
        if contains(item.rect, x, y) {
            return Some(Some(item.row));
        }
    }
    contains(layout.frame, x, y).then_some(None)
}

/// The file menu as one overlay layer.
#[must_use]
pub fn file_menu_build(
    layout: &FileMenuLayout,
    look: &FileMenuLook<'_>,
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

    for item in &layout.items {
        push_row(
            &Row {
                rect: item.rect,
                mark: Some(item.row.mark(look)),
                name: item.row.text(look),
                hint: None,
                // No row of this menu is a row of the shortcut table (系统性发现 ②).
                accel: None,
                dirty: false,
                hovered: hover == Some(item.row),
                // Every verb here acts on a path this process enumerated. There
                // is no machine on which one of them is a promise that cannot be
                // kept — the refusals these verbs *can* meet (a program the tree
                // will not run, a shell that has gone) happen after the press
                // and are spoken then, which is the same answer the double
                // click gives.
                available: true,
                pin: None,
            },
            scale,
            palette,
            &mut quads,
            &mut labels,
            &mut sprites,
        );
    }
    if let Some(rect) = layout.separator {
        quads.push(OverlayQuad {
            rect,
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

#[must_use]
pub fn git_menu_checkout_text() -> &'static str {
    crate::i18n::Text::GitMenuCheckout.text()
}
#[must_use]
pub fn git_menu_create_branch_text() -> &'static str {
    crate::i18n::Text::GitMenuCreateBranch.text()
}
#[must_use]
pub fn git_menu_create_tag_text() -> &'static str {
    crate::i18n::Text::GitMenuCreateTag.text()
}
#[must_use]
pub fn git_menu_rename_branch_text() -> &'static str {
    crate::i18n::Text::GitMenuRename.text()
}
#[must_use]
pub fn git_menu_delete_branch_text() -> &'static str {
    crate::i18n::Text::GateDelete.text()
}
#[must_use]
pub fn git_menu_delete_tag_text() -> &'static str {
    crate::i18n::Text::GitMenuDeleteTag.text()
}
#[must_use]
pub fn git_menu_checkout_tracking_text() -> &'static str {
    crate::i18n::Text::GitMenuCheckoutTracking.text()
}
#[must_use]
pub fn git_menu_stage_text() -> &'static str {
    crate::i18n::Text::GitActStage.text()
}
#[must_use]
pub fn git_menu_unstage_text() -> &'static str {
    crate::i18n::Text::GitActUnstage.text()
}
#[must_use]
pub fn git_menu_discard_text() -> &'static str {
    crate::i18n::Text::GateDiscard.text()
}
#[must_use]
pub fn git_menu_open_diff_text() -> &'static str {
    crate::i18n::Text::GitMenuOpenDiff.text()
}
/// The Git row's spelling of the verb, which is the tree row's spelling of it:
/// one function so the two menus cannot come apart, on
/// [`git_menu_copy_path_text`]'s own precedent one line down.
#[must_use]
pub fn git_menu_reveal_text() -> &'static str {
    reveal_in_explorer_text()
}
pub fn git_menu_copy_path_text() -> &'static str {
    crate::i18n::Text::FileMenuCopyPath.text()
}
#[must_use]
pub fn git_menu_copy_hash_text() -> &'static str {
    crate::i18n::Text::GitMenuCopyHash.text()
}
#[must_use]
pub fn git_menu_copy_subject_text() -> &'static str {
    crate::i18n::Text::GitMenuCopySubject.text()
}
#[must_use]
pub fn git_menu_copy_name_text() -> &'static str {
    crate::i18n::Text::GitMenuCopyName.text()
}
#[must_use]
pub fn git_menu_compare_selected_text() -> &'static str {
    crate::i18n::Text::GitMenuCompareSelected.text()
}
#[must_use]
pub fn git_menu_compare_working_text() -> &'static str {
    crate::i18n::Text::GitMenuCompareWorking.text()
}

impl GitMenuRow {
    /// What the row says.
    #[must_use]
    pub fn text(self) -> &'static str {
        match self {
            Self::Checkout => git_menu_checkout_text(),
            Self::CreateBranchHere => git_menu_create_branch_text(),
            Self::CreateTagHere => git_menu_create_tag_text(),
            Self::RenameBranch => git_menu_rename_branch_text(),
            Self::DeleteBranch => git_menu_delete_branch_text(),
            Self::DeleteTag => git_menu_delete_tag_text(),
            Self::CheckoutTracking => git_menu_checkout_tracking_text(),
            Self::Stage => git_menu_stage_text(),
            Self::Unstage => git_menu_unstage_text(),
            Self::Discard => git_menu_discard_text(),
            Self::OpenDiff => git_menu_open_diff_text(),
            Self::RevealInExplorer => git_menu_reveal_text(),
            Self::CopyPath => git_menu_copy_path_text(),
            Self::CopyHash => git_menu_copy_hash_text(),
            Self::CopySubject => git_menu_copy_subject_text(),
            Self::CopyName => git_menu_copy_name_text(),
            Self::CompareWithSelected => git_menu_compare_selected_text(),
            Self::CompareWithWorkingTree => git_menu_compare_working_text(),
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

    /// The mark in the row's 14-pixel column — the row's verb, asked of
    /// [`ActionIcon`].
    ///
    /// **`Rename…` had none, and it was an oversight wearing a choice's
    /// clothes.** The note here read: *"the house's mark set is cut from
    /// geometry and has no pencil in it; the nearest thing to one would be a
    /// mark that means something else, and a wrong picture is read faster than a
    /// missing one"*. It stopped being true on 2026-08-19, when `#i-pencil` was
    /// cut — and it was already untrue *in this file*, where `Rename` on a file
    /// row had been drawing that pencil seven hundred lines further up. One
    /// verb table is exactly what stops one dispatcher going stale against
    /// another; this arm is the first thing it found.
    #[must_use]
    fn mark(self) -> Option<ChromeMark> {
        Some(match self {
            Self::Checkout | Self::CheckoutTracking => ActionIcon::CheckoutBranch.mark(),
            Self::CreateBranchHere => ActionIcon::CreateBranch.mark(),
            Self::Stage => ActionIcon::StageChange.mark(),
            Self::CreateTagHere => ActionIcon::CreateTag.mark(),
            Self::RenameBranch => ActionIcon::RenameBranch.mark(),
            Self::DeleteBranch => ActionIcon::DeleteBranch.mark(),
            Self::DeleteTag => ActionIcon::DeleteTag.mark(),
            Self::Discard => ActionIcon::DiscardChanges.mark(),
            Self::Unstage => ActionIcon::UnstageChange.mark(),
            Self::OpenDiff => ActionIcon::OpenDiff.mark(),
            Self::RevealInExplorer => ActionIcon::RevealInFolder.mark(),
            Self::CopyPath => ActionIcon::CopyPath.mark(),
            Self::CopyHash => ActionIcon::CopyHash.mark(),
            Self::CopySubject => ActionIcon::CopySubject.mark(),
            Self::CopyName => ActionIcon::CopyName.mark(),
            // A comparison is two things stood beside each other, which is what
            // this mark draws.
            Self::CompareWithSelected | Self::CompareWithWorkingTree => {
                ActionIcon::CompareVersions.mark()
            }
        })
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
            Self::CreateBranch => crate::i18n::git_prompt_new_branch(subject),
            Self::CreateTag => crate::i18n::git_prompt_new_tag(subject),
            Self::RenameBranch => crate::i18n::git_prompt_rename(subject),
        }
    }

    /// What the empty field says it wants.
    #[must_use]
    pub fn placeholder(self) -> &'static str {
        match self {
            Self::CreateBranch => crate::i18n::Text::GitPromptBranchName,
            Self::CreateTag => crate::i18n::Text::GitPromptTagName,
            Self::RenameBranch => crate::i18n::Text::GitPromptNewName,
        }
        .text()
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
/// Clamped rather than cyclic, which is [`file_menu_step`]'s ruling and the
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

impl GitPromptRects {
    /// The run the name is laid out in, inside the field's own padding.
    #[must_use]
    pub fn text_run(&self, scale: f32) -> [f32; 4] {
        let px = |logical: f32| logical * scale;
        let left = self.field[0] + px(GIT_PROMPT_FIELD_PADDING_X_LOGICAL_PX);
        let right = self.field[2] - px(GIT_PROMPT_FIELD_PADDING_X_LOGICAL_PX);
        [left, self.field[1], right.max(left), self.field[3]]
    }

    /// **The caret's line box** — its x from [`Self::caret_x`], its top and bottom
    /// the field's own.
    ///
    /// One derivation with two readers, exactly as `search::Capsule::caret_line`
    /// is: [`git_menu_build`] insets this to draw the bar, and the window hands
    /// the same rectangle to the IME so a branch name composed in Chinese gets
    /// its candidate list under the prompt rather than wherever the last caret
    /// this window published happened to stand.
    #[must_use]
    pub fn caret_line(&self, scale: f32) -> [f32; 4] {
        let text = self.text_run(scale);
        let caret = (GIT_PROMPT_CARET_LOGICAL_PX * scale).round().max(1.0);
        let x = (text[0] + self.caret_x).min(text[2] - caret);
        [x, self.field[1], x + caret, self.field[3]]
    }
}

/// Every rectangle a git context menu draws and hit-tests.
#[derive(Clone, Debug, PartialEq)]
pub struct GitMenuLayout {
    scale: f32,
    frame: [f32; 4],
    items: Vec<GitMenuItem>,
    separator: Option<[f32; 4]>,
    prompt: Option<GitPromptRects>,
    /// Which way it grew — [`ProfileMenuLayout::travel`]'s field.
    travel: Travel,
}

impl GitMenuLayout {
    /// Which way this menu grew out of the press that raised it.
    #[must_use]
    pub fn travel(&self) -> Travel {
        self.travel
    }

    /// The prompt's rectangles, when this menu has become one.
    ///
    /// Read by the window so the IME can be told where the name is being typed —
    /// the only thing outside this module that needs a box from inside the menu,
    /// and it needs the one the painter used.
    #[must_use]
    pub fn prompt_rects(&self) -> Option<GitPromptRects> {
        self.prompt
    }
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
            travel: Travel::away_from(pressed_at(point), frame),
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
        travel: Travel::away_from(pressed_at(point), frame),
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
            mono: false,
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
        let typed = !prompt.text.is_empty();
        let text_rect = rects.text_run(layout.scale);
        labels.push(ChromeLabel {
            mono: false,
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
        let inset = px(GIT_PROMPT_CARET_INSET_LOGICAL_PX).round();
        let line = rects.caret_line(layout.scale);
        quads.push(OverlayQuad {
            rect: [line[0], line[1] + inset, line[2], line[3] - inset],
            color: palette.accent,
            alpha: 1.0,
        });
        if let (Some(hint), Some(fault)) = (rects.hint, prompt.fault) {
            labels.push(ChromeLabel {
                mono: false,
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
                // No row of this menu is a row of the shortcut table (系统性发现 ②).
                accel: None,
                dirty: false,
                hovered: look.hover == Some(item.row) && item.available,
                available: item.available,
                pin: None,
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

#[must_use]
pub fn term_menu_copy_text() -> &'static str {
    crate::i18n::Text::TermMenuCopy.text()
}
#[must_use]
pub fn term_menu_paste_text() -> &'static str {
    crate::i18n::Text::TermMenuPaste.text()
}
#[must_use]
pub fn term_menu_select_all_text() -> &'static str {
    crate::i18n::Text::TermMenuSelectAll.text()
}
#[must_use]
pub fn term_menu_find_text() -> &'static str {
    crate::i18n::Text::TermMenuFind.text()
}
#[must_use]
pub fn term_menu_clear_screen_text() -> &'static str {
    crate::i18n::Text::TermMenuClearScreen.text()
}
#[must_use]
pub fn term_menu_clear_scrollback_text() -> &'static str {
    crate::i18n::Text::TermMenuClearScrollback.text()
}
#[must_use]
pub fn term_menu_restart_text() -> &'static str {
    crate::i18n::Text::TermMenuShellAgain.text()
}

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

// ── §7.1.6i's floor: the lone pane's pane-verb segment ─────────────────────
//
// **`docs/DESIGN.md` §7.1.6i, 「两案共同、且不随选型摇摆的一件」.** The corner
// ghost makes the door *visible*; this makes it not need finding. Every other
// route into a pane's verbs has to be discovered — a chord somebody has to teach
// you, a mark you have to notice — and a right click inside a terminal is the
// one gesture this operating system has already taught every hand.
//
// **It is drawn only on a lone pane.** A pane with a sibling wears its head, the
// same verbs are eighteen pixels away in it, and a menu that repeats what is
// already visible beside it is how two lists come to disagree.
//
// **Two of the `⌄` menu's rows are ruled out, and by the ruling rather than by
// this file**: `Close pane` closes the whole *tab* on a lone pane, which is a
// row that quietly means something bigger than it says, and the tab's own `×`
// and `Ctrl+W` say it out loud; `Move pane to new tab` has nothing to move and
// nowhere to move it, since a lone pane already *is* the whole tab. The mock-up
// still prints that row and flags it in its own comment as "the row this file
// would cut" — §7.1.6i cut it, and `loneVerbSegmentHtml` has been brought into
// line with the ruling.
//
// **And the sibling §7.1.6i named has arrived** (multiwindow slice F1c): the
// paragraph above ended "its honest sibling in this layout is `Move pane to new
// window`, which is multi-window F's to deliver", and F delivered it. It is the
// one verb in this menu that a lone pane can actually spend — this window is not
// all there is — so the segment is four rows rather than three, and the debt
// §7.1.6i wrote down is paid where it was written.

/// The pane verbs a lone pane's right click carries, in the `⌄` menu's own
/// order.
///
/// [`PaneMenuRow`] values and not a parallel list of words: the rows a hand
/// finds by right-clicking and the rows it finds in the head are **the same
/// rows**, so the day one of them is renamed there is nowhere for the other to
/// disagree from. The verbs behind them are one implementation too — see
/// `Runtime::run_pane_verb`.
pub const TERM_MENU_LONE_PANE_ROWS: [PaneMenuRow; 4] = [
    PaneMenuRow::SplitWith,
    PaneMenuRow::NewInFolder,
    PaneMenuRow::Duplicate,
    PaneMenuRow::MoveToNewWindow,
];

/// One entry of the terminal's context menu.
///
/// Two arms rather than three more variants of [`TermMenuRow`], because the
/// second half of this menu is not a second list of terminal verbs — it is the
/// pane menu's own rows, standing in a second doorway. A `TermMenuRow::SplitWith`
/// would be exactly the duplicate §7.1.6e forbids: one verb, two doors, two
/// spellings, and a rename that lands on one of them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TermMenuEntry {
    /// A verb about the shell in this pane — the seven of [`TERM_MENU_ROWS`].
    Term(TermMenuRow),
    /// A verb about the pane itself, borrowed whole from the `⌄` menu.
    Pane(PaneMenuRow),
}

impl TermMenuEntry {
    /// What the entry says — each arm's own row, asked.
    #[must_use]
    pub fn text(self) -> &'static str {
        match self {
            Self::Term(row) => row.text(),
            Self::Pane(row) => row.text(),
        }
    }

    /// The mark in the entry's 14-pixel column.
    fn mark(self) -> Option<ChromeMark> {
        match self {
            Self::Term(row) => row.mark(),
            Self::Pane(row) => row.mark(),
        }
    }

    /// Whether this entry hangs a child list off itself.
    ///
    /// `Split with ▸` and nothing else, which is the pane menu's own answer read
    /// through the same function.
    #[must_use]
    pub fn has_submenu(self) -> bool {
        matches!(self, Self::Pane(row) if row.has_submenu())
    }

    /// **The shortcut row that is this same verb**, or `None` (系统性发现 ②).
    ///
    /// Each arm's own row, asked — the same shape [`Self::text`] and
    /// [`Self::mark`] have, and for the same reason: the pane half of this menu
    /// *is* the pane menu, so its chords are the pane menu's chords and cannot
    /// be a second opinion about them.
    fn accelerator(self) -> Option<crate::shortcuts::Action> {
        match self {
            Self::Term(row) => row.accelerator(),
            Self::Pane(row) => row.accelerator(),
        }
    }
}

impl TermMenuRow {
    /// **The shortcut row that is this same verb**, or `None` (系统性发现 ②).
    ///
    /// **One of the seven**, and the six silences are each a fact rather than an
    /// oversight. `Copy` and `Paste` are the interesting pair: the reader's
    /// hands know `Ctrl+Shift+C` and `Ctrl+Shift+V`, and this column stays empty
    /// beside those two rows because those chords are **not rows of the shortcut
    /// table** — they are decided in `input::should_copy_selection` and
    /// `input::is_paste_shortcut`, above the table, so that a terminal never
    /// loses its clipboard to a rebind. A menu that printed them would be
    /// reporting a table row that does not exist and that the Shortcuts page
    /// cannot edit; the honest answer is the empty slot, and the debt is the
    /// clipboard chords not being editable, not this column not printing them.
    ///
    /// `Select all`, the two `Clear`s and `Restart shell` have no chord at all —
    /// the audit says so of `Select all` in as many words (`Ctrl+A` in a
    /// terminal is the child's `0x01` and must stay so).
    fn accelerator(self) -> Option<crate::shortcuts::Action> {
        match self {
            Self::Find => Some(crate::shortcuts::Action::OpenSearch),
            Self::Copy
            | Self::Paste
            | Self::SelectAll
            | Self::ClearScreen
            | Self::ClearScrollback
            | Self::RestartShell => None,
        }
    }
}

/// The menu, top to bottom, for a pane that has a sibling or has not.
///
/// A function rather than two `const` lists, because the difference between the
/// two menus is one fact about the tree and a list that repeated the seven would
/// be a second place for the seven to change.
#[must_use]
pub fn term_menu_entries(lone: bool) -> Vec<TermMenuEntry> {
    let mut entries: Vec<TermMenuEntry> = TERM_MENU_ROWS
        .into_iter()
        .map(TermMenuEntry::Term)
        .collect();
    if lone {
        entries.extend(
            TERM_MENU_LONE_PANE_ROWS
                .into_iter()
                .map(TermMenuEntry::Pane),
        );
    }
    entries
}

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

/// Whether an entry can do what it says, on this pane.
///
/// The pane verbs are never greyed, and that is `pane_menu_build`'s own
/// argument carried across with them: a split the solver has no room for is
/// refused *after* the press exactly as the chord's is, and the chooser
/// `New terminal in folder…` opens is Windows', which this window does not get
/// to promise about in advance. Nothing in the segment is a promise this build
/// knows it cannot keep.
#[must_use]
pub fn term_menu_entry_available(entry: TermMenuEntry, subject: TermMenuSubject) -> bool {
    match entry {
        TermMenuEntry::Term(row) => term_menu_row_available(row, subject),
        TermMenuEntry::Pane(_) => true,
    }
}

/// The row a keyboard step lands on, **skipping the ones that answer nothing** —
/// [`git_menu_step`]'s rule and [`file_menu_step`]'s clamp, on this list.
///
/// The rule between the two halves is not a stop: the walk crosses it, because
/// a separator is punctuation and a keyboard that halted at one would make the
/// segment reachable only by pointer — which is the discoverability hole this
/// whole floor exists to fill, dug again one surface down.
#[must_use]
pub fn term_menu_step(
    current: Option<TermMenuEntry>,
    subject: TermMenuSubject,
    forwards: bool,
    lone: bool,
) -> Option<TermMenuEntry> {
    let walkable: Vec<TermMenuEntry> = term_menu_entries(lone)
        .into_iter()
        .filter(|entry| term_menu_entry_available(*entry, subject))
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
            Self::Copy => term_menu_copy_text(),
            Self::Paste => term_menu_paste_text(),
            Self::SelectAll => term_menu_select_all_text(),
            Self::Find => term_menu_find_text(),
            Self::ClearScreen => term_menu_clear_screen_text(),
            Self::ClearScrollback => term_menu_clear_scrollback_text(),
            Self::RestartShell => term_menu_restart_text(),
        }
    }

    /// The mark in the row's 14-pixel column.
    ///
    /// **Every row has one since P1.** `Find…` was the one gap, on the
    /// argument that the mock-up's sheet has no magnifier and that a wrong
    /// picture is read faster than a missing one — which was true of the
    /// alternatives on offer and stopped being true the day the house struck
    /// its own (`ChromeMark::Search`). The empty cell was the only one in the
    /// column, with a mark above it and a mark below it.
    #[must_use]
    fn mark(self) -> Option<ChromeMark> {
        Some(match self {
            Self::Copy => ActionIcon::CopySelection.mark(),
            Self::Paste => ActionIcon::PasteClipboard.mark(),
            Self::SelectAll => ActionIcon::SelectAll.mark(),
            Self::Find => ActionIcon::FindInTerminal.mark(),
            Self::ClearScreen => ActionIcon::ClearScreen.mark(),
            Self::ClearScrollback => ActionIcon::ClearScrollback.mark(),
            Self::RestartShell => ActionIcon::RestartShell.mark(),
        })
    }
}

/// What is lit on the terminal menu — the pointer's hover and the keyboard's
/// cursor, which are one thing in a menu ([`PaneMenuHover`]'s own sentence).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TermMenuHover {
    /// A row of the menu proper, on either side of the rule.
    Row(TermMenuEntry),
    /// A row of the open `Split with` child, by its index into [`PROFILES`].
    Submenu(usize),
}

/// What a point in one of the terminal menu's surfaces is over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TermMenuHit {
    /// A row that can answer. A greyed one is **not** reported: the pointer
    /// falls through it onto the menu's own body.
    Row(TermMenuEntry),
    /// A row of the open child, by its index into [`PROFILES`].
    Submenu(usize),
    /// Inside one of the surfaces but on no row: the padding, either rule, a
    /// greyed row.
    Surface,
}

/// Everything the terminal menu needs to lay itself out and draw.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TermMenuLook {
    pub subject: TermMenuSubject,
    pub hover: Option<TermMenuHover>,
    /// Whether the pane this menu was raised on is the only one in its tab —
    /// the one fact §7.1.6i's segment turns on.
    pub lone: bool,
    /// Whether the `Split with` child is up. Only ever true with [`Self::lone`],
    /// because the row it hangs from is only ever drawn then.
    pub submenu_open: bool,
}

/// Every rectangle the terminal's context menu draws and hit-tests.
#[derive(Clone, Debug, PartialEq)]
pub struct TermMenuLayout {
    scale: f32,
    frame: [f32; 4],
    items: Vec<TermMenuItem>,
    /// One accelerator per entry of [`Self::items`], in that order (系统性发现 ②).
    ///
    /// Beside the items rather than inside them, because [`TermMenuItem`] is
    /// `Copy` and a `String` is not — and because the reason for carrying them
    /// at all is [`PaneMenuLayout::accels`]': the frame was measured with this
    /// column reserved in it, so the painter must draw the very strings that
    /// were measured.
    accels: Vec<Option<(String, f32)>>,
    separator: [f32; 4],
    /// The second rule — the one §7.1.6i's segment brings with it — or `None` on
    /// a pane with a sibling, which has no segment to divide off.
    lone_separator: Option<[f32; 4]>,
    /// The `Split with` child's frame and rows, when it is open.
    ///
    /// [`PaneSubmenuLayout`] itself, laid out by [`pane_submenu_layout`]: the
    /// child hanging off this menu's heading is the *same* child that hangs off
    /// the head's, down to the seam it meets its parent on and the profile list
    /// it draws, so it is the same type placed by the same function.
    submenu: Option<PaneSubmenuLayout>,
    /// Which way it grew — [`ProfileMenuLayout::travel`]'s field.
    travel: Travel,
}

impl TermMenuLayout {
    /// Which way this menu grew out of the press that raised it.
    #[must_use]
    pub fn travel(&self) -> Travel {
        self.travel
    }

    /// Which way the child hanging off its heading grew, when one is up.
    #[must_use]
    pub fn submenu_travel(&self) -> Option<Travel> {
        self.submenu.as_ref().map(PaneSubmenuLayout::travel)
    }
}

impl TermMenuLayout {
    /// The child's border box, when one is up.
    #[must_use]
    pub fn submenu_frame(&self) -> Option<[f32; 4]> {
        self.submenu.as_ref().map(|submenu| submenu.frame)
    }

    /// The child's row boxes, when one is up.
    #[allow(dead_code)]
    #[must_use]
    pub fn submenu_rows(&self) -> Option<&[[f32; 4]]> {
        self.submenu
            .as_ref()
            .map(|submenu| submenu.items.as_slice())
    }

    /// **Which profile the child's `at`th row is** —
    /// [`PaneMenuLayout::submenu_row`] on this menu's own child, and for that
    /// method's reason: a hit counts rows on the glass, and only the table
    /// lookups want the identity.
    #[must_use]
    pub fn submenu_row(&self, at: usize) -> Option<usize> {
        self.submenu
            .as_ref()
            .and_then(|submenu| submenu.rows.get(at).copied())
    }

    /// **Whether this point is on the child at all** — its rows, its padding,
    /// its border. [`PaneMenuLayout::on_submenu`]'s reason verbatim: the hit
    /// answers `Surface` for both menus' padding, so the safety triangle cannot
    /// tell "on the child" from "not on the child" out of the hit alone.
    #[must_use]
    pub fn on_submenu(&self, x: f32, y: f32) -> bool {
        self.submenu
            .as_ref()
            .is_some_and(|submenu| contains(submenu.frame, x, y))
    }
}

/// One laid-out row of the terminal menu.
///
/// Its own type rather than [`GitMenuItem`] with a different row in it, because
/// the two lists hold different enums and a shared item would have to be generic
/// over them — a type parameter bought for three fields that are copied out at
/// the one call site each.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TermMenuItem {
    pub entry: TermMenuEntry,
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
///
/// **The second rule and the child are `look`'s to ask for**, exactly as the
/// pane menu's child is `submenu_open`'s: where they go is a fact about *this*
/// frame — the segment falls under the seven, the child hangs off one of its
/// rows and flips when this frame is near the window's edge — so a second entry
/// point would be a second opinion about where the parent is.
#[must_use]
pub fn term_menu_layout(
    point: [f32; 2],
    surface: (f32, f32),
    scale: f32,
    look: &TermMenuLook,
    shortcuts: &crate::shortcuts::Shortcuts,
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

    let entries = term_menu_entries(look.lone);
    // The `▸` claims the same slot the profile picker's `default` hint claims,
    // and it is reserved on **every** row of a menu that has a submenu at all
    // rather than on the one that wears it — `pane_menu_layout`'s own rule, for
    // its reason: a menu whose width depended on which rows had children would
    // change width the day a second row grew one. A menu with no segment has no
    // child anywhere in it and reserves nothing, which is why the seven rows are
    // the width they always were on a pane with a sibling.
    let indicator = if look.lone {
        px(SUBMENU_INDICATOR_LOGICAL_PX) + px(ITEM_GAP_LOGICAL_PX)
    } else {
        0.0
    };
    // The chord column, on `indicator`'s own rule one slot further out: measured
    // over every entry, reserved on every row (系统性发现 ②).
    let accels: Vec<Option<(String, f32)>> = entries
        .iter()
        .map(|entry| {
            accelerator_of(
                entry.accelerator(),
                entry.has_submenu(),
                shortcuts,
                scale,
                measure,
            )
        })
        .collect();
    let accel_claim = accelerator_claim(&accels, scale);
    let row_width = |entry: TermMenuEntry, measure: &mut dyn FnMut(&str, f32) -> f32| {
        px(ITEM_ICON_COLUMN_LOGICAL_PX)
            + px(ITEM_GAP_LOGICAL_PX)
            + measure(entry.text(), px(ITEM_FONT_LOGICAL_PX))
            + indicator
            + accel_claim
    };
    let content = entries.iter().fold(
        px(TERM_MENU_MIN_WIDTH_LOGICAL_PX) - chrome,
        |wide, entry| wide.max(row_width(*entry, measure)),
    );
    #[allow(clippy::cast_precision_loss)]
    let rows_height = entries.len() as f32 * item_height;
    // One block per rule that is actually drawn: the segment brings its own, and
    // a height that counted a rule the walk below does not lay out would be a
    // menu with a stripe of window at its foot.
    let separator_blocks: f32 = if look.lone { 2.0 } else { 1.0 };
    let height = 2.0f32
        .mul_add(
            border + padding,
            separator_blocks.mul_add(separator_block, rows_height),
        )
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

    let mut items = Vec::with_capacity(entries.len());
    let mut separator = [0.0_f32; 4];
    let mut lone_separator = None;
    let mut heading = None;
    for (at, entry) in entries.iter().enumerate() {
        // The segment's rule falls where the *subject* changes — the last thing
        // this menu says about the shell, then the first thing it says about the
        // pane — so it is placed by the entry it precedes rather than by an
        // index, which would be a second statement of how long the first list is.
        if matches!(entry, TermMenuEntry::Pane(_)) && lone_separator.is_none() {
            lone_separator = Some([
                content_left,
                cursor + separator_margin,
                content_right,
                cursor + separator_margin + separator_thickness,
            ]);
            cursor += separator_block;
        }
        let rect = [content_left, cursor, content_right, cursor + item_height];
        if entry.has_submenu() {
            heading = Some(rect);
        }
        items.push(TermMenuItem {
            entry: *entry,
            rect,
            available: term_menu_entry_available(*entry, look.subject),
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
    // Placed by `pane_submenu_layout` and not by a second copy of it: the child
    // that hangs off this menu's heading is the same child that hangs off the
    // head's, so the seam it meets its parent on, the side it flips to and the
    // rows it holds are one derivation. Only the parent it is measured against
    // differs, which is the argument that function already takes.
    let submenu = heading.filter(|_| look.submenu_open).map(|heading| {
        // **The terminal menu carries one submenu, and it is the profile list**
        // (B9). Its lone-pane segment is a hand-picked four rows and
        // `Move to window` is not among them, so the kind is stated rather than
        // derived — and the empty window list beside it is the honest way of
        // saying this menu has no such row to draw one for.
        pane_submenu_layout(
            frame,
            heading,
            PaneMenuRow::SplitWith,
            &[],
            surface,
            scale,
            border,
            padding,
            item_height,
            measure,
        )
    });
    debug_assert_eq!(accels.len(), items.len());
    TermMenuLayout {
        scale,
        frame,
        items,
        accels,
        separator,
        lone_separator,
        submenu,
        travel: Travel::away_from(pressed_at(point), frame),
    }
}

/// What a point is over, with the same answers every other menu gives: a row,
/// the menu's own padding, or nothing at all.
///
/// A row that cannot do what it says is **not** offered — the pointer falls
/// through it onto the menu's body, so it neither lights nor answers a press.
///
/// **The child is asked first**, because it is drawn over the parent and
/// overlaps it: a point in the strip where the two frames cross belongs to the
/// child, exactly as the topmost window owns a point everywhere else in this
/// program. [`pane_menu_hit`]'s sentence, on this menu's two surfaces.
#[must_use]
pub fn term_menu_hit(layout: &TermMenuLayout, x: f64, y: f64) -> Option<TermMenuHit> {
    let (x, y) = (x as f32, y as f32);
    if let Some(submenu) = layout.submenu.as_ref()
        && contains(submenu.frame, x, y)
    {
        for (row, rect) in submenu.items.iter().enumerate() {
            if contains(*rect, x, y) {
                return Some(TermMenuHit::Submenu(row));
            }
        }
        return Some(TermMenuHit::Surface);
    }
    for item in &layout.items {
        if item.available && contains(item.rect, x, y) {
            return Some(TermMenuHit::Row(item.entry));
        }
    }
    contains(layout.frame, x, y).then_some(TermMenuHit::Surface)
}

/// The menu, and its `Split with` child when one is up, as overlay layers.
///
/// `current_profile` and `programs` are the child's, and they are taken whether
/// or not one is open for [`pane_menu_build`]'s reason: which profile this pane
/// is running is a fact about the pane the menu was raised on, and a signature
/// that only asked for it sometimes would be a caller deciding when the answer
/// matters.
#[must_use]
pub fn term_menu_build(
    layout: &TermMenuLayout,
    look: &TermMenuLook,
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
    for (item, accel) in layout.items.iter().zip(layout.accels.iter()) {
        let hovered = look.hover == Some(TermMenuHover::Row(item.entry)) && item.available;
        push_row(
            &Row {
                rect: item.rect,
                mark: item.entry.mark(),
                name: item.entry.text(),
                hint: None,
                // The very string the frame was measured around — see
                // [`TermMenuLayout::accels`].
                accel: accel.clone(),
                dirty: false,
                hovered,
                available: item.available,
                pin: None,
            },
            scale,
            palette,
            &mut quads,
            &mut labels,
            &mut sprites,
        );
        if item.entry.has_submenu() {
            // The `▸`, drawn by the same three lines the head's heading draws it
            // with: `#i-tri` at rest, in the row's trailing padding, lit with the
            // row. A second angle or a written `▸` would be the fifth
            // close-enough triangle `ChromeMark::TreeDisclosure` argues against.
            //
            // **Asked of the registry since P2**, so that "the triangle lives in
            // the tree and on a submenu row and nowhere else" is a row of one
            // table rather than a construction in two files.
            let size = px(SUBMENU_INDICATOR_LOGICAL_PX).round().max(1.0);
            let right = item.rect[2] - px(ITEM_PADDING_X_LOGICAL_PX);
            let top = ((item.rect[1] + item.rect[3] - size) / 2.0).round();
            sprites.push(ChromeSprite::new(
                ActionIcon::OpenSubmenu.mark(),
                [right - size, top, right, top + size],
                if hovered {
                    palette.menu_item_text_selected
                } else {
                    palette.menu_item_hint_text
                },
            ));
        }
    }
    for rule in std::iter::once(layout.separator).chain(layout.lone_separator) {
        quads.push(OverlayQuad {
            rect: rule,
            color: palette.menu_border,
            alpha: separator_alpha(palette.menu_border),
        });
    }
    let mut layers = vec![OverlayLayer {
        quads,
        labels,
        sprites,
        ..Default::default()
    }];
    if let Some(submenu) = layout.submenu.as_ref() {
        // The head's own child, drawn by the head's own function. The hover is
        // translated at the boundary rather than shared as a type: what is lit on
        // *this* menu is this menu's fact, and the child only ever needs the one
        // arm of it that names one of its rows.
        let hover = match look.hover {
            Some(TermMenuHover::Submenu(index)) => Some(PaneMenuHover::Submenu(index)),
            _ => None,
        };
        layers.extend(push_submenu(
            submenu,
            scale,
            hover,
            current_profile,
            programs,
            &[],
            measure,
        ));
    }
    layers
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
pub const CHEVRON_HOVER_OPEN_DELAY: Duration = Duration::from_millis(250);

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
            .is_some_and(|since| now.duration_since(since) >= CHEVRON_HOVER_OPEN_DELAY)
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
            (Some(since), _) => Some(since + CHEVRON_HOVER_OPEN_DELAY),
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
/// Written upper-case at the source for [`recent_section_label()`]'s reason and
/// not for a different one: this pipeline has no `text-transform`, a chrome
/// label draws the string it is given, and a lower-case constant that nothing
/// upper-cases would be a lie about what appears on screen.
#[must_use]
pub fn picker_caption_text() -> &'static str {
    crate::i18n::Text::PaneMenuSplitCaption.text()
}

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
/// Closed rather than a `Vec`, on the argument [`FileMenuRow`] used to be able
/// to make and no longer can: a menu whose length cannot vary is also a menu
/// whose keyboard walk cannot go looking for a row that is not there. That menu
/// now varies on a subject and on a machine fact, and it keeps the argument's
/// substance by taking its list from [`file_menu`] everywhere; this one has
/// nothing to vary on, so it keeps the simpler shape as well.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneMenuRow {
    /// The Snap-Layouts picker — a drawing, not a row of text, and the only
    /// entry here that carries its own four answers.
    Picker,
    /// **This pane alone on the stage, and back again** (§7.1.6l).
    ///
    /// The one row here with two faces: over a tiled pane it offers the zoom,
    /// over the pane that *is* the stage it offers the way back. See
    /// [`Self::text_when`] and [`Self::mark_when`], which turn together.
    ///
    /// It stands directly under the diagram because those two entries are the
    /// pair that answers **how much of the stage is this pane's** — the picker
    /// puts another pane beside it, this row puts every other pane away — and
    /// the four rows below make, move or end a pane.
    ZoomPane,
    /// The submenu heading: split against a profile you name.
    SplitWith,
    /// Split, with the new shell rooted in a folder the system chooser names.
    NewInFolder,
    /// Split, with the new shell on this pane's own profile *and* this pane's
    /// own directory.
    Duplicate,
    /// The pane leaves this tab and becomes a tab of its own.
    MoveToNewTab,
    /// **The pane leaves this window and becomes a window of its own**
    /// (multiwindow slice F1c; the plan's own row, `plan.md` F1c).
    ///
    /// The row above it and this one are one journey of two lengths — a pane out
    /// of its tab, and a pane out of its window — and the second is composed of
    /// the first: the pane is promoted to a tab exactly as the row above
    /// promotes it, and the tab is then moved by the application's transfer.
    ///
    /// **It is also what makes a lone pane's menu worth opening.** §7.1.6i left
    /// `Move pane to new tab` as a no-op on a pane with no siblings — a tab is
    /// already all that pane is — and said the debt would be caught by the row
    /// that came next. This is that row: a lone pane has nowhere to go inside
    /// this window and a window of its own to go to.
    MoveToNewWindow,
    /// **Move this pane into a window that is already open** (B9, user ruling
    /// 2026-08-25) — the third and last length of one journey.
    ///
    /// It is a submenu and the two rows above it are not, and the difference is
    /// not a matter of degree: a new tab and a new window are places this verb
    /// *makes*, so there is nothing to choose between; an open window is a place
    /// that already exists, and which one is the whole of what the row is
    /// asking.
    MoveToWindow,
    /// The same verb the `×` in the head has.
    ClosePane,
}

impl PaneMenuRow {
    /// Every entry, top to bottom — the order they are laid out in and the order
    /// a keyboard walks them.
    ///
    /// **Every verb here is a verb about this pane** (user ruling 2026-08-19).
    /// Focus mode had a row at the head of this list for one day, and it was
    /// withdrawn on the argument that it stood in the wrong menu: the mode is a
    /// posture of the *window*, and a list whose every other line splits, moves
    /// or closes the pane under it is not where a window verb belongs. Its two
    /// doors are now `Ctrl+Shift+Z` and the `Appearance` row, and the
    /// command palette will carry it when there is one — which is the
    /// systematic door for a low-frequency verb, rather than a temporary one
    /// hung off whatever surface happened to be near.
    /// **Zoom joined it on 2026-08-24** (§7.1.6l), and it passes the rule above
    /// rather than being an exception to it: "this pane alone on the stage" is a
    /// verb about *this pane*, which is exactly what focus mode's withdrawn row
    /// was not. The mode is a posture of the window and rearranges the tab
    /// strip; a zoom is one pane's own share of one tab's stage, and every other
    /// line here is about that pane's share of that stage too.
    pub const ALL: [Self; 9] = [
        Self::Picker,
        Self::ZoomPane,
        Self::SplitWith,
        Self::NewInFolder,
        Self::Duplicate,
        Self::MoveToNewTab,
        Self::MoveToNewWindow,
        Self::MoveToWindow,
        Self::ClosePane,
    ];

    // **`TEXT_ROWS` is gone** (user ruling 2026-08-25). It was `ALL` without the
    // picker, and it was the *second* list this menu was assembled from — the
    // width and the height were measured over it while the boxes were laid out
    // over `ALL`, which was safe only for as long as the two could not disagree.
    // A menu that shows one row fewer is exactly the day they can, so there is
    // one list now ([`Self::rows`]) and one place it is carried
    // ([`PaneMenuLayout::rows`]). A caller that wants the text rows filters the
    // drawing out of that, which is what the layout, the painter and the walk
    // each do in one line.

    /// **The rows one menu actually shows** — [`Self::ALL`] less the one row
    /// that can have nowhere to go (user ruling 2026-08-25).
    ///
    /// `Move to window ▸` is the only entry here whose *existence* turns on
    /// anything outside the pane, and it is the one entry that can be a promise
    /// this build knows it cannot keep: it hangs a list of the other windows off
    /// itself, and on a session with one window that list is empty. It stood
    /// there greyed with a `▸` on it for a day, and a disclosure arrow over
    /// nothing is worse than a missing row — the arrow says "there is more this
    /// way" and there is not.
    ///
    /// **Nothing is lost by its going.** `Move pane to new window`, directly
    /// above, is the verb for exactly the case where there is no second window,
    /// and it is the row this one was added *beside* rather than instead of.
    ///
    /// A `Vec` and not a second `const`, for [`term_menu_entries`]'s reason: two
    /// lists that share eight entries are two places for those eight to change.
    #[must_use]
    pub fn rows(other_windows: bool) -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|row| other_windows || *row != Self::MoveToWindow)
            .collect()
    }

    /// The glyph in this row's icon column — the row's verb, asked of
    /// [`ActionIcon`].
    fn mark(self) -> Option<ChromeMark> {
        Some(match self {
            // The picker is the drawing; it has no column and no glyph.
            Self::Picker => return None,
            // The face over a tiled pane. `mark_when` is what turns it, and this
            // arm is the state a row with nothing to turn it is asked about —
            // see `text` below, which says the same thing at more length.
            Self::ZoomPane => ActionIcon::ZoomPane.mark(),
            // The `⊞` the pane head just gave up, in the one place it still
            // means what it always meant: "another one of these, beside this".
            Self::SplitWith => ActionIcon::SplitPane.mark(),
            Self::NewInFolder => ActionIcon::NewTerminalInFolder.mark(),
            Self::Duplicate => ActionIcon::DuplicatePane.mark(),
            // `#i-float`'s own sentence is "opens outside this frame" — and a
            // pane leaving for a tab of its own is exactly that. It is the same
            // glyph the files head wears for undocking, which is the same idea
            // aimed at a different container.
            Self::MoveToNewTab => ActionIcon::MoveToNewTab.mark(),
            // **The bare arrow beside the framed one**, which is the pair's own
            // sentence read one container up: `#i-float`'s doc says the frame is
            // *this pane leaves the tree* and the bare arrow is *this content
            // leaves the window*, and these two rows differ by exactly that. Two
            // framed arrows in adjacent rows would be one drawing asked to mean
            // both — the argument `ChromeMark::External` was drawn for.
            Self::MoveToNewWindow => ActionIcon::MoveToNewWindow.mark(),
            // **The same bare arrow as the row above it**, because it is the
            // same sentence — this content leaves the window — and what differs
            // is only where it lands, which is what the submenu is for. Giving
            // this row a glyph of its own would be the drawing claiming a
            // difference the words are already carrying.
            Self::MoveToWindow => ActionIcon::MoveToWindow.mark(),
            // **The pane's own `×` and not the tab's**, since the registry: the
            // two are one `<symbol>` under two names, and which of them a menu
            // row reached for was arbitrary — this row closes a *pane*.
            Self::ClosePane => ActionIcon::ClosePane.mark(),
        })
    }

    /// The words on this row, in the state a row that has no state is in.
    ///
    /// Six of the seven verbs read the same whatever the pane is doing, and this
    /// is the whole table for them. [`Self::ZoomPane`] is the seventh: its
    /// canonical name is the action, which is what a row is called when nobody
    /// has said which face is wanted, and [`Self::text_when`] is the one seam
    /// that asks. Keeping this function total rather than making every caller
    /// carry a `bool` is what lets `TermMenuEntry::text` go on being a
    /// two-line delegation — the lone-pane segment carries no zoom row
    /// (§7.1.6l), so it has no face to choose between.
    fn text(self) -> &'static str {
        match self {
            Self::Picker => picker_caption_text(),
            Self::ZoomPane => zoom_pane_text(),
            Self::SplitWith => split_with_text(),
            Self::NewInFolder => new_in_folder_text(),
            Self::Duplicate => duplicate_pane_text(),
            Self::MoveToNewTab => move_to_new_tab_text(),
            Self::MoveToNewWindow => move_to_new_window_text(),
            Self::MoveToWindow => move_to_window_text(),
            Self::ClosePane => close_pane_text(),
        }
    }

    /// **The words on this row, told what the pane under the menu is doing**
    /// (§7.1.6l).
    ///
    /// One row turns and the other six do not, and both halves of that are
    /// asserted rather than trusted — see
    /// `the_zoom_row_changes_its_word_and_its_mark_with_the_state_it_names`.
    ///
    /// The state is the menu's, taken when the menu was raised, exactly as
    /// §7.1.6i takes `lone` and `subject` there: a menu that re-asked every
    /// frame would change its own first row under a hand that was already
    /// moving toward it.
    #[must_use]
    pub fn text_when(self, zoomed: bool) -> &'static str {
        match self {
            Self::ZoomPane if zoomed => restore_pane_text(),
            other => other.text(),
        }
    }

    /// **The mark on this row, told the same thing** — and it turns with the
    /// word, never on its own.
    ///
    /// The `Lock` ruling (§7.1.6c-8) said it once and it holds here: change only
    /// the word and the drawing goes on lying; change only the drawing and the
    /// reader has to decide which half to believe.
    #[must_use]
    pub fn mark_when(self, zoomed: bool) -> Option<ChromeMark> {
        match self {
            Self::ZoomPane => Some(ChromeMark::PaneZoom { zoomed }),
            other => other.mark(),
        }
    }

    /// Whether this row hangs a submenu off itself.
    #[must_use]
    pub fn has_submenu(self) -> bool {
        matches!(self, Self::SplitWith | Self::MoveToWindow)
    }

    /// **The shortcut row that is this same verb**, or `None` (系统性发现 ②).
    ///
    /// Three of the nine, and that is the honest count rather than a shortfall:
    /// this menu and the chord table overlap where they overlap. `New terminal
    /// in folder…` opens a system chooser, the two `Move to` rows and
    /// `Split with ▸` are places rather than chords, and the picker is a
    /// drawing — none of them is a row of [`crate::shortcuts::BINDINGS`], so
    /// none of them prints anything here.
    ///
    /// Named exhaustively rather than swept into a `_`, on this file's standing
    /// rule for row tables: a row added tomorrow must be *decided* about, not
    /// silently given no chord.
    fn accelerator(self) -> Option<crate::shortcuts::Action> {
        use crate::shortcuts::Action;
        match self {
            Self::ZoomPane => Some(Action::ZoomPane),
            // `Duplicate pane` is `SplitSeed::Inherit` wearing a name, and
            // `DuplicatePaneSplit` is the chord for that same verb — the seed's
            // own doc comment says so in as many words.
            Self::Duplicate => Some(Action::DuplicatePaneSplit),
            Self::ClosePane => Some(Action::ClosePane),
            Self::Picker
            | Self::SplitWith
            | Self::NewInFolder
            | Self::MoveToNewTab
            | Self::MoveToNewWindow
            | Self::MoveToWindow => None,
        }
    }
}

/// The submenu heading. The `▸` is drawn rather than written: see
/// [`pane_menu_build`], which strikes the house's `⌄` turned a quarter into the
/// row's trailing edge.
#[must_use]
pub fn split_with_text() -> &'static str {
    crate::i18n::Text::PaneMenuSplitWith.text()
}
/// The zoom row's action face — what the press does to a tiled pane.
#[must_use]
pub fn zoom_pane_text() -> &'static str {
    crate::i18n::Text::PaneMenuZoom.text()
}
/// The zoom row's state face — the way back off the stage.
#[must_use]
pub fn restore_pane_text() -> &'static str {
    crate::i18n::Text::PaneMenuRestore.text()
}
/// The ellipsis is load-bearing: it is this window's promise that a row asks
/// before it acts, and this row opens a system dialog you can cancel.
#[must_use]
pub fn new_in_folder_text() -> &'static str {
    crate::i18n::Text::PaneMenuNewInFolder.text()
}
#[must_use]
pub fn duplicate_pane_text() -> &'static str {
    crate::i18n::Text::PaneMenuDuplicate.text()
}
#[must_use]
pub fn move_to_new_tab_text() -> &'static str {
    crate::i18n::Text::PaneMenuMoveToNewTab.text()
}
/// One word apart from the row above it, and the word is the container.
#[must_use]
pub fn move_to_window_text() -> &'static str {
    crate::i18n::Text::PaneMenuMoveToWindow.text()
}
#[must_use]
pub fn move_to_new_window_text() -> &'static str {
    crate::i18n::Text::PaneMenuMoveToNewWindow.text()
}
/// The `×`'s verb, spelled — a menu row has room for the word the button does
/// not, and `Close pane` is the mock-up's own `title` for that button (4672).
#[must_use]
pub fn close_pane_text() -> &'static str {
    crate::i18n::Text::ClosePane.text()
}
/// What the submenu writes beside the profile this pane is already running.
///
/// The `.default-hint` slot, on the profile picker's own precedent: the mark
/// column belongs to the profile's own glyph, so "this is the one you are on"
/// has to be said in words rather than with a tick.
pub fn current_profile_hint_text() -> &'static str {
    crate::i18n::Text::ProfileHintCurrent.text()
}

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
    ///
    /// `rows` is **the list this menu is showing** and not the enum's whole
    /// vocabulary (user ruling 2026-08-25) — [`PaneMenuLayout::rows`], which the
    /// picture was built from. A walk over `ALL` would stop the highlight on a
    /// row that is not on the glass, which is the same defect
    /// [`file_menu_step`] takes a subject to avoid one menu over.
    #[must_use]
    pub fn step(
        current: Option<Self>,
        step: MenuStep,
        submenu_rows: usize,
        rows: &[PaneMenuRow],
    ) -> Option<Self> {
        // The text rows in the order they are drawn — the walk's own list, which
        // is `rows` less the drawing at the top of it.
        let text: Vec<PaneMenuRow> = rows
            .iter()
            .copied()
            .filter(|row| *row != PaneMenuRow::Picker)
            .collect();
        // A menu with no verbs in it has nowhere for a highlight to go — which
        // cannot happen today and is answered rather than asserted, because this
        // is a walk and a walk that panicked would take the window with it.
        let (&first, &last) = text.first().zip(text.last())?;
        match current {
            // Nothing lit: the list is entered at whichever end the key names.
            None => match step {
                MenuStep::Down => Some(Self::Zone(SplitZone::Right)),
                MenuStep::Up => Some(Self::Row(last)),
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
                // leads anywhere — the picker is the first entry again now that
                // the focus row above it has been withdrawn (user ruling
                // 2026-08-19), so `↑` from `Up` is the top of the list and
                // clamps, exactly as every other walk in this window clamps at
                // its ends.
                match step {
                    // The first text row, asked for by position rather than by
                    // name: §7.1.6l put a verb directly under the diagram, and a
                    // walk that named `SplitWith` here would have stepped over
                    // it.
                    MenuStep::Down => Some(Self::Row(first)),
                    _ => None,
                }
            }
            Some(Self::Row(row)) => {
                // A highlight on a row this menu is not showing: the ordinary
                // case rather than a fault, because the list can be rebuilt
                // between one key and the next. Answering `None` leaves the
                // highlight where it was, which is what every other walk in this
                // window does when a step leads nowhere.
                let index = text.iter().position(|it| *it == row)?;
                match step {
                    MenuStep::Down => Some(Self::Row(text[(index + 1).min(text.len() - 1)])),
                    // The row above the first text row is the picker, entered at
                    // the zone nearest the row being left.
                    MenuStep::Up if index == 0 => Some(Self::Zone(SplitZone::Down)),
                    MenuStep::Up => Some(Self::Row(text[index - 1])),
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
    /// **The entries this menu is showing**, top to bottom — [`PaneMenuRow::rows`]
    /// for the session the menu was raised in (user ruling 2026-08-25).
    ///
    /// Carried rather than re-derived on [`Self::zoomed`]'s reasoning: the frame
    /// was measured against *this* list, so a painter or a hit test that worked
    /// the list out again would be a second opinion about a thing the layout
    /// already knows — and the two would disagree on exactly the frame where a
    /// second window opened or the last one closed.
    rows: Vec<PaneMenuRow>,
    /// One rectangle per entry of [`Self::rows`], in that order. The picker's is
    /// its whole block.
    items: Vec<[f32; 4]>,
    /// One accelerator per entry of [`Self::rows`], in that order (系统性发现 ②).
    ///
    /// Carried on [`Self::rows`]' own reasoning, one step further: the frame was
    /// measured with a column this wide reserved in it, so the painter must draw
    /// the very strings that were measured. Re-deriving them at paint time would
    /// be a second reading of the shortcut table, and the two would disagree on
    /// exactly the frame where somebody rebound a chord with this menu open.
    accels: Vec<Option<(String, f32)>>,
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
    /// The rule above `Close pane`, which separates the five verbs that *make* a
    /// pane or move one from the one that ends it.
    ///
    /// The menu's only rule again: the second one drew the sentence break under
    /// the focus-mode row, and left with it (user ruling 2026-08-19).
    separator: [f32; 4],
    /// **Which face the zoom row is wearing** (§7.1.6l) — carried on the layout
    /// rather than passed to the painter a second time, so the words the frame
    /// was measured against are the words drawn into it. Two parameters would be
    /// two opinions, and D4 is the rule that there is one.
    zoomed: bool,
    /// The submenu's frame and rows, when it is open.
    submenu: Option<PaneSubmenuLayout>,
    /// Which way it grew — [`ProfileMenuLayout::travel`]'s field.
    travel: Travel,
}

/// A submenu's own boxes, and **which of its parent's rows opened it** (B9).
///
/// The kind is carried rather than inferred, on `PaneMenuLayout::zoomed`'s own
/// reasoning: the frame was measured against the words of one particular list,
/// so a painter or a press handler that had to work out which list that was
/// would be a second opinion about a thing the layout already knows.
#[derive(Clone, Debug, PartialEq)]
pub struct PaneSubmenuLayout {
    frame: [f32; 4],
    /// One row per offered item, top to bottom.
    items: Vec<[f32; 4]>,
    /// Which entry of the **kind's own list** each of [`Self::items`] draws —
    /// the offered profile's index into `PROFILES` for `SplitWith`, and the
    /// window's place in the list it was handed for `MoveToWindow`.
    ///
    /// `ProfileMenuLayout`'s mapping, for its reason: a list that hides
    /// something must hand back the identity of what it drew rather than the
    /// place on the glass.
    rows: Vec<usize>,
    /// The parent row this hangs off.
    kind: PaneMenuRow,
    /// Which way it grew out of that row — [`ProfileMenuLayout::travel`]'s
    /// field, and the one place in this window where the answer is horizontal.
    travel: Travel,
}

impl PaneSubmenuLayout {
    /// Which way this child grew out of its parent's row.
    #[must_use]
    pub fn travel(&self) -> Travel {
        self.travel
    }
}

impl PaneMenuLayout {
    /// Which way this menu grew out of the press that raised it.
    #[must_use]
    pub fn travel(&self) -> Travel {
        self.travel
    }

    /// Which way the child hanging off one of its rows grew, when one is up.
    #[must_use]
    pub fn submenu_travel(&self) -> Option<Travel> {
        self.submenu.as_ref().map(PaneSubmenuLayout::travel)
    }

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
        let index = self
            .rows
            .iter()
            .position(|it| *it == row)
            .expect("every row this menu is showing has a box");
        self.items[index]
    }

    /// **What this menu is showing**, top to bottom — the walk the keyboard
    /// takes ([`PaneMenuHover::step`]) and the list a pin asks the length of.
    #[must_use]
    pub fn rows(&self) -> &[PaneMenuRow] {
        &self.rows
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

    /// The child's row boxes, when one is up — [`Self::item`]'s opposite number
    /// for the second surface, and read for its reason: a pin about the strip
    /// *between* the frame and the first row has to know where both of them are,
    /// and deriving the second from the padding constants would be a pin that
    /// agreed with the layout by arithmetic rather than by reading it.
    #[allow(dead_code)]
    #[must_use]
    pub fn submenu_rows(&self) -> Option<&[[f32; 4]]> {
        self.submenu
            .as_ref()
            .map(|submenu| submenu.items.as_slice())
    }

    /// **Which row's list is up** (B9), or `None` when no child is.
    ///
    /// The press handler's question and the painter's: two rows hang lists off
    /// themselves now, and a `Submenu(index)` hit means an index into whichever
    /// of the two this is.
    #[must_use]
    pub fn submenu_kind(&self) -> Option<PaneMenuRow> {
        self.submenu.as_ref().map(|submenu| submenu.kind)
    }

    /// **What the child's `at`th row is about** — a `PROFILES` index under
    /// `Split with`, a place in the window list under `Move to window`.
    ///
    /// [`PaneMenuHit::Submenu`] and [`PaneMenuHover::Submenu`] both count *rows
    /// on the glass*, and this is the one door from that number to the thing it
    /// stands for. They used to disagree — the hit answered with the profile's
    /// table index while the keyboard walk counted rows — which was harmless
    /// only for as long as every profile in the table was offered, and would
    /// have lit the wrong row on the first machine missing one.
    #[must_use]
    pub fn submenu_row(&self, at: usize) -> Option<usize> {
        self.submenu
            .as_ref()
            .and_then(|submenu| submenu.rows.get(at).copied())
    }

    /// Whether this point is on either surface. Two rectangles, because a menu
    /// with a submenu open is two windows and a pointer on the second one has
    /// not left the first.
    ///
    /// **And the union of them is continuous**, which is a fact about
    /// [`pane_submenu_layout`]'s seam rather than about this function: the child
    /// stands on the parent's own border, so every column between the two
    /// belongs to one of them and there is no x at which a hand crossing from
    /// parent to child is reported as having left the menu. It read as a
    /// four-pixel gap for one afternoon, and that is the whole of the second
    /// user report of 2026-08-19.
    #[must_use]
    pub fn contains(&self, x: f32, y: f32) -> bool {
        contains(self.frame, x, y)
            || self
                .submenu
                .as_ref()
                .is_some_and(|submenu| contains(submenu.frame, x, y))
    }

    /// **Whether this point is on the child at all** — its rows, its padding,
    /// its border (user report, 2026-08-19, second cause).
    ///
    /// [`pane_menu_hit`] answers `Surface` for a point on the child that is not
    /// on one of its *rows*, and `Surface` is the same word it uses for a point
    /// on the parent's padding — so a caller that has to tell "on the child"
    /// from "not on the child" cannot get it from the hit alone. That caller is
    /// the safety triangle, and reading `Surface` as "not on the child" is what
    /// made the child close **under the pointer that had just landed on it**:
    /// the child's own left border and padding are five logical pixels wide, the
    /// hand crosses them on the way in, and the triangle's base is the child's
    /// left edge — which by then is *behind* the pointer, so the aim test says
    /// "not travelling toward it" and the child goes. Measured on the machine:
    /// a hop that lands anywhere in the child's leading 5px shut it every time,
    /// which is the whole of what the report called 断 hover.
    ///
    /// It is a question about the frame and not about the rows, so it is asked
    /// of the frame.
    #[must_use]
    pub fn on_submenu(&self, x: f32, y: f32) -> bool {
        self.submenu
            .as_ref()
            .is_some_and(|submenu| contains(submenu.frame, x, y))
    }

    /// **Whether the menu still has this hand** — the two surfaces, the seam
    /// between them, and the aim across them, as one region.
    ///
    /// [`Self::contains`] answers where the pointer *is*; this answers whether
    /// the pointer is still dealing with this menu, which is not the same
    /// question on either of the two edges it differs on:
    ///
    /// * A hand that has crossed onto the child is on the menu, and `contains`
    ///   already says so.
    /// * A hand cutting **diagonally** from the heading to a child row that
    ///   hangs below the parent's own bottom edge is over neither rectangle for
    ///   part of the trip. It has plainly not left — it is aiming at a surface
    ///   this menu put on the screen — and [`safe_triangle_holds`] is the
    ///   industry's own answer to exactly that trip, already used one level down
    ///   to keep the *child* open. This is the same verdict read one level up,
    ///   so that the leave grace on the *parent* cannot fire against a hand the
    ///   child is already holding.
    ///
    /// `from` is where the pointer was on the previous move, which is what makes
    /// the triangle a statement about direction; `None` (the first move after
    /// the menu opened) has no direction to read and falls back to the
    /// rectangles.
    #[must_use]
    pub fn holds(&self, from: Option<[f32; 2]>, to: [f32; 2]) -> bool {
        if self.contains(to[0], to[1]) {
            return true;
        }
        let (Some(submenu), Some(from)) = (self.submenu_frame(), from) else {
            return false;
        };
        safe_triangle_holds(from, to, submenu)
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
/// `zoomed` says which face the zoom row is wearing (§7.1.6l), and it is here
/// rather than only in the painter because the frame is measured against the
/// words it is about to hold: `Restore pane` and `Zoom pane` are not the same
/// width, and a menu laid out for one and drawn with the other would clip its
/// own first row at whichever language made it wider.
///
/// `windows` decides **how many rows there are** as well as how wide the child
/// is (user ruling 2026-08-25): with nowhere to go, `Move to window ▸` is not
/// drawn at all. See [`PaneMenuRow::rows`].
// Eight, and the eighth is the shortcut table (系统性发现 ②). The seven before
// it were already this list, and the honest fix for the count is the `look`
// struct `term_menu_layout` and `git_menu_layout` carry — a refactor of every
// caller for no change on the glass, which is not what this slice is. The allow
// is the same one `push_caps` and `push_button` carry two screens up.
#[allow(clippy::too_many_arguments)]
pub fn pane_menu_layout(
    point: [f32; 2],
    surface: (f32, f32),
    scale: f32,
    submenu: Option<PaneMenuRow>,
    zoomed: bool,
    windows: &[String],
    shortcuts: &crate::shortcuts::Shortcuts,
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
    // **The rows this menu is showing**, which is the one list the width, the
    // height, the boxes, the paint and the hit test are all built from.
    let rows = PaneMenuRow::rows(!windows.is_empty());
    // **And the chord each of them also answers to** (系统性发现 ②), measured
    // here because this is where the font is and carried on the layout because
    // the frame is about to be measured around them.
    let accels: Vec<Option<(String, f32)>> = rows
        .iter()
        .map(|row| {
            accelerator_of(
                row.accelerator(),
                row.has_submenu(),
                shortcuts,
                scale,
                measure,
            )
        })
        .collect();
    let accel_claim = accelerator_claim(&accels, scale);
    let text_rows = rows.len() - 1;
    let content = rows
        .iter()
        .filter(|row| **row != PaneMenuRow::Picker)
        .map(|row| {
            px(ITEM_ICON_COLUMN_LOGICAL_PX)
                + px(ITEM_GAP_LOGICAL_PX)
                + measure(row.text_when(zoomed), px(ITEM_FONT_LOGICAL_PX))
                + indicator
                + accel_claim
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
        + text_rows as f32 * item_height
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

    // One walk of the shown rows lays every entry out, which is what keeps the
    // order on screen and the order the keyboard walks from being two lists.
    let mut items = vec![[0.0_f32; 4]; rows.len()];
    let mut separator = [0.0_f32; 4];
    for (index, row) in rows.iter().enumerate() {
        let height = match row {
            PaneMenuRow::Picker => picker_height,
            _ => item_height,
        };
        // The rule falls where the sentence changes: five verbs that make a pane
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
    debug_assert_eq!(accels.len(), rows.len());

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

    // **The child hangs off the row that opened it**, which is the whole of
    // what the second submenu cost this function: `items[1]` was the right
    // heading while `Split with` was the only row with a list, and it was the
    // right *number* for the wrong reason — the heading is a fact about which
    // row is open, not about which position happens to be a submenu.
    let submenu = submenu
        // **And a row this menu is not showing hangs nothing** (user ruling
        // 2026-08-25): the state that says which child is open outlives one
        // frame, so a window that closed while `Move to window ▸` was open would
        // otherwise be asking for the box of a row that is no longer there.
        .filter(|row| row.has_submenu() && rows.contains(row))
        .map(|kind| {
            let heading = items[rows
                .iter()
                .position(|row| *row == kind)
                .expect("the row was just checked to be one this menu shows")];
            pane_submenu_layout(
                frame,
                heading,
                kind,
                windows,
                surface,
                scale,
                border,
                padding,
                item_height,
                measure,
            )
        })
        // A list with nothing in it is not a list: one window has nowhere to
        // send a pane, and a child frame drawn around no rows is an empty box
        // the pointer can get lost in.
        .filter(|child| !child.items.is_empty());

    PaneMenuLayout {
        scale,
        frame,
        rows,
        items,
        accels,
        picker_pane,
        zones,
        zone_hits,
        caption,
        separator,
        zoomed,
        submenu,
        travel: Travel::away_from(pressed_at(point), frame),
    }
}

/// Where the `Split with` submenu hangs.
///
/// **Against the parent's right edge, level with the heading's own top padding,
/// and flipped against its left edge when the window's right edge is too
/// close** — the two rules every submenu in every product follows, and the
/// second is not optional: a menu opened on a pane head near the right edge is a
/// menu whose child has nowhere to go on that side, and a child clamped instead
/// of flipped would sit *on top of* the parent it hangs from.
///
/// # The seam is one border wide, and it is an overlap (user reports 2026-08-19)
///
/// This edge has now been wrong in both directions on the same day, and the two
/// reports together are what fixes it.
///
/// The first version seated the child `border + padding` *inside* the parent's
/// edge, so the child covered the right-hand column of every row it stood
/// beside — including the `▸` on the very heading it hangs from — and the parent
/// read as truncated. The fix pushed the child *clear* by
/// [`MENU_OFFSET_LOGICAL_PX`], and the second report is what that bought: four
/// pixels of window between the two surfaces that belong to **neither** of them,
/// so a hand crossing from parent to child passes through a column where
/// [`PaneMenuLayout::contains`] answers "not on this menu" — and the pane
/// chevron's leave grace ([`CHEVRON_LEAVE_GRACE`]) starts running against a hand
/// that has not left anything. Slowly enough, the whole menu shuts in the gap it
/// was drawn across. It is `UI-UX.md` §十 principle 1 exactly: *展开后的范围必须
/// 完整包含触发它的范围*, and a gap is the one shape that cannot.
///
/// **So the child overlaps by exactly `border`**, and that number is not a taste:
///
/// * Qt states it as a metric of its own — `QStyle::PM_SubMenuOverlap`, "the
///   horizontal overlap between a submenu and its parent" — i.e. the toolkit's
///   named quantity here is an *overlap*, and the common style derives it from
///   the menu frame's own panel width, so the two frames' borders land on each
///   other and the seam is not a gap but a shared hairline.
/// * GTK spells the same thing as `GtkMenu`'s negative `horizontal-offset`.
/// * Radix (and, through it, most of the web) hangs `SubContent` flush against
///   its trigger with no side offset at all, and then pushes the grace polygon's
///   apex **back into** the trigger by five pixels (`const bleed = rightSide ?
///   -5 : +5`) precisely so that no un-owned column can exist between them.
///
/// One border is the smallest overlap that makes the union continuous, and it
/// covers only the parent's own hairline — never a row's text, which is what the
/// first report was about. Both claims are pinned by
/// `the_submenu_meets_the_parent_on_its_own_border_with_no_column_between_them`.
///
/// The safety triangle is untouched by this and stays what it was: the seam is
/// what a *straight* crossing needs, and the triangle is what a *diagonal* one
/// needs — a hand cutting across the parent's other rows toward a child row is
/// outside both rectangles no matter how they are seated.
///
/// **When neither side fits**, the child takes whichever has more room and is
/// cropped by the window rather than moved over the parent. That is a window
/// narrower than two menus side by side, and of the two failures available —
/// a child running off the screen, or a child hiding the rows you are choosing
/// between — only the first leaves the parent readable.
#[allow(clippy::too_many_arguments)]
fn pane_submenu_layout(
    parent: [f32; 4],
    heading: [f32; 4],
    kind: PaneMenuRow,
    windows: &[String],
    surface: (f32, f32),
    scale: f32,
    border: f32,
    padding: f32,
    item_height: f32,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> PaneSubmenuLayout {
    let px = |value: f32| value * scale;
    let hint = measure(current_profile_hint_text(), px(HINT_FONT_LOGICAL_PX));
    let chrome = 2.0 * (border + padding) + 2.0 * px(ITEM_PADDING_X_LOGICAL_PX);
    // **The window list has no hint column and no icon column** (B9): every row
    // of it says the same kind of thing, so there is no default to mark and no
    // two kinds of destination to tell apart with a glyph. The profile list has
    // both, which is why the two widths are measured apart rather than one being
    // made to fit the other's furniture.
    let offered: Vec<usize> = match kind {
        PaneMenuRow::MoveToWindow => (0..windows.len()).collect(),
        _ => table().offered(),
    };
    let content = offered
        .iter()
        .map(|index| match kind {
            PaneMenuRow::MoveToWindow => measure(
                windows.get(*index).map_or("", String::as_str),
                px(ITEM_FONT_LOGICAL_PX),
            ),
            _ => {
                px(ITEM_ICON_COLUMN_LOGICAL_PX)
                    + px(ITEM_GAP_LOGICAL_PX)
                    + measure(title(*index), px(ITEM_FONT_LOGICAL_PX))
                    + px(ITEM_GAP_LOGICAL_PX)
                    + hint
            }
        })
        .fold(0.0_f32, f32::max);
    let width = (chrome + content)
        .max(px(FILE_MENU_MIN_WIDTH_LOGICAL_PX))
        .round();
    let height = (2.0 * (border + padding) + offered.len() as f32 * item_height).round();

    let (surface_width, surface_height) = surface;
    let edge = px(MENU_EDGE_MARGIN_LOGICAL_PX);
    // The shared hairline: the child's near edge stands **on** the parent's
    // border rather than beside it, so the two rectangles share a column and the
    // union of them has no hole. See this function's own header for why the
    // number is the border and not a gap.
    let seam = border;
    let right_of = parent[2] - seam;
    let left_of = parent[0] + seam - width;
    let left = if right_of + width + edge <= surface_width {
        right_of
    } else if left_of >= edge {
        left_of
    } else if parent[0] >= surface_width - parent[2] {
        // Neither side holds it. Take the side with more room and let the
        // window crop it — clamping to `edge` here is what would put the child
        // back on top of the parent, which is the one outcome this placement
        // exists to prevent.
        left_of
    } else {
        right_of
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
    let mut items = Vec::with_capacity(offered.len());
    for _ in 0..offered.len() {
        items.push([content_left, cursor, content_right, cursor + item_height]);
        cursor += item_height;
    }
    PaneSubmenuLayout {
        frame,
        items,
        rows: offered,
        kind,
        // **Off the row, not off the menu.** A child hangs on the seam beside
        // the heading that opened it, so the heading is what it grew out of —
        // which is also what makes the flip above visible in the entrance: a
        // child forced to the left arrives from the right, and says so.
        travel: Travel::away_from(heading, frame),
    }
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
        for (row, rect) in submenu.items.iter().enumerate() {
            if contains(*rect, x, y) {
                return Some(PaneMenuHit::Submenu(row));
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
    // Walked over the layout's own rows beside its own boxes, stepping over the
    // picker — the painter's own walk, and one list for the same reason.
    // `items[1..]` was true only while the picker was the first entry; door 4
    // now stands above it (§7.1.6b′ ④), and a hit test that assumed where the
    // drawing sits would answer every press with the name of the row below it.
    for (row, rect) in layout.rows.iter().zip(layout.items.iter()) {
        if *row == PaneMenuRow::Picker {
            continue;
        }
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
    windows: &[String],
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

    // **Walked over the layout's own rows beside its own boxes**, rather than
    // over a second list against a slice of them. An `items[1..]` would be true
    // only while the picker is the first entry — it stopped being so the day a
    // row was inserted above it and started again the day that row was withdrawn
    // — and a zip that assumes where the drawing sits is a zip that silently
    // draws every caption one row out of place the next time the order changes.
    // Since 2026-08-25 the list is not even a constant: a session with one
    // window shows no `Move to window ▸`.
    for ((row, rect), accel) in layout
        .rows
        .iter()
        .zip(layout.items.iter())
        .zip(layout.accels.iter())
    {
        if *row == PaneMenuRow::Picker {
            continue;
        }
        push_row(
            &Row {
                rect: *rect,
                // Both from the layout's own snapshot (§7.1.6l), so the word and
                // the mark cannot come from two different readings of the state.
                mark: row.mark_when(layout.zoomed),
                name: row.text_when(layout.zoomed),
                hint: None,
                // The very string the frame was measured around — see
                // [`PaneMenuLayout::accels`].
                accel: accel.clone(),
                dirty: false,
                hovered: hover == Some(PaneMenuHover::Row(*row)),
                // A split the solver has no room for is refused *by the solver*,
                // after the press, exactly as the chord's is; a pane can always
                // be closed; and the chooser `New terminal in folder…` opens is
                // Windows', which this window does not get to promise about in
                // advance. Nothing here is a promise this build knows it cannot
                // keep.
                available: true,
                pin: None,
            },
            scale,
            palette,
            &mut quads,
            &mut labels,
            &mut sprites,
        );
        if row.has_submenu() {
            // The `▸`, which is `#i-tri` **at rest**: the file tree's disclosure
            // triangle points right until something opens it, and pointing right
            // is the whole of what a submenu indicator says. No new glyph, no new
            // angle, and no fifth close-enough triangle in a build that already
            // has one — see `ChromeMark::TreeDisclosure`, whose own note argues
            // that three marks differing by where a line falls are three marks
            // nobody can tell apart at fourteen pixels. Asked of the registry
            // since P2, with the file menu's own submenu row.
            let size = px(SUBMENU_INDICATOR_LOGICAL_PX).round().max(1.0);
            let right = rect[2] - px(ITEM_PADDING_X_LOGICAL_PX);
            let top = ((rect[1] + rect[3] - size) / 2.0).round();
            sprites.push(ChromeSprite::new(
                ActionIcon::OpenSubmenu.mark(),
                [right - size, top, right, top + size],
                if hover == Some(PaneMenuHover::Row(*row)) {
                    palette.menu_item_text_selected
                } else {
                    palette.menu_item_hint_text
                },
            ));
        }
        // The rule, struck at the row it stands **above** rather than at the one
        // it happens to follow: the row above it can be missing (user ruling
        // 2026-08-25), and a separator keyed to that one would disappear with
        // it — which is the same list the layout keys it to, one screen up.
        if *row == PaneMenuRow::ClosePane {
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
            windows,
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
        mono: false,
        text: picker_caption_text().to_owned(),
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
    windows: &[String],
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
    // **`at` is the row on the glass and `of` is what it is about**, which is
    // the distinction `PaneMenuLayout::submenu_row` exists to keep: the hover
    // and the hit both count rows, and only the lookups below want the identity.
    for (at, rect) in layout.items.iter().enumerate() {
        let of = layout.rows.get(at).copied().unwrap_or(at);
        let hovered = hover == Some(PaneMenuHover::Submenu(at));
        match layout.kind {
            // **A window row is words and nothing else** (B9). There is no
            // default window to mark and no second kind of window to tell apart
            // with a glyph, so a mark column here would be a column of one
            // repeated drawing — and every row is offered, because a window that
            // is open is a window a pane can be put in.
            PaneMenuRow::MoveToWindow => push_row(
                &Row {
                    rect: *rect,
                    mark: None,
                    name: windows.get(of).map_or("", String::as_str),
                    hint: None,
                    // No row of this menu is a row of the shortcut table (系统性发现 ②).
                    accel: None,
                    dirty: false,
                    hovered,
                    available: true,
                    pin: None,
                },
                scale,
                palette,
                &mut quads,
                &mut labels,
                &mut sprites,
            ),
            _ => {
                let hint = (current_profile == Some(of)).then(|| {
                    (
                        current_profile_hint_text().to_owned(),
                        measure(current_profile_hint_text(), px(HINT_FONT_LOGICAL_PX)),
                    )
                });
                push_row(
                    &Row {
                        rect: *rect,
                        mark: Some(mark(of)),
                        name: title(of),
                        hint,
                        // No row of this menu is a row of the shortcut table (系统性发现 ②).
                        accel: None,
                        dirty: false,
                        hovered,
                        // The picker's own rule, and the same fact: a profile
                        // whose program this machine does not have cannot start
                        // a shell, and a row that lights under the pointer and
                        // then does nothing is worse than one that says so.
                        available: programs.is_available(of),
                        pin: None,
                    },
                    scale,
                    palette,
                    &mut quads,
                    &mut labels,
                    &mut sprites,
                );
            }
        }
    }
    vec![OverlayLayer {
        quads,
        labels,
        sprites,
        ..Default::default()
    }]
}

// ── a tab's own context menu (丙2, the invisible-gestures audit 2026-08-26) ──
//
// **The ninth menu family, and the first one whose subject is a tab.**
//
// The audit counted eighty-seven gestures in this product and found seven with
// no clue anywhere on the glass. Three of the seven were the **only** door their
// verb has, and 丙2 is the worst of those three: dragging a tab past the window's
// own edge tears it into a window of its own, and `ChromeTarget::Tab` had
// exactly one press arm in the whole program — an activation — so there was no
// list, no tip and no row anywhere that said the verb exists. A pane has carried
// `Move pane to new window` in its `⌄` since F1c; a tab had nothing at all,
// because this product had no tab context menu at all.
//
// **The prescription was a menu and not a hint**, and the audit's own reason is
// the one this window keeps arriving at: there is no first-run mechanism here —
// no `first_run`, no `onboarding`, no `seen_once` anywhere in the repository —
// so the whole teaching surface is tooltips, menu rows, tip cards and five
// sentences in Settings. A verb with no row behind it can therefore only be
// taught by accident. One list a right click reaches turns 丙2 from
// 「自创且无线索」 into 「菜单里有同义行」, and carries the rest of a tab's verbs
// with it.
//
// **It is the pane menu's family, down to the geometry**, and deliberately not a
// second mechanism: the same [`Row`] and [`push_row`], the same item height,
// icon column, padding and indicator, the same [`pane_submenu_layout`] for the
// child, the same [`safe_triangle_holds`] on the way to it. What is new is only
// what a menu is allowed to differ in — how many rows, which words, which marks,
// where the rule falls — which is the ruling `#pane-menu` made first and every
// menu in this file has been cloned rather than generalised under since.

/// One row of a tab's context menu.
///
/// Closed rather than a list of words, on [`PaneMenuRow`]'s argument: a menu
/// whose entries are an enum is a menu whose keyboard walk cannot land on a row
/// that is not on the glass. Two things vary and both are asked of a subject
/// rather than of the window — which rows are *shown* ([`Self::rows`]) and which
/// of them can *answer* ([`Self::available`]) — because everything this menu
/// knows was true at the moment it was raised.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TabMenuRow {
    /// **The name, opened for editing** — the editor a double click on the tab
    /// body already opens, through the same `Runtime::open_rename`.
    ///
    /// It leads the list because it is the one verb here that changes nothing
    /// but the words: everything below it moves the tab somewhere or ends it,
    /// and a reader scanning down meets the harmless entry first. §7.1.6e's rule
    /// applies to it exactly as it does to the four below — one verb, two doors,
    /// never two implementations — and the door it shares is a *gesture*, which
    /// is why this row is owed at all: a double click is not a thing a list can
    /// tell you about.
    Rename,
    /// **One row with two faces**, [`PaneMenuRow::ZoomPane`]'s arrangement: over
    /// an ordinary tab it offers the pin, over a pinned one it offers the way
    /// back. See [`Self::text_when`] and [`Self::mark_when`], which turn
    /// together — the `Lock` ruling (§7.1.6c-8) says why they may never turn
    /// apart.
    Pin,
    /// **A tab seeded from this one** — its profile and its folder.
    ///
    /// The `+` opens the default profile wherever the pane you were looking at
    /// is standing; this opens *this tab's* shell where *this tab's* shell is,
    /// which is the same relationship `Duplicate pane` has to a bare split. The
    /// two facts come off one leaf, which is the rule [`new_tab_cwd`] already
    /// states: a profile taken from one pane and a folder from another describes
    /// a pane that does not exist.
    Duplicate,
    /// **The tab leaves this window and becomes a window of its own** — the row
    /// this whole menu was built for (丙2).
    ///
    /// It is `Move pane to new window` read one container up, and it is *shorter*
    /// than that verb rather than longer: a pane has to be promoted to a tab
    /// before it can move, and a tab is already a tab. So this row is the second
    /// half of that journey with the first half deleted, which is why it can
    /// push `NewWindowPlan::receiving` directly and needs no promotion recorded
    /// in the errand it writes.
    MoveToNewWindow,
    /// **Move this tab into a window that is already open** — the same third
    /// exit the pane menu grew on 2026-08-25, and a submenu for that row's
    /// reason: a new window is a place this verb *makes*, so there is nothing to
    /// choose between; an open window is a place that already exists, and which
    /// one is the whole of what the row is asking.
    MoveToWindow,
    /// The same verb the tab's own `×` has, and the one this menu puts a rule
    /// above.
    Close,
}

/// What the tab under the menu can answer for.
///
/// Two facts and no tab, [`TermMenuSubject`]'s arrangement and for its reason:
/// this menu is laid out and drawn from a snapshot taken when it was raised, so
/// that a strip reordering under an open menu — or the shell in the tab it is
/// about exiting — cannot rewrite a row under a hand already moving toward it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TabMenuSubject {
    /// Whether this tab is pinned, which is the face [`TabMenuRow::Pin`] wears.
    pub pinned: bool,
    /// **Whether this tab has a shell to copy.**
    ///
    /// False on a folder tab and on a file tab (§7.1.6h): a tab's `sessions` map
    /// may legitimately be empty, and a tab identified by a path on disk has
    /// neither a profile nor a working directory for a duplicate to be seeded
    /// from. The row is drawn **unavailable** rather than hidden, on
    /// [`term_menu_row_available`]'s own rule — a menu whose shape moves under
    /// the hand is a menu nobody can learn the shape of — and unavailable rather
    /// than arbitrary, because the only other answer available to this verb is
    /// "open a default shell somewhere", which is a tab nobody asked for wearing
    /// the name of a row that promised a copy.
    pub can_duplicate: bool,
}

impl Default for TabMenuSubject {
    /// **An ordinary tab**: not pinned, and with a shell behind it.
    ///
    /// Written out rather than derived, for [`TermMenuSubject`]'s reason: one of
    /// these two facts is good news and `bool::default()` is `false`, so a
    /// derived default would describe a folder tab — the rare shape — and every
    /// caller reaching for `..Default::default()` would silently be talking
    /// about one.
    fn default() -> Self {
        Self {
            pinned: false,
            can_duplicate: true,
        }
    }
}

impl TabMenuRow {
    /// Every entry, top to bottom — the order they are laid out in and the order
    /// a keyboard walks them.
    ///
    /// **Every verb here is a verb about this tab.** The rule is the one
    /// [`PaneMenuRow::ALL`] states about panes, and it is what keeps `New tab`,
    /// `Close window` and the tab layout out of a list whose every other line
    /// names the thing under the pointer.
    pub const ALL: [Self; 6] = [
        Self::Rename,
        Self::Pin,
        Self::Duplicate,
        Self::MoveToNewWindow,
        Self::MoveToWindow,
        Self::Close,
    ];

    /// **The rows one menu actually shows** — [`Self::ALL`] less the one row
    /// that can have nowhere to go.
    ///
    /// [`PaneMenuRow::rows`]'s judgement, and it transfers whole because the row
    /// is the same row: `Move to window ▸` hangs a list of the other windows off
    /// itself, and on a session with one window that list is empty. A disclosure
    /// arrow over nothing is worse than a missing row — the arrow says "there is
    /// more this way" and there is not — and nothing is lost by its going,
    /// because `Move tab to new window` directly above is the verb for exactly
    /// the case where there is no second window.
    ///
    /// A `Vec` and not a second `const`, for [`term_menu_entries`]'s reason: two
    /// lists that share five entries are two places for those five to change.
    #[must_use]
    pub fn rows(other_windows: bool) -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|row| other_windows || *row != Self::MoveToWindow)
            .collect()
    }

    /// Whether this row hangs a submenu off itself.
    #[must_use]
    pub fn has_submenu(self) -> bool {
        matches!(self, Self::MoveToWindow)
    }

    /// **Whether this row can do what it says**, on this tab.
    ///
    /// One row turns on the subject and the other five do not, and the five are
    /// always answerable: every tab has a name to edit, a pin to flip, a window
    /// to leave for and a way to be closed. A pinned tab included — the strip
    /// withholds the `×` from one (F61) and this row deliberately does not,
    /// because the row that would then be greyed stands directly under the row
    /// that ungreys it, and a reader who has just read `Unpin` is a reader who
    /// knows what to press.
    #[must_use]
    pub fn available(self, subject: TabMenuSubject) -> bool {
        match self {
            Self::Duplicate => subject.can_duplicate,
            Self::Rename | Self::Pin | Self::MoveToNewWindow | Self::MoveToWindow | Self::Close => {
                true
            }
        }
    }

    /// **The words on this row, told what the tab under the menu is.**
    ///
    /// Five of the six read the same whatever the tab is doing. [`Self::Pin`] is
    /// the sixth, and it is the one place in this menu where the state is a
    /// *word* rather than a greying — [`PaneMenuRow::text_when`]'s seam, on this
    /// menu's own one row with two faces.
    #[must_use]
    pub fn text_when(self, subject: TabMenuSubject) -> &'static str {
        match self {
            Self::Rename => tab_menu_rename_text(),
            Self::Pin if subject.pinned => tab_menu_unpin_text(),
            Self::Pin => tab_menu_pin_text(),
            Self::Duplicate => tab_menu_duplicate_text(),
            Self::MoveToNewWindow => tab_menu_move_to_new_window_text(),
            // **The pane menu's own string** and not a second one that happens
            // to read the same: the row opens a list of windows, which is the
            // same sentence about the same destinations whichever thing is being
            // moved into them. Two literals here is how two menus come to
            // disagree about what a window is called.
            Self::MoveToWindow => move_to_window_text(),
            Self::Close => tab_menu_close_text(),
        }
    }

    /// **The mark on this row, told the same thing** — and it turns with the
    /// word, never on its own (§7.1.6c-8: change only the word and the drawing
    /// goes on lying; change only the drawing and the reader has to decide which
    /// half to believe).
    ///
    /// Five of the six are the registry's, asked by name. The pin is the sixth
    /// and it is `ChromeMark::Pin { filled }` directly, which is what the tab
    /// strip's own pin draws and what [`RowPin`] draws: state rides on the fill
    /// and never on a different glyph, so there is one drawing with two faces
    /// rather than two drawings. [`PaneMenuRow::mark_when`] reaches past the
    /// registry for `ChromeMark::PaneZoom { zoomed }` in exactly the same way and
    /// for exactly this reason — a *face* is not a verb, and the registry indexes
    /// verbs.
    #[must_use]
    pub fn mark_when(self, subject: TabMenuSubject) -> ChromeMark {
        match self {
            // The pencil, which three verbs in this window already share —
            // `Rename`, `Rename branch` and a settings row's `Edit`. The
            // registry's variant is named for the surface the pencil was first
            // needed on; what it indexes is the act, and renaming a tab is that
            // act.
            Self::Rename => ActionIcon::RenameFile.mark(),
            Self::Pin => ChromeMark::Pin {
                filled: subject.pinned,
            },
            // `#i-duplicate` on the same terms: one drawing for "another one of
            // this, seeded from this", which is what the row says one container
            // up from the pane it was struck for.
            Self::Duplicate => ActionIcon::DuplicatePane.mark(),
            // **The bare arrow and the window it points out of** — the pair the
            // pane menu's two exits already wear, and the pair is the whole of
            // the distinction: one names a window this verb makes, the other a
            // window that is already there.
            Self::MoveToNewWindow => ActionIcon::MoveToNewWindow.mark(),
            Self::MoveToWindow => ActionIcon::MoveToWindow.mark(),
            // **The tab's own `×` and not the pane's** — the two are one
            // `<symbol>` under two names, and this row closes a *tab*.
            Self::Close => ActionIcon::CloseTab.mark(),
        }
    }
}

/// **`Rename tab`** — the tab menu's first row.
#[must_use]
pub fn tab_menu_rename_text() -> &'static str {
    crate::i18n::Text::TabMenuRename.text()
}
/// The pin row's unpinned face — **the tab strip's own word**, reused rather
/// than restated: there is one pin verb in this product (the preview head's
/// control is a *lock*, §7.7 ⑧), so the strip's tip and this row say the same
/// thing because they are the same thing.
#[must_use]
pub fn tab_menu_pin_text() -> &'static str {
    crate::i18n::Text::Pin.text()
}
/// The pin row's pinned face.
///
/// **Not `Text::Unpin`**, and the reason is geometric rather than editorial:
/// that string is a *tip*, and it carries its own second clause — "a pinned tab
/// closes only after unpinning" — because a tip is where the mock-up (4204) put
/// the explanation of where the `×` went. A menu's width is the widest row it
/// holds, so a sentence in this slot would be a menu three times as wide as its
/// other five rows, on the one row that is a state rather than an errand.
#[must_use]
pub fn tab_menu_unpin_text() -> &'static str {
    crate::i18n::Text::TabMenuUnpin.text()
}
#[must_use]
pub fn tab_menu_duplicate_text() -> &'static str {
    crate::i18n::Text::TabMenuDuplicate.text()
}
/// One word apart from the pane menu's own exit, and the word is the thing being
/// moved.
#[must_use]
pub fn tab_menu_move_to_new_window_text() -> &'static str {
    crate::i18n::Text::TabMenuMoveToNewWindow.text()
}
/// The `×`'s verb, spelled — a menu row has room for the word the button does
/// not.
#[must_use]
pub fn tab_menu_close_text() -> &'static str {
    crate::i18n::Text::TabMenuClose.text()
}

/// What a point in one of the tab menu's two surfaces is over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TabMenuHit {
    /// A row that can answer. A greyed one is **not** reported: the pointer
    /// falls through it onto the menu's own body, so it neither lights nor
    /// answers a press.
    Row(TabMenuRow),
    /// A row of the open window list, by its place on the glass.
    Submenu(usize),
    /// Inside one of the two surfaces but on no control: the padding, the rule
    /// above `Close tab`, a greyed row.
    Surface,
}

/// What is lit — the pointer's hover and the keyboard's cursor, which are one
/// thing in a menu ([`PaneMenuHover`]'s own sentence).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TabMenuHover {
    /// A row of the menu proper.
    Row(TabMenuRow),
    /// A row of the open window list.
    Submenu(usize),
}

/// One laid-out row of the tab menu.
///
/// Its own type rather than a triple, for [`TermMenuItem`]'s reason: the three
/// facts are read together at every call site, and a tuple of a row, a rectangle
/// and a `bool` is a shape two of whose members can be swapped silently.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabMenuItem {
    pub row: TabMenuRow,
    pub rect: [f32; 4],
    pub available: bool,
}

/// Every rectangle the tab's context menu draws and hit-tests.
#[derive(Clone, Debug, PartialEq)]
pub struct TabMenuLayout {
    scale: f32,
    frame: [f32; 4],
    /// One entry per row this menu is showing, top to bottom — the one list the
    /// paint, the hit test and the keyboard walk are all built from.
    ///
    /// [`PaneMenuLayout::rows`]' ruling, and the same failure behind it: the
    /// width and the height were measured against *this* list, so a painter or a
    /// walk that worked the list out again would be a second opinion about a
    /// thing the layout already knows — and the two would disagree on exactly
    /// the frame where a second window opened or the last one closed.
    items: Vec<TabMenuItem>,
    /// The rule above `Close tab`, which separates the four verbs that name or
    /// move a tab from the one that ends it — the pane menu's own written rule:
    /// the rule falls where the sentence changes.
    separator: [f32; 4],
    /// **What the tab was when this menu was raised** — carried on the layout
    /// rather than passed to the painter a second time, so the words the frame
    /// was measured against are the words drawn into it. `PaneMenuLayout`'s
    /// `zoomed` exactly: `Pin` and `Unpin` are not the same width, and a menu
    /// laid out for one and drawn with the other would clip its own second row
    /// at whichever language made it wider.
    subject: TabMenuSubject,
    /// The window list's frame and rows, when it is open.
    ///
    /// [`PaneSubmenuLayout`] itself, laid out by [`pane_submenu_layout`]: the
    /// child hanging off this menu's heading is the *same* child that hangs off
    /// the pane menu's `Move to window ▸`, down to the seam it meets its parent
    /// on and the rows it draws, so it is the same type placed by the same
    /// function.
    submenu: Option<PaneSubmenuLayout>,
    /// Which way it grew — [`ProfileMenuLayout::travel`]'s field.
    travel: Travel,
}

impl TabMenuLayout {
    /// Which way this menu grew out of the press that raised it.
    #[must_use]
    pub fn travel(&self) -> Travel {
        self.travel
    }

    /// Which way the child hanging off its heading grew, when one is up.
    #[must_use]
    pub fn submenu_travel(&self) -> Option<Travel> {
        self.submenu.as_ref().map(PaneSubmenuLayout::travel)
    }

    /// **What this menu is showing**, top to bottom — the walk the keyboard
    /// takes ([`tab_menu_step`]) and the list a pin asks the length of.
    #[must_use]
    pub fn items(&self) -> &[TabMenuItem] {
        &self.items
    }

    /// The child's border box, when one is up — the safety triangle's base, and
    /// the second rectangle a press has to miss.
    #[must_use]
    pub fn submenu_frame(&self) -> Option<[f32; 4]> {
        self.submenu.as_ref().map(|submenu| submenu.frame)
    }

    /// The child's row boxes, when one is up — read by the pins about the
    /// child's geometry and by nothing in the window, which reaches its rows
    /// through [`tab_menu_hit`]. [`PaneMenuLayout::submenu_rows`]' arrangement
    /// and its reason: a pin about the strip *between* the frame and the first
    /// row has to know where both of them are, and deriving the second from the
    /// padding constants would be a pin that agreed with the layout by
    /// arithmetic rather than by reading it.
    #[allow(dead_code)]
    #[must_use]
    pub fn submenu_rows(&self) -> Option<&[[f32; 4]]> {
        self.submenu
            .as_ref()
            .map(|submenu| submenu.items.as_slice())
    }

    /// **Which window the child's `at`th row is about** —
    /// [`PaneMenuLayout::submenu_row`] on this menu's child, and for that
    /// method's reason: a hit counts rows on the glass, and only the lookup
    /// wants the identity.
    #[must_use]
    pub fn submenu_row(&self, at: usize) -> Option<usize> {
        self.submenu
            .as_ref()
            .and_then(|submenu| submenu.rows.get(at).copied())
    }

    /// **Whether this point is on the child at all** — its rows, its padding,
    /// its border. [`PaneMenuLayout::on_submenu`]'s reason verbatim: the hit
    /// answers `Surface` for both surfaces' padding, so the safety triangle
    /// cannot tell "on the child" from "not on the child" out of the hit alone,
    /// and reading it that way is what shut a child under the pointer that had
    /// just landed on it.
    #[must_use]
    pub fn on_submenu(&self, x: f32, y: f32) -> bool {
        self.submenu
            .as_ref()
            .is_some_and(|submenu| contains(submenu.frame, x, y))
    }
}

/// Lay a tab's context menu out under the point it was raised at.
///
/// **A point, not a tab** — [`term_menu_layout`]'s ruling, and it matters here
/// for a reason of its own: the strip under this menu can be scrolled,
/// re-ordered, widened by a tab arriving or narrowed by one leaving while the
/// menu stands, and a menu that re-found its tab every frame would walk across
/// the window while the reader was reading it.
///
/// `subject` decides **what one of the rows says and whether another can
/// answer**, and it is here rather than only in the painter because the frame is
/// measured against the words it is about to hold. `windows` decides **how many
/// rows there are** as well as how wide the child is: with nowhere to go,
/// `Move to window ▸` is not drawn at all — see [`TabMenuRow::rows`].
#[must_use]
pub fn tab_menu_layout(
    point: [f32; 2],
    surface: (f32, f32),
    scale: f32,
    subject: TabMenuSubject,
    submenu_open: bool,
    windows: &[String],
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> TabMenuLayout {
    let px = |value: f32| value * scale;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let padding = px(MENU_PADDING_LOGICAL_PX);
    let item_height = px(ITEM_HEIGHT_LOGICAL_PX).round();
    let separator_thickness = (SEPARATOR_THICKNESS_LOGICAL_PX * scale).round().max(1.0);
    let separator_margin = px(SEPARATOR_MARGIN_Y_LOGICAL_PX).round();
    let separator_block = 2.0 * separator_margin + separator_thickness;
    let chrome = 2.0 * (border + padding) + 2.0 * px(ITEM_PADDING_X_LOGICAL_PX);

    // **The rows this menu is showing**, which is the one list the width, the
    // height, the boxes, the paint and the hit test are all built from.
    let rows = TabMenuRow::rows(!windows.is_empty());
    // The `▸` claims the same slot the profile picker's `default` hint claims,
    // and it is reserved on **every** row of a menu that has a submenu at all —
    // [`pane_menu_layout`]'s rule, so that a menu whose width depended on which
    // rows had children could not change width the day a second row grew one. A
    // menu with no `Move to window ▸` in it has no child anywhere and reserves
    // nothing, which is [`term_menu_layout`]'s own qualification of that rule.
    let indicator = if rows.iter().any(|row| row.has_submenu()) {
        px(SUBMENU_INDICATOR_LOGICAL_PX) + px(ITEM_GAP_LOGICAL_PX)
    } else {
        0.0
    };
    let content = rows.iter().fold(0.0_f32, |wide, row| {
        wide.max(
            px(ITEM_ICON_COLUMN_LOGICAL_PX)
                + px(ITEM_GAP_LOGICAL_PX)
                + measure(row.text_when(subject), px(ITEM_FONT_LOGICAL_PX))
                + indicator,
        )
    });
    let width = (chrome + content)
        .max(px(FILE_MENU_MIN_WIDTH_LOGICAL_PX))
        .round();
    let height =
        (2.0 * (border + padding) + rows.len() as f32 * item_height + separator_block).round();

    let (surface_width, surface_height) = surface;
    let edge = px(MENU_EDGE_MARGIN_LOGICAL_PX);
    // Clamped on both axes, [`file_menu_layout`]'s own discipline: the strip runs
    // along the top of the window and the rail down its left edge, so a menu
    // raised on the last tab of a tall rail drops straight through the floor
    // without this.
    let left = point[0].min(surface_width - width - edge).max(edge).round();
    let top = point[1]
        .min(surface_height - height - edge)
        .max(edge)
        .round();
    let frame = [left, top, left + width, top + height];

    let content_left = frame[0] + border + padding;
    let content_right = frame[2] - border - padding;
    let mut cursor = frame[1] + border + padding;
    let mut items = Vec::with_capacity(rows.len());
    let mut separator = [0.0_f32; 4];
    let mut heading = None;
    for row in &rows {
        // The rule falls where the sentence changes: the verbs that name a tab
        // or move one, then the one that ends it. Struck at the row it stands
        // **above** rather than after the one it happens to follow, because the
        // row above it can be missing.
        if *row == TabMenuRow::Close {
            separator = [
                content_left,
                cursor + separator_margin,
                content_right,
                cursor + separator_margin + separator_thickness,
            ];
            cursor += separator_block;
        }
        let rect = [content_left, cursor, content_right, cursor + item_height];
        if row.has_submenu() {
            heading = Some(rect);
        }
        items.push(TabMenuItem {
            row: *row,
            rect,
            available: row.available(subject),
        });
        cursor += item_height;
    }

    // **The child hangs off the row that opened it**, placed by
    // [`pane_submenu_layout`] and not by a second copy of it: the list of
    // windows this menu hangs out is the same list the pane menu hangs out, so
    // the seam it meets its parent on, the side it flips to and the rows it
    // holds are one derivation. Only the parent it is measured against differs,
    // which is the argument that function already takes.
    let submenu = heading
        .filter(|_| submenu_open)
        .map(|heading| {
            pane_submenu_layout(
                frame,
                heading,
                PaneMenuRow::MoveToWindow,
                windows,
                surface,
                scale,
                border,
                padding,
                item_height,
                measure,
            )
        })
        // A list with nothing in it is not a list: a child frame drawn around no
        // rows is an empty box the pointer can get lost in.
        .filter(|child| !child.items.is_empty());

    TabMenuLayout {
        scale,
        frame,
        items,
        separator,
        subject,
        submenu,
        travel: Travel::away_from(pressed_at(point), frame),
    }
}

/// What a point is over, with the same answers every other menu here gives: a
/// row, the menu's own padding, or nothing at all.
///
/// **The child is asked first**, because it is drawn over the parent and
/// overlaps it: a point in the strip where the two frames cross belongs to the
/// child, exactly as the topmost window owns a point everywhere else in this
/// program.
///
/// A row that cannot do what it says is **not** offered — the pointer falls
/// through it onto the menu's body, which is [`term_menu_hit`]'s rule and the
/// one that makes `available` a refusal rather than a colour.
#[must_use]
pub fn tab_menu_hit(layout: &TabMenuLayout, x: f64, y: f64) -> Option<TabMenuHit> {
    let (x, y) = (x as f32, y as f32);
    if let Some(submenu) = layout.submenu.as_ref()
        && contains(submenu.frame, x, y)
    {
        for (row, rect) in submenu.items.iter().enumerate() {
            if contains(*rect, x, y) {
                return Some(TabMenuHit::Submenu(row));
            }
        }
        return Some(TabMenuHit::Surface);
    }
    for item in &layout.items {
        if item.available && contains(item.rect, x, y) {
            return Some(TabMenuHit::Row(item.row));
        }
    }
    contains(layout.frame, x, y).then_some(TabMenuHit::Surface)
}

/// The row a keyboard step lands on, **skipping the ones that answer nothing** —
/// [`term_menu_step`]'s rule and [`file_menu_step`]'s clamp, on this list.
///
/// The rule above `Close tab` is not a stop: the walk crosses it, because a
/// separator is punctuation and a keyboard that halted at one would make the
/// destructive row reachable by pointer only.
///
/// `rows` is **the picture**, not the vocabulary (the ruling of 2026-08-25): a
/// walk over [`TabMenuRow::ALL`] would stop the highlight on a
/// `Move to window ▸` that this session has no second window for.
#[must_use]
pub fn tab_menu_step(
    current: Option<TabMenuRow>,
    forwards: bool,
    rows: &[TabMenuItem],
) -> Option<TabMenuRow> {
    let walkable: Vec<TabMenuRow> = rows
        .iter()
        .filter(|item| item.available)
        .map(|item| item.row)
        .collect();
    // A menu with no answerable row in it has nowhere for a highlight to go —
    // answered rather than asserted, because this is a walk and a walk that
    // panicked would take the window with it.
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

/// The tab menu, and its window list when one is up, as overlay layers.
///
/// `programs` is the child's and is taken whether or not one is open, for
/// [`term_menu_build`]'s reason read the other way round: the child is placed
/// and drawn by the *pane* menu's own functions, which serve two kinds of list,
/// and this menu only ever hangs out the one of them that has no profiles in it.
/// Handing the argument through is what keeps that one drawing rather than two —
/// exactly as `term_menu_build` hands `&[]` through for the window list it has no
/// row for.
#[must_use]
pub fn tab_menu_build(
    layout: &TabMenuLayout,
    hover: Option<TabMenuHover>,
    windows: &[String],
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
    for item in &layout.items {
        let hovered = hover == Some(TabMenuHover::Row(item.row)) && item.available;
        push_row(
            &Row {
                rect: item.rect,
                // Both from the layout's own snapshot, so the word and the mark
                // cannot come from two different readings of the state.
                mark: Some(item.row.mark_when(layout.subject)),
                name: item.row.text_when(layout.subject),
                hint: None,
                // No row of this menu is a row of the shortcut table (系统性发现 ②).
                accel: None,
                dirty: false,
                hovered,
                available: item.available,
                // **The pin is in the mark column here and not at the trailing
                // edge.** [`RowPin`] is the control a row wears when the row
                // names something *else* that can be pinned — a folder in a list
                // of folders — and it is an offer that appears with the hand.
                // This row's whole subject is the pin, so the pin is its glyph.
                pin: None,
            },
            scale,
            palette,
            &mut quads,
            &mut labels,
            &mut sprites,
        );
        if item.row.has_submenu() {
            // The `▸`, which is `#i-tri` at rest — asked of the registry, in the
            // row's trailing padding, lit with the row. Three lines, the same
            // three the two menus above draw it with.
            let size = px(SUBMENU_INDICATOR_LOGICAL_PX).round().max(1.0);
            let right = item.rect[2] - px(ITEM_PADDING_X_LOGICAL_PX);
            let top = ((item.rect[1] + item.rect[3] - size) / 2.0).round();
            sprites.push(ChromeSprite::new(
                ActionIcon::OpenSubmenu.mark(),
                [right - size, top, right, top + size],
                if hovered {
                    palette.menu_item_text_selected
                } else {
                    palette.menu_item_hint_text
                },
            ));
        }
    }
    quads.push(OverlayQuad {
        rect: layout.separator,
        color: palette.menu_border,
        alpha: separator_alpha(palette.menu_border),
    });

    let mut layers = vec![OverlayLayer {
        quads,
        labels,
        sprites,
        ..Default::default()
    }];
    if let Some(submenu) = layout.submenu.as_ref() {
        // The pane menu's own child, drawn by the pane menu's own function. The
        // hover is translated at the boundary rather than shared as a type: what
        // is lit on *this* menu is this menu's fact, and the child only ever
        // needs the one arm of it that names one of its rows.
        let hover = match hover {
            Some(TabMenuHover::Submenu(index)) => Some(PaneMenuHover::Submenu(index)),
            _ => None,
        };
        layers.extend(push_submenu(
            submenu, scale, hover, None, programs, windows, measure,
        ));
    }
    layers
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
#[must_use]
pub fn git_filter_remotes_text() -> &'static str {
    crate::i18n::Text::GitFilterShowRemotes.text()
}
#[must_use]
pub fn git_filter_tags_text() -> &'static str {
    crate::i18n::Text::GitFilterShowTags.text()
}

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
    /// Which way it grew — [`ProfileMenuLayout::travel`]'s field.
    travel: Travel,
}

impl GitFilterMenuLayout {
    /// Which way this menu grew out of the filter button.
    #[must_use]
    pub fn travel(&self) -> Travel {
        self.travel
    }

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
        travel: Travel::away_from(anchor, frame),
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
        GitFilterRow::All => crate::git_graph::graph_filter_all(),
        GitFilterRow::Branch(name) => name,
        GitFilterRow::Remotes => git_filter_remotes_text(),
        GitFilterRow::Tags => git_filter_tags_text(),
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
        (_, true) => Some(ActionIcon::MenuTick.mark()),
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
                // No row of this menu is a row of the shortcut table (系统性发现 ②).
                accel: None,
                dirty: false,
                hovered: hover == Some(row),
                // Every row here is a setting, and a setting can always be set.
                // There is no machine on which one of these is a promise that
                // cannot be kept: what the *repository* answers to a filter is
                // git's business, and an empty graph is an honest answer.
                available: true,
                pin: None,
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

/// **What a dirty buffer's dot claims out of the switcher row** — `.pvm-dot`
/// (mock-up 581).
///
/// The dot itself is `crate::marks::dirty_dot_sprite`, which is where the other
/// three heads' dots already came from. This one was still `●` U+25CF set in
/// the menu's hint type until P1 — the last of the four, missed because it is
/// the only one that rides in a *row* rather than on a head, so it was reached
/// through the hint slot instead of through a sprite.
///
/// The width is the dot's own diameter and the row's gap, on
/// [`row_pin_claim`]'s pattern: a reservation is what stops a name shortening
/// when the thing beside it appears, so it is spent whether the dot is struck
/// or not.
#[must_use]
pub fn preview_menu_dirty_claim(scale: f32) -> f32 {
    (crate::marks::DIRTY_DOT_LOGICAL_PX * scale).round() + ITEM_GAP_LOGICAL_PX * scale
}

/// One row of the switcher: a live buffer in the tab's pool, **or a file the
/// user pinned and nobody has opened yet** (user ruling 2026-08-19).
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewMenuItem {
    pub name: String,
    /// Unsaved edits — the dot on the right of the row.
    pub dirty: bool,
    /// Whether this is the buffer the pane is showing (`.tm-item.cur`).
    pub current: bool,
    /// What this row names, when it names something that can be kept — the pin's
    /// category and its target.
    ///
    /// **The category is the row's, not the surface's** (W2 slice ③): one list
    /// holds files and pages, and a `pins.json` row is identified by its category
    /// *and* its target, so a switcher that pinned everything as a `file` would
    /// write a page into the section the root menu draws from.
    ///
    /// `None` for the rows that cannot be kept at all: a diff and a commit's
    /// reading of a file are documents this window computed, not places, and
    /// keeping one across a restart would mean keeping the question rather than
    /// the answer.
    pub keep: Option<PreviewMenuTarget>,
    /// Whether the user said to keep this file.
    ///
    /// The kept rows are at the front of the list — one list and one index
    /// space, [`apply_pins`]'s rule said about buffers.
    pub pinned: bool,
    /// Which buffer of the pool this row was drawn from, when it was drawn from
    /// one.
    ///
    /// `None` on a kept file that has no buffer yet — the row that makes the
    /// PINNED section worth having across a restart. Choosing it opens the file;
    /// choosing any other row is a change of view over a buffer that already
    /// exists.
    pub pool: Option<usize>,
}

impl PreviewMenuItem {
    /// Whether this row is a page — which decides its mark and, at the two call
    /// sites that navigate, which door it goes through.
    #[must_use]
    pub fn is_page(&self) -> bool {
        self.keep
            .as_ref()
            .is_some_and(|keep| keep.kind == bt_persist::PinKind::Url)
    }

    /// **The address this row stands for**, for a row that is a page.
    ///
    /// The one thing a favicon has to be looked up by, off the field the row
    /// already carries. `None` for a file, and for the same reason
    /// [`crate::marks::preview_row_mark`] ignores an icon handed to a file: what
    /// makes a row a page is its category, and a row that is not one has no site
    /// to be given somebody's icon from.
    #[must_use]
    pub fn page_url(&self) -> Option<&str> {
        self.keep
            .as_ref()
            .filter(|keep| keep.kind == bt_persist::PinKind::Url)
            .map(|keep| keep.target.as_str())
    }
}

/// **What one switcher row keeps** — a `pins.json` row's two halves.
///
/// A pair and not a bare string, for [`crate::pins::row_of`]'s reason: the table
/// is one array, so "is this pinned" has to be asked of a category *and* a
/// target or the answer comes back from somebody else's row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewMenuTarget {
    pub kind: bt_persist::PinKind,
    pub target: String,
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
    /// **The pane the switcher is hanging under, by its whole [`LeafId`]**
    /// (§7.12 ⓑ).
    ///
    /// This state is the *window's* — one pointer, one open menu — while the
    /// head it hangs from is a tab's. On a bare seat number, switching tabs with
    /// the switcher open handed it to whichever pane the arriving tab had
    /// numbered the same: the chevron turned, the rows listed that tab's pool,
    /// and a click chose a file for a pane the gesture had never been about.
    open: Option<LeafId>,
    hover: Option<PreviewMenuHit>,
}

/// What a point in the switcher is over: a row, or the pin at the end of one.
///
/// [`RootMenuHit`]'s division, said about the other menu that grew a pin on the
/// same day, and kept a separate type for the reason the two menus keep separate
/// row types: an index into the switcher is not an index into the root menu, and
/// a shared type would let one be passed where the other was meant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewMenuHit {
    /// The row itself: show this document in the pane.
    Row(usize),
    /// The pin: keep this file, or stop keeping it, and leave the menu open.
    Pin(usize),
}

impl PreviewMenuHit {
    /// Which row this is about, whichever half of it was hit.
    #[must_use]
    pub fn row(self) -> usize {
        match self {
            Self::Row(index) | Self::Pin(index) => index,
        }
    }
}

impl PreviewMenu {
    pub fn leaf(self) -> Option<LeafId> {
        self.open
    }

    /// The name button: open here, or shut if this very pane already has it open
    /// (P136 — "同一个 `data-leaf` 再点 → 收").
    pub fn toggle(&mut self, leaf: LeafId) {
        self.open = (self.open != Some(leaf)).then_some(leaf);
        self.hover = None;
    }

    pub fn close(&mut self) -> bool {
        let was_open = self.open.is_some();
        self.open = None;
        self.hover = None;
        was_open
    }

    pub fn set_hover(&mut self, hover: Option<PreviewMenuHit>) -> bool {
        let hover = self.open.and(hover);
        let changed = self.hover != hover;
        self.hover = hover;
        changed
    }

    pub fn hover(self) -> Option<PreviewMenuHit> {
        self.hover
    }
}

/// Every rectangle the switcher draws and hit-tests.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewMenuLayout {
    scale: f32,
    frame: [f32; 4],
    /// The `PINNED` heading and the hairline under the rows it heads — absent
    /// when nothing is pinned. The rows below the hairline keep no heading of
    /// their own: they are what the switcher has always been, and naming them
    /// now would be labelling a list that never needed a label.
    pinned_label: Option<[f32; 4]>,
    /// …and absent again when *everything* is pinned, because a rule with
    /// nothing under it is a boundary between one thing and no things.
    pinned_separator: Option<[f32; 4]>,
    items: Vec<[f32; 4]>,
    /// Which way it grew — [`ProfileMenuLayout::travel`]'s field.
    travel: Travel,
}

impl PreviewMenuLayout {
    /// Which way this switcher grew out of the name it hangs under.
    #[must_use]
    pub fn travel(&self) -> Travel {
        self.travel
    }
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
    let dot = preview_menu_dirty_claim(scale);
    let chrome = 2.0 * (border + padding) + 2.0 * px(ITEM_PADDING_X_LOGICAL_PX);
    // The pin's column, reserved wherever any row could carry one — the dot's
    // own reservation, one control further out. A switcher of nothing but
    // computed documents has no pin anywhere and gives the width back.
    let pin = if items.iter().any(|item| item.keep.is_some()) {
        row_pin_claim(scale)
    } else {
        0.0
    };
    let content = items
        .iter()
        .map(|item| {
            px(ITEM_ICON_COLUMN_LOGICAL_PX)
                + px(ITEM_GAP_LOGICAL_PX)
                + measure(&item.name, px(ITEM_FONT_LOGICAL_PX))
                + dot
                + pin
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
    let kept = items.iter().take_while(|item| item.pinned).count();
    let separator_thickness = (SEPARATOR_THICKNESS_LOGICAL_PX * scale).round().max(1.0);
    let separator_margin = px(SEPARATOR_MARGIN_Y_LOGICAL_PX).round();
    let separator_block = 2.0 * separator_margin + separator_thickness;
    let section_block = px(SECTION_LABEL_PADDING_TOP_LOGICAL_PX
        + SECTION_LABEL_LINE_LOGICAL_PX
        + SECTION_LABEL_PADDING_BOTTOM_LOGICAL_PX)
    .round();
    // The rule is drawn only when something is under it — the root menu's rule,
    // and here it matters more, because keeping every open buffer is one press
    // away and would otherwise leave a hairline along the bottom of the box.
    let ruled = kept > 0 && kept < items.len();
    let kept_block = if kept == 0 {
        0.0
    } else {
        section_block + if ruled { separator_block } else { 0.0 }
    };
    let height = (2.0 * (border + padding) + kept_block + item_height * items.len() as f32).round();

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
    let mut rects = Vec::with_capacity(items.len());
    let (pinned_label, pinned_separator) = if kept == 0 {
        (None, None)
    } else {
        let band = [content_left, cursor, content_right, cursor + section_block];
        cursor += section_block;
        for _ in 0..kept {
            rects.push([content_left, cursor, content_right, cursor + item_height]);
            cursor += item_height;
        }
        let rule = ruled.then(|| {
            let rule = [
                content_left,
                cursor + separator_margin,
                content_right,
                cursor + separator_margin + separator_thickness,
            ];
            cursor += separator_block;
            rule
        });
        (Some(band), rule)
    };
    for _ in kept..items.len() {
        rects.push([content_left, cursor, content_right, cursor + item_height]);
        cursor += item_height;
    }
    PreviewMenuLayout {
        scale,
        frame,
        pinned_label,
        pinned_separator,
        items: rects,
        travel: Travel::away_from(anchor, frame),
    }
}

/// What a point is over, with the same three answers every other menu gives:
/// `None` for "not this menu at all", `Some(None)` for "the menu but no row".
#[must_use]
pub fn preview_menu_hit(
    layout: &PreviewMenuLayout,
    items: &[PreviewMenuItem],
    x: f64,
    y: f64,
) -> Option<Option<PreviewMenuHit>> {
    let (x, y) = (x as f32, y as f32);
    for (index, item) in layout.items.iter().enumerate() {
        if contains(*item, x, y) {
            // The pin is inside the row and is asked about first — and only on
            // rows that have one: a computed document's row is a row all the way
            // to its trailing edge.
            let keepable = items.get(index).is_some_and(|item| item.keep.is_some());
            return Some(Some(
                if keepable && contains(row_pin_rect(*item, layout.scale), x, y) {
                    PreviewMenuHit::Pin(index)
                } else {
                    PreviewMenuHit::Row(index)
                },
            ));
        }
    }
    contains(layout.frame, x, y).then_some(None)
}

/// The switcher as one overlay layer.
#[must_use]
/// **No `measure` arm.** It had one until P1, for exactly one purpose: to ask
/// the font how wide `●` was. The dot is geometry now, so the whole switcher
/// is laid out and painted without a typeface having an opinion about any of
/// its boxes.
pub fn preview_menu_build(
    layout: &PreviewMenuLayout,
    items: &[PreviewMenuItem],
    hover: Option<PreviewMenuHit>,
    favicons: &crate::favicon::Favicons,
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

    if let Some(band) = layout.pinned_label {
        labels.push(section_label(pinned_section_label(), band, scale, palette));
    }
    if let Some(rule) = layout.pinned_separator {
        quads.push(OverlayQuad {
            rect: rule,
            color: palette.menu_border,
            alpha: separator_alpha(palette.menu_border),
        });
    }

    let dot_width = preview_menu_dirty_claim(scale) - px(ITEM_GAP_LOGICAL_PX);
    for (index, (item, rect)) in items.iter().zip(&layout.items).enumerate() {
        push_row(
            &Row {
                rect: *rect,
                // **A page wears the web class's globe and a file wears
                // `#i-file`, in the same box** (`docs/DESIGN.md` §7.7 ⑤), through
                // the one door every preview row asks — **and the site's own
                // icon in that same box where the session has one.** This is the
                // surface the favicon slice, `docs/DESIGN.md` §7.13 was called for: a switcher holding
                // six pages of six servers could until now only be read.
                mark: Some(crate::marks::preview_row_mark(
                    item.is_page(),
                    item.page_url().and_then(|url| favicons.of_url(url)),
                )),
                name: &item.name,
                // Reserved on every row and inked on the dirty ones. Drawn as an
                // empty string rather than omitted so the name's box ends in the
                // same place down the whole list — see the reservation in
                // [`preview_menu_layout`].
                // The words are empty and the reservation is not: the name's
                // box has to end in the same place down the whole list, dot or
                // no dot — see the reservation in [`preview_menu_layout`].
                hint: Some((String::new(), dot_width)),
                // No row of this menu is a row of the shortcut table (系统性发现 ②).
                accel: None,
                dirty: item.dirty,
                // `.tm-item.cur { background: var(--hover) }` is the same fill
                // the pointer draws, which is what the mock-up asks for: the row
                // you are on and the row you are pointing at look alike, and when
                // they are the same row there is nothing to reconcile.
                hovered: hover.map(PreviewMenuHit::row) == Some(index) || item.current,
                available: true,
                // Only a row that names a file on a disk. A diff and a commit's
                // reading of a file are documents this window computed, and
                // keeping one would be keeping a question.
                pin: item.keep.as_ref().map(|_| RowPin {
                    filled: item.pinned,
                    hovered: hover == Some(PreviewMenuHit::Pin(index)),
                    // The *pointer's* row and not `.cur`: the current buffer is
                    // shaded like a hovered row, but nobody is offering to pin
                    // it merely because it is on screen.
                    revealed: hover.map(PreviewMenuHit::row) == Some(index),
                }),
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
    /// **The chord table a menu prints its accelerator column from**
    /// (gesture audit 2026-08-26, 系统性发现 ②).
    ///
    /// The shipped defaults, because that is what these pins are about: the
    /// column a reader who has edited nothing sees. The one pin that is about a
    /// *rebound* chord lays its own overrides on, and says so where it does it.
    fn chord_table() -> crate::shortcuts::Shortcuts {
        crate::shortcuts::Shortcuts::defaults()
    }

    /// The shipped five, as the `const` array this module's cases were written
    /// against.
    ///
    /// A call per case rather than a static: the rows own their strings now
    /// (§7.1.6c-6), so there is no array to borrow. What the cases mean by it is
    /// unchanged — **the table this build ships**, which is exactly what they
    /// were pinning before the user could reorder anything.
    fn shipped_five() -> Vec<Profile> {
        shipped()
    }

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

    /// A file menu's look for a face with nothing runtime in it, with a named
    /// terminal mark so that a test which does not care about the shell still
    /// says which one it was handed.
    fn plain_look(subject: FileMenuSubject) -> FileMenuLook<'static> {
        FileMenuLook {
            subject,
            crumbs: &[],
            terminal: ChromeMark::ProfilePowerShell,
        }
    }

    // ── P130-P137: the preview's filename switcher ──────────────────────────

    fn pool(names: &[(&str, bool, bool)]) -> Vec<PreviewMenuItem> {
        names
            .iter()
            .enumerate()
            .map(|(index, (name, dirty, current))| PreviewMenuItem {
                name: (*name).to_owned(),
                dirty: *dirty,
                current: *current,
                keep: Some(PreviewMenuTarget {
                    kind: bt_persist::PinKind::File,
                    target: format!(r"C:\work\{name}"),
                }),
                pinned: false,
                pool: Some(index),
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
        let layer = one_layer(preview_menu_build(
            &layout,
            &items,
            None,
            &crate::favicon::Favicons::default(),
        ));
        for (name, _, _) in [("a.txt", 0, 0), ("notes.md", 0, 0), ("main.rs", 0, 0)] {
            assert!(
                layer.labels.iter().any(|label| label.text == name),
                "{name} is in the inventory"
            );
        }
        let palette = chrome_palette();
        // **A drawing and not a codepoint** (P1, 字符退役). It was a `●` in the
        // hint slot until this block — the last of the four dirty dots in the
        // window still set in a typeface, two hundred pixels from three that
        // were geometry — so the assertion moved from the labels to the
        // sprites, which is the whole of the change stated as a test.
        // Written as a number and turned into a character here, rather than as
        // an escape: `icons::tests::no_font_character_stands_in_for_a_mark`
        // reads this crate's own source for the escape spelling, and a test
        // that proves a codepoint is gone must not be the reason it is found.
        let dot = char::from_u32(0x25cf).expect("U+25CF is a character");
        assert!(
            !layer.labels.iter().any(|label| label.text.contains(dot)),
            "the switcher's dirty dot is a drawing, not a character",
        );
        let dots: Vec<_> = layer
            .sprites
            .iter()
            .filter(|sprite| {
                matches!(sprite.mark, ChromeMark::ControlPill { .. })
                    && sprite.color == palette.accent
            })
            .collect();
        assert_eq!(dots.len(), 1, "one dot, for the one dirty buffer");

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
                preview_menu_hit(&layout, &items, f64::from(x), f64::from(y)),
                Some(Some(PreviewMenuHit::Row(index)))
            );
        }
        assert_eq!(
            preview_menu_hit(
                &layout,
                &items,
                f64::from(layout.frame[0] + 1.0),
                f64::from(layout.frame[1] + 1.0)
            ),
            Some(None)
        );
        assert_eq!(preview_menu_hit(&layout, &items, 1.0, 1.0), None);
    }

    /// PIN — the kept files are a section of their own at the top of the
    /// switcher, and a kept file that is also open appears **once** (user ruling
    /// 2026-08-19).
    ///
    /// MUTATIONS:
    /// ① list the kept files and then the pool — the open one is drawn twice,
    ///    which is the "PINNED 与最近列表不留双副本" the ruling forbids by name;
    /// ② give a computed document a pin — a diff is a question this window
    ///    asked, and keeping it across a restart would keep the question;
    /// ③ reveal an unkept pin on the *current* row — the buffer on screen is
    ///    shaded like a hovered one, and its pin would appear with nobody
    ///    pointing at it.
    #[test]
    fn the_kept_files_are_a_section_at_the_top_of_the_switcher_and_appear_once() {
        let mut items = pool(&[("a.txt", true, false), ("notes.md", false, true)]);
        // One kept file that is open, one kept file that is not, and a computed
        // document that cannot be kept at all.
        items.insert(
            0,
            PreviewMenuItem {
                name: "a.txt".to_owned(),
                dirty: true,
                current: false,
                keep: Some(PreviewMenuTarget {
                    kind: bt_persist::PinKind::File,
                    target: r"C:\work\a.txt".to_owned(),
                }),
                pinned: true,
                pool: Some(0),
            },
        );
        items.remove(1);
        items.insert(
            1,
            PreviewMenuItem {
                name: "old.rs".to_owned(),
                dirty: false,
                current: false,
                keep: Some(PreviewMenuTarget {
                    kind: bt_persist::PinKind::File,
                    target: r"C:\work\old.rs".to_owned(),
                }),
                pinned: true,
                pool: None,
            },
        );
        items.push(PreviewMenuItem {
            name: "main.rs (working tree)".to_owned(),
            dirty: false,
            current: false,
            keep: None,
            pinned: false,
            pool: Some(9),
        });
        assert_eq!(
            items
                .iter()
                .filter(|item| item.keep.as_ref().map(|keep| keep.target.as_str())
                    == Some(r"C:\work\a.txt"))
                .count(),
            1,
            "a kept file that is also open is one row, not two"
        );

        let layout = preview_menu_layout(
            [40.0, 8.0, 140.0, 27.0],
            (960.0, 600.0),
            1.0,
            &items,
            &mut fake_measure,
        );
        let heading = layout.pinned_label.expect("the kept rows have a heading");
        let rule = layout
            .pinned_separator
            .expect("and a rule between them and the buffers below");
        assert!(heading[3] <= layout.items[0][1]);
        assert!(rule[1] >= layout.items[1][3]);
        assert!(layout.items[2][1] >= rule[3]);
        assert!(
            layout.items.last().expect("rows")[3] <= layout.frame[3],
            "the box grew to hold the section it gained"
        );

        let layer = one_layer(preview_menu_build(
            &layout,
            &items,
            None,
            &crate::favicon::Favicons::default(),
        ));
        assert!(
            layer
                .labels
                .iter()
                .any(|label| label.text == pinned_section_label())
        );
        assert_eq!(
            layer
                .sprites
                .iter()
                .filter(|sprite| sprite.mark == ChromeMark::Pin { filled: true })
                .count(),
            2
        );

        // A computed document's row is a row all the way to its trailing edge:
        // there is no pin there to press.
        let diff = layout.items[3];
        let y = f64::from((diff[1] + diff[3]) / 2.0);
        let pin = row_pin_rect(diff, layout.scale);
        assert_eq!(
            preview_menu_hit(&layout, &items, f64::from((pin[0] + pin[2]) / 2.0), y),
            Some(Some(PreviewMenuHit::Row(3)))
        );
        // A file's row does have one, and it answers for its own box.
        let kept = layout.items[0];
        let y = f64::from((kept[1] + kept[3]) / 2.0);
        let pin = row_pin_rect(kept, layout.scale);
        assert_eq!(
            preview_menu_hit(&layout, &items, f64::from((pin[0] + pin[2]) / 2.0), y),
            Some(Some(PreviewMenuHit::Pin(0)))
        );
        assert_eq!(
            preview_menu_hit(&layout, &items, f64::from(pin[0] - 2.0), y),
            Some(Some(PreviewMenuHit::Row(0)))
        );

        // `.cur` shades a row but offers nothing: the pin follows the pointer.
        let current_row = items
            .iter()
            .position(|item| item.current)
            .expect("one buffer is on screen");
        let resting = one_layer(preview_menu_build(
            &layout,
            &items,
            None,
            &crate::favicon::Favicons::default(),
        ));
        assert_eq!(
            resting
                .sprites
                .iter()
                .filter(|sprite| sprite.mark == ChromeMark::Pin { filled: false })
                .count(),
            0,
            "nobody is pointing at anything, so nothing is offered"
        );
        let pointed = one_layer(preview_menu_build(
            &layout,
            &items,
            Some(PreviewMenuHit::Row(current_row)),
            &crate::favicon::Favicons::default(),
        ));
        assert_eq!(
            pointed
                .sprites
                .iter()
                .filter(|sprite| sprite.mark == ChromeMark::Pin { filled: false })
                .count(),
            1
        );

        // **Keep every row and the rule goes with the section it divided.** One
        // press away in a switcher of two, and a hairline along the bottom of
        // the box is a boundary between one thing and no things.
        let all_kept: Vec<PreviewMenuItem> = items
            .iter()
            .filter(|item| item.keep.is_some())
            .map(|item| PreviewMenuItem {
                pinned: true,
                ..item.clone()
            })
            .collect();
        let all_kept_layout = preview_menu_layout(
            [40.0, 8.0, 140.0, 27.0],
            (960.0, 600.0),
            1.0,
            &all_kept,
            &mut fake_measure,
        );
        assert!(all_kept_layout.pinned_label.is_some());
        assert_eq!(all_kept_layout.pinned_separator, None);
        assert!(
            all_kept_layout.items.last().expect("rows")[3] <= all_kept_layout.frame[3],
            "and the box shrank with it rather than keeping the room"
        );
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
    ///
    /// **The third pane here is `one`'s seat number on another tab** (§7.12 ⓑ).
    /// MUTATION: key this state by `leaf.seat` and that press *shuts* the menu
    /// instead of moving it, because the two panes are the same pane to a bare
    /// number — which on the glass is the switcher vanishing when you open it on
    /// the second of two tabs that happen to have numbered their preview panes
    /// alike.
    #[test]
    fn the_switcher_belongs_to_one_pane_and_its_own_button_shuts_it() {
        let mut menu = PreviewMenu::default();
        let one = LeafId {
            tab: crate::TabId(1),
            seat: bt_layout::SeatId(1),
        };
        let two = LeafId {
            tab: crate::TabId(1),
            seat: bt_layout::SeatId(2),
        };
        let one_next_door = LeafId {
            tab: crate::TabId(2),
            seat: bt_layout::SeatId(1),
        };
        assert_eq!(menu.leaf(), None);
        menu.toggle(one);
        assert_eq!(menu.leaf(), Some(one));
        menu.toggle(one);
        assert_eq!(menu.leaf(), None, "the same button shuts it");
        menu.toggle(one);
        menu.toggle(two);
        assert_eq!(menu.leaf(), Some(two), "and another pane's takes it over");
        menu.toggle(one);
        menu.toggle(one_next_door);
        assert_eq!(
            menu.leaf(),
            Some(one_next_door),
            "a pane on another tab is another pane, however the seats are \
             numbered — the same number in two tabs is two switchers"
        );
        assert!(menu.set_hover(Some(PreviewMenuHit::Row(1))));
        assert_eq!(menu.hover(), Some(PreviewMenuHit::Row(1)));
        assert!(menu.close());
        assert_eq!(menu.hover(), None, "a shut menu is over nothing");
        assert!(!menu.close(), "and closing it again is not a change");
        assert!(!menu.set_hover(Some(PreviewMenuHit::Row(0))));
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
                    notes: RootNotes::of(RootNote::Home),
                    pinned: false
                },
                RootChoice {
                    path: r"D:\repos\api".to_owned(),
                    notes: RootNotes::of(RootNote::Terminal),
                    pinned: false
                },
                RootChoice {
                    path: r"C:\work".to_owned(),
                    notes: RootNotes::of(RootNote::Terminal).and(RootNote::Parent),
                    pinned: false
                },
            ],
            "the parent is already on the list as a terminal, and wears both badges"
        );

        // With no shell standing in it, the parent is offered as the parent.
        let choices = root_choices(r"C:\work\project", Some(r"C:\Users\dev"), &[]);
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[1].path, r"C:\work");
        assert_eq!(choices[1].notes, RootNotes::of(RootNote::Parent));
    }

    /// PIN — a folder with two reasons to be on the list is **one row wearing
    /// both badges** (user report, 2026-08-19).
    ///
    /// The bug this pins: the parent of the root, when a shell happened to be
    /// standing in it, was folded into the terminal row and lost its `parent`
    /// badge — so the one row every user looks for, the way *up*, was on the
    /// menu and unrecognisable. Keeping the first note was the whole of it.
    #[test]
    fn one_folder_is_one_row_wearing_every_badge_it_has_earned() {
        let choices = root_choices(
            r"C:\Users\dev\work",
            Some(r"C:\Users\dev"),
            &[r"C:\Users\dev".to_owned()],
        );
        assert_eq!(choices.len(), 1);
        // Home, a shell is standing in it, *and* it is the folder above — all
        // three, on one row, in the list's own permanent-to-local order.
        assert_eq!(
            choices[0].notes,
            RootNotes::of(RootNote::Home)
                .and(RootNote::Terminal)
                .and(RootNote::Parent)
        );
        assert_eq!(
            choices[0].notes.text(),
            format!(
                "{} · {} · {}",
                RootNote::Home.text(),
                RootNote::Terminal.text(),
                RootNote::Parent.text()
            )
        );

        // The user's own case: the folder above happens to be a shell's folder.
        // The row is where the shell put it — the order does not move — and it
        // says *both* things.
        let choices = root_choices(
            r"C:\work\project",
            None,
            &[r"D:\repos\api".to_owned(), r"C:\work".to_owned()],
        );
        assert_eq!(
            choices
                .iter()
                .map(|choice| choice.path.as_str())
                .collect::<Vec<_>>(),
            vec![r"D:\repos\api", r"C:\work"],
            "merging two reasons does not move the row"
        );
        assert!(choices[1].notes.has(RootNote::Parent));
        assert!(choices[1].notes.has(RootNote::Terminal));
        assert!(!choices[1].notes.has(RootNote::Home));
        assert_eq!(
            choices[1].notes.text(),
            format!(
                "{} · {}",
                RootNote::Terminal.text(),
                RootNote::Parent.text()
            )
        );

        // One reason is one badge and no punctuation.
        assert_eq!(
            choices[0].notes.text(),
            RootNote::Terminal.text(),
            "a row with one reason wears one badge and no joiner"
        );

        // Every badge the menu could ever print is a badge the width was
        // reserved for, which is what keeps the popup from moving under the
        // pointer when a shell moves.
        let every: Vec<RootNotes> = RootNotes::every().collect();
        assert_eq!(every.len(), 7, "three reasons, every non-empty combination");
        for notes in every {
            assert!(!notes.text().is_empty());
        }
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
        // The row's *left* half, deliberately: its right end is the pin's now,
        // and a press there is a different verb (2026-08-19).
        let (_, y) = middle(first.1);
        let x = first.1[0] + 4.0;
        assert_eq!(
            root_menu_hit(&layout, f64::from(x), f64::from(y)),
            Some(Some(RootMenuHit::Row(RootMenuRow::Choice(0))))
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
            Some(Some(RootMenuHit::Row(RootMenuRow::Browse)))
        );
        // And its trailing edge is the row's too: `Browse…` has no pin, so
        // pressing where every other row's pin sits still says `Browse…`.
        let pin_x = layout.browse[2] - 4.0;
        assert_eq!(
            root_menu_hit(&layout, f64::from(pin_x), f64::from(y)),
            Some(Some(RootMenuHit::Row(RootMenuRow::Browse)))
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
            Some(RootMenuHit::Row(RootMenuRow::Browse)),
            &mut fake_measure,
        ));
        assert!(
            layer.labels.iter().any(|label| label.text == browse_text()),
            "the row says its own name"
        );
    }

    /// **P2's fill policy, read off the menus the user actually opens**, and
    /// restored on 2026-08-27 after a day with the opposite ruling in it.
    ///
    /// RED EVIDENCE (2026-08-26, on the real window): the registry gate went
    /// green while the profile menu still drew two solid accent-blue folders
    /// under its rule, because those two rows named `ChromeMark::Folder`
    /// themselves instead of asking. A table gate is worth what its drawing
    /// points are worth, so this one asks the *drawings*:
    ///
    /// * **The profile menu names no place at all** — every row is an act, so
    ///   there is no solid folder anywhere in it.
    /// * **The root menu names both**, and the division is the whole policy: a
    ///   cwd row *is* a folder and wears the solid one, `Browse…` is a question
    ///   and wears the struck one.
    ///
    /// **This assertion was inverted for one day and is back** (user ruling,
    /// 2026-08-27): a first acceptance on 2026-08-26 read the solid folder as
    /// the nicer drawing and the struck pair was deleted, and the second looked
    /// at the same menu and said why that is wrong —「这个菜单里所有的都
    /// 是描边的,突然出现一个实心就会怪怪的」. The method never changed;
    /// what changed is which way it points.
    ///
    /// MUTATION: point either profile-menu row back at `ChromeMark::Folder` and
    /// the first half goes red naming the row's rectangle; point `Browse…` back
    /// at `ChromeMark::FolderOpen` and the second half goes red.
    #[test]
    fn a_folder_in_a_column_of_verbs_is_struck_and_a_folder_that_is_a_place_is_solid() {
        let scale = 1.0;
        let vault = [term(r"C:\work", None, 3_600)];
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            &vault,
            &chord_table(),
            &mut fake_measure,
        );
        let layer = one_layer(build(
            &layout,
            &equipped(),
            0,
            None,
            &vault,
            now(),
            &crate::favicon::Favicons::default(),
            &mut fake_measure,
        ));
        let solid: Vec<[f32; 4]> = layer
            .sprites
            .iter()
            .filter(|sprite| matches!(sprite.mark, ChromeMark::Folder | ChromeMark::FolderOpen))
            .map(|sprite| sprite.rect)
            .collect();
        assert!(
            solid.is_empty(),
            "every row of the profile menu is an act, so none of them wears the \
             object's solid folder: {solid:?}",
        );
        assert_eq!(
            layer
                .sprites
                .iter()
                .filter(|sprite| sprite.mark == ChromeMark::FolderOutline)
                .count(),
            2,
            "`Files pane` and `New terminal in folder…` wear the struck one",
        );

        // And the menu that holds both kinds of row.
        let choices = root_choices(r"C:\work\project", None, &[]);
        let root = root_menu_layout(
            [40.0, 8.0, 140.0, 27.0],
            (960.0, 600.0),
            1.0,
            &choices,
            &mut fake_measure,
        );
        let root_layer = one_layer(root_menu_build(
            &root,
            &choices,
            r"C:\work\project",
            None,
            &mut fake_measure,
        ));
        let inside = |row: [f32; 4], sprite: &ChromeSprite| {
            sprite.rect[1] >= row[1] && sprite.rect[3] <= row[3]
        };
        let browse: Vec<ChromeMark> = root_layer
            .sprites
            .iter()
            .filter(|sprite| inside(root.browse, sprite))
            .map(|sprite| sprite.mark)
            .collect();
        assert_eq!(
            browse,
            vec![ChromeMark::FolderOpenOutline],
            "`Browse…` is a question, and a question is struck",
        );
        assert!(
            root_layer.sprites.iter().any(|sprite| {
                root.items.iter().any(|item| inside(*item, sprite))
                    && matches!(sprite.mark, ChromeMark::Folder | ChromeMark::FolderOpen)
            }),
            "and a row that names a place keeps the object's solid folder",
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

    /// PIN — the kept folders are a section of their own, **above home**, with
    /// one hairline between it and the list below (user ruling 2026-08-19).
    ///
    /// MUTATIONS:
    /// ① append the kept rows instead of lifting them — the folder appears
    ///    twice, once as a pin and once as the place this window found;
    /// ② draw the `PINNED` heading whether or not anything is kept — a heading
    ///    over nothing, which the reader has to work out is empty;
    /// ③ leave the hairline out — the two sections read as one list with a
    ///    label stuck in the middle of it.
    #[test]
    fn the_kept_folders_are_a_section_above_home_with_a_rule_under_them() {
        let found = root_choices(
            r"C:\work\project",
            Some(r"C:\Users\dev"),
            &[r"D:\repos\api".to_owned()],
        );
        // One kept folder this window also found, and one it did not.
        let choices = apply_pins(
            found,
            &[r"D:\repos\api".to_owned(), r"Z:\archive".to_owned()],
        );
        assert_eq!(
            choices
                .iter()
                .map(|choice| (choice.path.as_str(), choice.pinned))
                .collect::<Vec<_>>(),
            vec![
                (r"D:\repos\api", true),
                (r"Z:\archive", true),
                (r"C:\Users\dev", false),
                (r"C:\work", false),
            ],
            "kept first, in the file's order, then what this window found"
        );
        assert!(
            choices[0].notes.has(RootNote::Terminal),
            "a kept row that was also found keeps every badge it earned"
        );
        assert_eq!(
            choices[1].notes,
            RootNotes::default(),
            "and one that was not found is offered because you kept it, and says nothing else"
        );

        let layout = root_menu_layout(
            [40.0, 8.0, 140.0, 27.0],
            (960.0, 600.0),
            1.0,
            &choices,
            &mut fake_measure,
        );
        let heading = layout.pinned_label.expect("the kept rows have a heading");
        let rule = layout
            .pinned_separator
            .expect("and a rule between them and the list below");
        assert!(
            heading[3] <= layout.items[0][1],
            "the heading is above them"
        );
        assert!(
            rule[1] >= layout.items[1][3],
            "the rule is under the last of them"
        );
        let found = layout
            .label
            .expect("the list below still has rows of its own");
        assert!(
            found[1] >= rule[3],
            "and `OPEN FOLDER` sits under the rule, so home is below the kept rows"
        );
        assert!(layout.items[2][1] >= found[3]);
        assert!(
            layout.browse[3] <= layout.frame[3],
            "the box grew to hold the section it gained"
        );

        // **A heading over nothing goes too**, which is the PINNED section's own
        // rule read for the other one: pinning every place this window found
        // leaves `OPEN FOLDER` standing over a hairline.
        let all_kept = apply_pins(
            root_choices(r"C:\work\project", Some(r"C:\Users\dev"), &[]),
            &[r"C:\Users\dev".to_owned(), r"C:\work".to_owned()],
        );
        let all_kept_layout = root_menu_layout(
            [40.0, 8.0, 140.0, 27.0],
            (960.0, 600.0),
            1.0,
            &all_kept,
            &mut fake_measure,
        );
        assert!(all_kept_layout.pinned_label.is_some());
        assert_eq!(
            all_kept_layout.label, None,
            "no places left to head, so no heading"
        );
        assert_eq!(
            all_kept_layout.pinned_separator, None,
            "and no rule either: it would sit straight on `Browse…`'s own"
        );
        assert!(all_kept_layout.browse[3] <= all_kept_layout.frame[3]);
        assert!(
            !one_layer(root_menu_build(
                &all_kept_layout,
                &all_kept,
                r"C:\work\project",
                None,
                &mut fake_measure,
            ))
            .labels
            .iter()
            .any(|label| label.text == root_section_label())
        );

        let layer = one_layer(root_menu_build(
            &layout,
            &choices,
            r"C:\work\project",
            None,
            &mut fake_measure,
        ));
        assert!(
            layer
                .labels
                .iter()
                .any(|label| label.text == pinned_section_label()),
            "the section says its own name"
        );
        assert_eq!(
            layer
                .sprites
                .iter()
                .filter(|sprite| sprite.mark == ChromeMark::Pin { filled: true })
                .count(),
            2,
            "each kept row wears a filled pin, whether or not anybody is pointing at it"
        );
        assert_eq!(
            layer
                .sprites
                .iter()
                .filter(|sprite| sprite.mark == ChromeMark::Pin { filled: false })
                .count(),
            0,
            "and an unkept row's pin is an offer, so it waits for the hand"
        );

        // Nothing kept is the menu exactly as it was: no heading, no rule.
        let plain = apply_pins(
            root_choices(r"C:\work\project", Some(r"C:\Users\dev"), &[]),
            &[],
        );
        let plain_layout = root_menu_layout(
            [40.0, 8.0, 140.0, 27.0],
            (960.0, 600.0),
            1.0,
            &plain,
            &mut fake_measure,
        );
        assert_eq!(plain_layout.pinned_label, None);
        assert_eq!(plain_layout.pinned_separator, None);
    }

    /// PIN — the pin is a second verb on the row, told apart by the rectangle it
    /// is in and not by how far right the press landed.
    #[test]
    fn a_root_row_carries_a_pin_that_answers_for_its_own_box() {
        let choices = apply_pins(
            root_choices(r"C:\work\project", Some(r"C:\Users\dev"), &[]),
            &[],
        );
        let layout = root_menu_layout(
            [40.0, 8.0, 140.0, 27.0],
            (960.0, 600.0),
            1.0,
            &choices,
            &mut fake_measure,
        );
        let item = layout.items[0];
        let y = f64::from((item[1] + item[3]) / 2.0);
        let pin = row_pin_rect(item, layout.scale);
        assert_eq!(
            root_menu_hit(&layout, f64::from((pin[0] + pin[2]) / 2.0), y),
            Some(Some(RootMenuHit::Pin(RootMenuRow::Choice(0))))
        );
        assert_eq!(
            root_menu_hit(&layout, f64::from(pin[0] - 2.0), y),
            Some(Some(RootMenuHit::Row(RootMenuRow::Choice(0)))),
            "one pixel outside the pin is the row again"
        );
        assert!(
            pin[2] <= item[2],
            "and the pin lives inside the row it belongs to"
        );

        // Hovering the row reveals the offer; hovering the pin lights it.
        let revealed = one_layer(root_menu_build(
            &layout,
            &choices,
            r"C:\work\project",
            Some(RootMenuHit::Row(RootMenuRow::Choice(0))),
            &mut fake_measure,
        ));
        assert_eq!(
            revealed
                .sprites
                .iter()
                .filter(|sprite| sprite.mark == ChromeMark::Pin { filled: false })
                .count(),
            1,
            "the row under the hand offers its pin, and no other row does"
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
        assert!(menu.set_hover(Some(RootMenuHit::Row(RootMenuRow::Choice(1)))));
        assert_eq!(menu.hover(), Some(RootMenuHit::Row(RootMenuRow::Choice(1))));
        menu.close();
        assert!(
            !menu.set_hover(Some(RootMenuHit::Row(RootMenuRow::Choice(1)))),
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
    /// second, accidental copy of `display_title(…)`, so the 7 / 5.1 rename came
    /// back as six failures in tests that had no opinion about it.
    fn powershell_seven() -> String {
        display_title(index_of_id("pwsh"))
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
        /// would quietly make [`fallback_profile()`] unavailable, which is the one
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
                profile_id: shipped_five()[fallback_profile()].id.to_owned(),
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
    /// The middle section: one rule and **two** rows, always drawn — `Files
    /// pane` and `New terminal in folder…` (user ruling 2026-08-20).
    ///
    /// Named beside [`recent_block`] so the three height pins state the menu's
    /// shape the same way, and so the day this section grows a second row there
    /// is one place to say so. That day was 2026-08-20, and this is the one
    /// place.
    fn files_block(scale: f32) -> f32 {
        separator_block(scale) + 2.0 * (ITEM_HEIGHT_LOGICAL_PX * scale).round()
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
                &chord_table(),
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
            let longest = (0..count())
                .map(|index| fake_measure(title(index), ITEM_FONT_LOGICAL_PX * scale))
                .fold(0.0_f32, f32::max);
            let annotation = fake_measure(hint_text(), HINT_FONT_LOGICAL_PX * scale).max(
                fake_measure(unavailable_hint_text(), HINT_FONT_LOGICAL_PX * scale),
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
            assert_eq!(layout.items.len(), count());
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
            &chord_table(),
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
            &chord_table(),
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
            &chord_table(),
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
            &chord_table(),
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
    /// [`fallback_profile()`] is a constant index too. Reordering this array
    /// silently re-points it.
    ///
    /// This test replaces one that asserted `count() == 1` and was right
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
        assert_eq!(count(), 5);
        let shipped = shipped_five();
        let listed: Vec<_> = shipped.iter().map(|profile| profile.id.as_str()).collect();
        assert_eq!(listed, ["pwsh", "winps", "wsl", "gitbash", "cmd"]);
        assert_eq!(display_title(fallback_profile()), "Windows PowerShell 5.1");

        // **Mark × title, and not the mark alone.** This used to require every
        // mark to be distinct, and the two PowerShells retire that: they are one
        // family and there is one PowerShell symbol in the mock-up, so drawing a
        // second glyph would assert a visual distinction the family does not
        // have. What has to stay unique is what `docs/UI-UX.md` §126-137 says
        // identity *is* — "图标 × 目录", the icon and the text together — and in
        // this list the text is the title. Two rows with the same mark are fine;
        // two rows a reader cannot tell apart are not.
        for (index, left) in shipped_five().iter().enumerate() {
            for right in &shipped_five()[index + 1..] {
                assert_ne!(
                    (left.mark, &left.display_title),
                    (right.mark, &right.display_title),
                    "{} and {} would be one row twice",
                    left.id,
                    right.id
                );
            }
        }
        assert_eq!(
            mark(index_of_id("pwsh")),
            mark(index_of_id("winps")),
            "and the two PowerShells share theirs on purpose"
        );

        // And five ids, because an id is what a seed is keyed on: two profiles
        // sharing one would be two tabs that cannot be told apart on disk.
        let ids: std::collections::HashSet<_> = listed.iter().collect();
        assert_eq!(ids.len(), count());
        for profile in shipped_five() {
            assert_eq!(
                index_of_id(&profile.id),
                shipped_five()
                    .iter()
                    .position(|p| p.id == profile.id)
                    .unwrap(),
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
        for profile in shipped_five() {
            let has_nologo = profile.args.iter().any(|argument| argument == "-NoLogo");
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
        assert_eq!(args(index_of_id("cmd")), &[] as &[&str]);
        assert_eq!(
            args(index_of_id("gitbash")),
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
    /// * point `fallback_profile()` at `pwsh` and the floor under every other
    ///   profile becomes a row that is allowed to be greyed.
    #[test]
    fn the_two_powershells_are_two_rows_and_only_one_of_them_can_be_missing() {
        let (seven, five) = (index_of_id("pwsh"), index_of_id("winps"));
        assert_ne!(seven, five, "two rows, so two indices");
        // Each row says which one it is. The bare "PowerShell" / "Windows
        // PowerShell" pair these shipped with is what the user could not read
        // apart at a glance, and the version is the whole answer.
        assert_eq!(display_title(seven), "PowerShell 7");
        assert_eq!(display_title(five), "Windows PowerShell 5.1");
        for profile in [seven, five] {
            assert!(
                shipped_five()[profile]
                    .display_title
                    .split_whitespace()
                    .any(|word| word.starts_with(|first: char| first.is_ascii_digit())),
                "{:?} names its version, which is the only thing telling it from \
                 the row beside it",
                shipped_five()[profile].id
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
        assert_eq!(five, fallback_profile());

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
            let title = display_title(index_of_id(id));
            let quoted = format!("'{title}'");
            assert!(
                script.contains(&quoted),
                "{id}'s title {title:?} is what the script's {edition} arm writes; \
                 folio.ps1 does not contain {quoted}"
            );
        }
        // And the arms are told apart the way the script tells them apart, so the
        // pair above cannot both be satisfied by one arm carrying both strings.
        let seven = display_title(index_of_id("pwsh"));
        let five = display_title(index_of_id("winps"));
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
            shipped_five()[fallback_profile()].id,
            "winps",
            "the floor is the shell that is part of Windows"
        );
        assert!(
            !matches!(
                shipped_five()[fallback_profile()].program,
                ProgramSource::PowerShellSeven
            ),
            "and never the row that is allowed to answer `no` — a fallback chain              whose bottom can be greyed has a hole in it"
        );
        // Even on a machine with nothing else on it.
        assert!(bare().is_available(fallback_profile()));
        assert!(equipped().is_available(fallback_profile()));
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
            &chord_table(),
            &mut fake_measure,
        );
        for (chosen, profile) in shipped_five().iter().enumerate() {
            let layers = build(
                &layout,
                &equipped(),
                chosen,
                None,
                NO_RECENT,
                now(),
                &crate::favicon::Favicons::default(),
                &mut fake_measure,
            );
            let captioned: Vec<usize> = layout
                .items
                .iter()
                .enumerate()
                .filter(|(_, row)| {
                    layers.iter().flat_map(|layer| &layer.labels).any(|label| {
                        label.text == hint_text()
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
            &chord_table(),
            &mut fake_measure,
        );
        // Every profile as the default in turn, so the longest name carrying the
        // hint is covered rather than only the first row's short one.
        for chosen in 0..count() {
            for programs in [equipped(), bare()] {
                let layer = one_layer(build(
                    &layout,
                    &programs,
                    chosen,
                    None,
                    &vault,
                    now(),
                    &crate::favicon::Favicons::default(),
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
        let vault = [term(r"D:\Developer\folio-terminal\crates", None, 30)];
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            &vault,
            &chord_table(),
            &mut fake_measure,
        );

        let bare_tips: Vec<_> = layout.tips(&bare(), &vault).collect();
        let profiles_tipped: Vec<_> = bare_tips
            .iter()
            .filter_map(|(row, _, text)| match row {
                MenuRow::Profile(index) => Some((*index, text.clone())),
                // The two folder rows' captions say everything they know, so
                // they are never tipped — the same rule an available profile row
                // follows.
                MenuRow::Recent(_) | MenuRow::FilesPane | MenuRow::NewInFolder => None,
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
                MenuRow::NewInFolder => layout.new_in_folder,
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
                r"D:\Developer\folio-terminal\crates".to_owned()
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
    /// installed, and one that only ever answered `fallback_profile()` makes the
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
            fallback_profile(),
            "nobody has ever opened the setting: the floor, not an error"
        );
        assert_eq!(
            default_profile("a-profile-from-a-newer-build", &all),
            fallback_profile(),
            "an id this build does not have degrades exactly as a leaf's does"
        );
        assert_eq!(
            default_profile("gitbash", &bare()),
            fallback_profile(),
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
                    directory: Some(PathBuf::from(r"C:\Users\dev")),
                },
                "{profile} is a Windows process and takes a working directory"
            );
        }
        assert_eq!(
            spawn_place(index_of_id("wsl"), None, &machine),
            SpawnPlace {
                working_directory: None,
                arguments: vec![OsString::from("--cd"), OsString::from("~")],
                // The place itself is still said, on the one channel that can
                // carry it. `~` is where this shell stands, and it is what a leaf
                // that never reported an OSC 7 has to answer with.
                directory: Some(PathBuf::from("~")),
            },
            "WSL's home has no Windows spelling, so it is asked for rather than handed over"
        );
        // The home is read from the variable and never composed, so a redirected
        // profile is followed rather than guessed at.
        assert_eq!(
            spawn_place(
                fallback_profile(),
                None,
                &FakeMachine::default().with_var("USERPROFILE", r"\\server\redirected\dev")
            )
            .working_directory,
            Some(PathBuf::from(r"\\server\redirected\dev")),
        );
        assert_eq!(
            spawn_place(fallback_profile(), None, &FakeMachine::default()),
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
                directory: Some(PathBuf::from(r"D:\Developer")),
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
                directory: Some(PathBuf::from("/mnt/d/Developer")),
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
            (r"C:\Users\Alice", "/mnt/c/Users/Alice"),
            (
                r"D:\Developer\folio-terminal",
                "/mnt/d/Developer/folio-terminal",
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
            ("/mnt/c/Users/Alice", r"C:\Users\Alice"),
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
            r"\\wsl.localhost\Ubuntu-24.04\home\alice",
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
            "/home/alice",
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
        let windows = Path::new(r"D:\Developer\folio-terminal");
        let mounted = Path::new("/mnt/d/Developer/folio-terminal");
        let inside = Path::new("/home/alice/src");
        for (source, profile) in shipped_five().iter().enumerate() {
            for (target, other) in shipped_five().iter().enumerate() {
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
        for inside in ["/home/alice/src", "/mnt/d/Developer"] {
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
        for profile in &shipped_five() {
            assert_eq!(
                compose_title(profile, None),
                profile.display_title,
                "{} is its own title on a machine that answered nothing",
                profile.id
            );
            let qualified = compose_title(profile, Some("Ubuntu-24.04"));
            match profile.qualifier {
                Qualifier::WslDistribution => {
                    assert_eq!(qualified, "WSL · Ubuntu-24.04");
                    // The mock-up's own rule (line 4013): a session's name is
                    // everything before `" ·"`, and it is the constant.
                    assert_eq!(
                        qualified.split(" ·").next(),
                        Some(profile.display_title.as_str())
                    );
                }
                Qualifier::None => assert_eq!(
                    qualified, profile.display_title,
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
            (0..count())
                .filter(|index| none.is_available(*index))
                .collect::<Vec<_>>(),
            vec![fallback_profile()],
            "a bare Windows box offers PowerShell and says the truth about the rest"
        );

        let all = equipped();
        for (index, profile) in shipped_five().iter().enumerate() {
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
            &chord_table(),
            &mut fake_measure,
        );
        assert_eq!(
            layout.items.len(),
            count(),
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
            fallback_profile(),
            None,
            NO_RECENT,
            now(),
            &crate::favicon::Favicons::default(),
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
                .any(|label| label.text == unavailable_hint_text()),
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
        let winps = layer.sprites[fallback_profile()];
        assert_eq!(winps.mark, ChromeMark::ProfilePowerShell);
        assert_eq!(winps.opacity, 1.0);
        assert!(!winps.grayscale);
        // …while PowerShell 7, one row above it, is greyed on this machine.
        let pwsh = layer.sprites[index_of_id("pwsh")];
        assert_eq!(pwsh.mark, ChromeMark::ProfilePowerShell);
        assert_eq!(pwsh.opacity, UNAVAILABLE_MARK_OPACITY);
        assert!(pwsh.grayscale);
        assert!(
            layer.labels.iter().any(|label| label.text == hint_text()),
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
            &chord_table(),
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
            fallback_profile(),
            None,
            &vault,
            now(),
            &crate::favicon::Favicons::default(),
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
            &chord_table(),
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
            &chord_table(),
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
            &chord_table(),
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
            &chord_table(),
            &mut fake_measure,
        );
        let palette = chrome_palette();
        let rest = one_layer(build(
            &layout,
            &equipped(),
            fallback_profile(),
            None,
            NO_RECENT,
            now(),
            &crate::favicon::Favicons::default(),
            &mut fake_measure,
        ));
        let hover = one_layer(build(
            &layout,
            &equipped(),
            fallback_profile(),
            Some(MenuRow::Profile(0)),
            NO_RECENT,
            now(),
            &crate::favicon::Favicons::default(),
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
            rest_labels.iter().any(|label| label.text == hint_text()),
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
            recent_section_label(),
            "RECENTLY OPENED",
            "`Recently opened` under `text-transform: uppercase`"
        );

        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let layout = layout(
                anchor(scale),
                MenuSide::Below,
                (960.0 * scale, 600.0),
                scale,
                NO_RECENT,
                &chord_table(),
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
            &chord_table(),
            &mut fake_measure,
        );
        let palette = chrome_palette();
        let layers = build(
            &layout,
            &equipped(),
            fallback_profile(),
            None,
            NO_RECENT,
            now(),
            &crate::favicon::Favicons::default(),
            &mut fake_measure,
        );
        let labels: Vec<_> = layers.iter().flat_map(|layer| &layer.labels).collect();
        let sprites: Vec<_> = layers.iter().flat_map(|layer| &layer.sprites).collect();
        let hint = labels
            .iter()
            .find(|label| label.text == hint_text())
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
        assert_eq!(
            mark.rect[2] - mark.rect[0],
            (item_mark_box_logical_px(ChromeMark::ProfilePowerShell)[0] * scale).round(),
        );
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
                &chord_table(),
                &mut fake_measure,
            );
            let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
            assert_eq!(
                layout.frame[3] - layout.frame[1],
                (2.0 * (border + MENU_PADDING_LOGICAL_PX * scale)
                    + (ITEM_HEIGHT_LOGICAL_PX * scale).round() * count() as f32
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
                fallback_profile(),
                None,
                NO_RECENT,
                now(),
                &crate::favicon::Favicons::default(),
                &mut fake_measure,
            ));
            assert!(
                !layer
                    .labels
                    .iter()
                    .any(|label| label.text == recent_section_label()),
                "scale {scale}: no heading over an empty list"
            );
            assert_eq!(
                layer.sprites.len(),
                count() + 2,
                "scale {scale}: one mark per profile row, plus a folder on each of \
                 the middle section's two rows"
            );
        }
    }

    /// PIN — the middle section is one rule and **two** rows, and the second of
    /// them is `New terminal in folder…` (user ruling 2026-08-20).
    ///
    /// Red gate: the section's height is written once, as `files_block`, and the
    /// rows are laid down against a cursor that advances by it. A row added to
    /// the list without the block being told is a row drawn on top of whatever
    /// comes next — the Recent rule under an empty vault, the menu's own bottom
    /// padding otherwise — and it hit-tests over the top of it too. So the
    /// arithmetic is asserted rather than the rectangle alone: that the new row
    /// follows the old one exactly, and that everything below it moved down by
    /// exactly one row.
    #[test]
    fn the_middle_section_is_a_rule_and_two_folder_rows() {
        let vault = [term("C:\\repo", None, 0)];
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let item_height = (ITEM_HEIGHT_LOGICAL_PX * scale).round();
            let empty = layout(
                anchor(scale),
                MenuSide::Below,
                (960.0 * scale, 600.0),
                scale,
                NO_RECENT,
                &chord_table(),
                &mut fake_measure,
            );

            // Immediately under `Files pane`, on its edges: two rows of one
            // section, not a row and an afterthought.
            assert_eq!(
                empty.new_in_folder[1], empty.files_pane[3],
                "scale {scale}: the second row follows the first with no gap"
            );
            assert_eq!(empty.new_in_folder[0], empty.files_pane[0]);
            assert_eq!(empty.new_in_folder[2], empty.files_pane[2]);
            assert_eq!(
                empty.new_in_folder[3] - empty.new_in_folder[1],
                item_height,
                "scale {scale}: and it is a `.profile-item` like every other row"
            );

            // Nothing is drawn over: under an empty vault the new row is the last
            // thing in the menu, and the menu's own bottom padding follows it.
            let chrome =
                (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0) + MENU_PADDING_LOGICAL_PX * scale;
            let gap = empty.frame[3] - empty.new_in_folder[3];
            assert!(
                (gap - chrome).abs() <= 0.5,
                "scale {scale}: the menu grew by the row rather than covering it \
                 — {gap} of trailing chrome against {chrome} (the frame's own \
                 height is rounded to whole device pixels, so half a pixel of \
                 that rounding lands here)"
            );

            // And with a vault, everything below moved down by exactly one row:
            // the Recent rule stands its own margin below the new one.
            // [`the_recent_section_is_a_rule_a_heading_and_one_row_for_each_seed`]
            // states that offset from the other side of the same gap.
            let full = layout(
                anchor(scale),
                MenuSide::Below,
                (960.0 * scale, 600.0),
                scale,
                &vault,
                &chord_table(),
                &mut fake_measure,
            );
            let rule = full.separator.expect("a filled vault is separated");
            assert!(
                rule[1] > full.new_in_folder[3],
                "scale {scale}: the Recent rule is below the row, not through it"
            );

            // The caption decides the width alongside the profile rows, because
            // it is one of this module's own strings — and in both languages it
            // is the longest row in the menu.
            let px = |value: f32| value * scale;
            let needed = 2.0
                * ((FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0)
                    + px(MENU_PADDING_LOGICAL_PX)
                    + px(ITEM_PADDING_X_LOGICAL_PX))
                + px(ITEM_ICON_COLUMN_LOGICAL_PX)
                + px(ITEM_GAP_LOGICAL_PX)
                + fake_measure(new_in_folder_text(), px(ITEM_FONT_LOGICAL_PX))
                + px(ITEM_GAP_LOGICAL_PX);
            assert!(
                empty.frame[2] - empty.frame[0] >= needed,
                "scale {scale}: the menu makes room for its own longest caption"
            );
        }
    }

    /// PIN — a press on the folder row is that row, and never the row above it.
    ///
    /// Red gate: a rectangle the layout hands out and the hit test does not read
    /// is a row that lights nothing under the pointer and does nothing when
    /// pressed — the menu's own `hit` doc calls the half-written version of this
    /// "a menu lying about what it is about to do".
    #[test]
    fn a_press_on_the_folder_row_opens_the_chooser_and_never_the_files_column() {
        let scale = 1.0;
        let vault = [term("C:\\repo", None, 0)];
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            &vault,
            &chord_table(),
            &mut fake_measure,
        );
        let centre = |rect: [f32; 4]| {
            (
                f64::from((rect[0] + rect[2]) / 2.0),
                f64::from((rect[1] + rect[3]) / 2.0),
            )
        };
        let (x, y) = centre(layout.new_in_folder);
        assert_eq!(
            hit(&layout, &equipped(), &vault, x, y),
            Some(Some(MenuRow::NewInFolder)),
        );
        let (x, y) = centre(layout.files_pane);
        assert_eq!(
            hit(&layout, &equipped(), &vault, x, y),
            Some(Some(MenuRow::FilesPane)),
            "the row above still answers for itself",
        );
        // A machine with only Windows PowerShell on it changes nothing here:
        // this row starts no shell of its own, so there is no program for it to
        // be missing and no greyed state to reach.
        let (x, y) = centre(layout.new_in_folder);
        assert_eq!(
            hit(&layout, &bare(), &vault, x, y),
            Some(Some(MenuRow::NewInFolder)),
        );
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
                &chord_table(),
                &mut fake_measure,
            );
            let full = layout(
                anchor(scale),
                MenuSide::Below,
                (960.0 * scale, 600.0),
                scale,
                &vault,
                &chord_table(),
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
            // The Recent rule follows the middle section's **last** row rather
            // than the last profile: the menu has three sections, and this one is
            // third. Its last row has been `New terminal in folder…` since
            // 2026-08-20; measuring from `files_pane` was right while that
            // section held one row and is a rule drawn through a row now.
            assert_eq!(
                rule[1] - full.new_in_folder[3],
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
    /// straight into [`shipped_five()`]. With a Recent section under them that number
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
            &chord_table(),
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
            &chord_table(),
            &mut fake_measure,
        );
        assert_eq!(RECENT_CAPACITY, 8, "the vault's own cap, not a second one");
        assert_eq!(layout.recent.len(), RECENT_CAPACITY);
        assert_eq!(
            layout.frame[3] - layout.frame[1],
            (2.0 * ((FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0)
                + MENU_PADDING_LOGICAL_PX * scale)
                + (ITEM_HEIGHT_LOGICAL_PX * scale).round() * count() as f32
                + files_block(scale)
                + recent_block(scale, RECENT_CAPACITY))
            .round(),
            "and the menu is only as tall as the rows it draws"
        );

        let layer = one_layer(build(
            &layout,
            &equipped(),
            fallback_profile(),
            None,
            &vault,
            now(),
            &crate::favicon::Favicons::default(),
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
            count() + 2 + RECENT_CAPACITY,
            "one mark per drawn row, the middle section's two included"
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
            &chord_table(),
            &mut fake_measure,
        );
        let palette = chrome_palette();
        let layer = one_layer(build(
            &layout,
            &equipped(),
            fallback_profile(),
            None,
            &vault,
            now(),
            &crate::favicon::Favicons::default(),
            &mut fake_measure,
        ));
        let heading = layer
            .labels
            .iter()
            .find(|label| label.text == recent_section_label())
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
        assert_eq!(cwd_leaf("C:\\Users\\Alice\\repo"), "repo");
        assert_eq!(cwd_leaf("C:\\Users\\Alice\\repo\\"), "repo");
        assert_eq!(cwd_leaf("C:\\"), "C:", "a drive root names its drive");
        assert_eq!(cwd_leaf("C:"), "C:");
        assert_eq!(
            cwd_leaf("/home/alice/src"),
            "src",
            "and forward slashes too"
        );

        let vault = [
            term("C:\\Users\\Alice\\repo", Some("build"), 0),
            term("C:\\Users\\Alice\\notes", None, 60),
            // `||` in the mock-up falls through an empty string: a row captioned
            // with nothing is a row you cannot tell from the one above it.
            term("C:\\Users\\Alice\\empty", Some(""), 120),
            files("D:\\Developer\\folio-terminal\\", 180),
        ];
        let layout = layout(
            anchor(1.0),
            MenuSide::Below,
            (960.0, 600.0),
            1.0,
            &vault,
            &chord_table(),
            &mut fake_measure,
        );
        let layer = one_layer(build(
            &layout,
            &equipped(),
            fallback_profile(),
            None,
            &vault,
            now(),
            &crate::favicon::Favicons::default(),
            &mut fake_measure,
        ));
        let drawn: Vec<&str> = layer
            .labels
            .iter()
            .map(|label| label.text.as_str())
            .collect();
        for name in ["build", "notes", "empty", "folio-terminal"] {
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
            &chord_table(),
            &mut fake_measure,
        );
        let palette = chrome_palette();
        let layer = one_layer(build(
            &layout,
            &equipped(),
            fallback_profile(),
            None,
            &vault,
            now(),
            &crate::favicon::Favicons::default(),
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
        assert_eq!(shell.mark, mark(fallback_profile()));
        assert_eq!(shell.color, palette.accent);
        // An id this build does not have costs the row its shell choice, never
        // its mark — `index_of_id` falls back rather than refusing.
        assert_eq!(
            recent_mark(
                &Seed::Term {
                    profile_id: "a-shell-from-a-newer-build".to_owned(),
                    cwd: "C:\\repo".to_owned(),
                    manual_name: None,
                },
                &crate::favicon::Favicons::default(),
            ),
            mark(fallback_profile())
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
            &chord_table(),
            &mut fake_measure,
        );
        let palette = chrome_palette();
        let layer = one_layer(build(
            &layout,
            &equipped(),
            fallback_profile(),
            Some(MenuRow::Recent(0)),
            &vault,
            now(),
            &crate::favicon::Favicons::default(),
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

    /// PIN — **a file row's menu is its doors, the rule, and its three path
    /// verbs** (user rulings 2026-08-24 and 2026-08-25).
    ///
    /// `DESIGN.md` §7.1.3's three are still here in their order. What the two
    /// rulings added, and what this pin holds together, is that the *ways in*
    /// half grew — a second door out of the row (`Open with default app`) beside
    /// the preview — while the path half grew once (`Reveal in Explorer`). The
    /// rule still divides *what this row is* from *what its path is*, and it
    /// moves down as the first half grows, because `file_menu` puts it where
    /// `hands_out_the_path` starts being true rather than at an index.
    ///
    /// **The editor row is gone** (user ruling 2026-08-25) and with it the whole
    /// second half this pin used to have: there is no machine on which this menu
    /// is a different length from this menu. `a_menus_length_is_a_fact_about_its_
    /// subject_and_nothing_else` is where that is now nailed.
    ///
    /// **`Show in files column` is not here**, and that is the composition of
    /// the two rulings rather than a row lost in one: the press that raises this
    /// menu happens *inside* the files column, so the verb belongs to the
    /// breadcrumb's face alone — see
    /// `the_breadcrumbs_menu_is_the_document_faces_own_list`.
    ///
    /// Order is asserted from the drawn labels rather than from the enum,
    /// because the enum's order is only a promise until something reads it in
    /// that order: a painter that walked `items` backwards would still satisfy
    /// a test that only counted rows.
    ///
    /// MUTATIONS:
    /// ① take `OpenWith` or `Reveal` out of [`file_menu`]'s `File` arm;
    /// ② put the separator back at a fixed index.
    #[test]
    fn a_file_rows_menu_draws_its_doors_then_a_rule_then_its_path_verbs() {
        let look = plain_look(FileMenuSubject::File);
        let layout = file_menu_layout(
            [300.0, 200.0],
            (960.0, 600.0),
            1.0,
            &look,
            &mut fake_measure,
        );
        let layer = one_layer(file_menu_build(&layout, &look, None));
        let names: Vec<&str> = layer
            .labels
            .iter()
            .map(|label| label.text.as_str())
            .collect();
        assert_eq!(
            names,
            vec![
                crate::i18n::Text::FileMenuOpenPreview.text(),
                crate::i18n::Text::FileMenuOpenWith.text(),
                crate::i18n::Text::FileMenuRename.text(),
                copy_path_text(),
                insert_path_text(),
                reveal_in_explorer_text(),
            ],
            "six rows since B5, top to bottom, and no heading over them"
        );
        let ways_in = layout
            .items
            .iter()
            .filter(|item| !item.row.hands_out_the_path(look.subject))
            .count();
        let separator = layout.separator.expect("a menu with both kinds of row");
        assert!(
            separator[1] >= layout.items[ways_in - 1].rect[3]
                && separator[3] <= layout.items[ways_in].rect[1],
            "the rule lies between the last door and the first path verb"
        );
        assert_eq!(
            layer
                .sprites
                .iter()
                .map(|sprite| sprite.mark)
                .collect::<Vec<_>>(),
            vec![
                ChromeMark::File,
                ChromeMark::External,
                ChromeMark::Pencil,
                ChromeMark::Copy,
                ChromeMark::Paste,
                // The *act* of revealing, so the struck rendition (P2's fill
                // policy, restored 2026-08-27): a menu's icon column is one
                // weight all the way down.
                ChromeMark::FolderOpenOutline,
            ],
            "each verb wears its own glyph — the copy and the paste are not one \
             mark twice"
        );
    }

    /// PIN (user ruling 2026-08-25, the day's second) — **every row of this
    /// menu is struck in the column's own ink, the shell's row included.**
    ///
    /// The menu is eight thin monochrome glyphs down one column, and `New
    /// terminal here` arrived wearing the default profile's *coloured* mark —
    /// PowerShell's filled blue slab in a run of hairlines. The shape was right
    /// and stays: the row is about the shell it opens, which is why it is not
    /// `#i-folder`. What was wrong was the style, and the fix is
    /// [`ChromeMark::in_line`] applied where the default profile is read.
    ///
    /// Asserted as **one style family over the whole list** rather than as "the
    /// terminal row is a `ProfileLine`", because the claim the ruling made is
    /// about the column: any row that came to carry colours of its own would be
    /// the same defect wearing a different glyph, and a pin that named one row
    /// would not catch it.
    ///
    /// All five profile marks, because which one this is depends on a setting:
    /// a pin that only tried PowerShell would go on passing the day somebody's
    /// default was Ubuntu.
    ///
    /// RED EVIDENCE (2026-08-25): with `file_menu_look` handing
    /// `profiles::mark(default)` straight through — and `plain_look` below
    /// standing in for it — the `Folder` face fails on `ProfilePowerShell`:
    /// `assertion failed: the New terminal row draws ProfilePowerShell, which
    /// carries its own colours into a column of line icons`.
    ///
    /// MUTATIONS: drop the `.in_line()` in `Runtime::file_menu_look`, or give
    /// any row here a mark that paints itself.
    #[test]
    fn every_file_menu_row_wears_the_columns_own_ink() {
        for mark in [
            ChromeMark::ProfilePowerShell,
            ChromeMark::ProfileUbuntu,
            ChromeMark::ProfileGit,
            ChromeMark::ProfileCmd,
            ChromeMark::ProfileGeneric {
                colour: crate::marks::MarkColour::Teal,
            },
        ] {
            for subject in [
                FileMenuSubject::File,
                FileMenuSubject::Folder { expanded: false },
                FileMenuSubject::Folder { expanded: true },
                FileMenuSubject::Document,
                FileMenuSubject::FoldedPath { levels: 3 },
            ] {
                let look = FileMenuLook {
                    subject,
                    crumbs: &["alpha".to_owned(), "bravo".to_owned(), "charlie".to_owned()],
                    // What `Runtime::file_menu_look` hands in, spelled here so
                    // the pin is about the whole trip and not about one call.
                    terminal: mark.in_line(),
                };
                for row in file_menu(subject).rows {
                    assert!(
                        row.mark(&look).takes_current_color(),
                        "the {row:?} row draws {:?}, which carries its own \
                         colours into a column of line icons",
                        row.mark(&look)
                    );
                }
            }
        }
    }

    /// PIN (user ruling 2026-08-25) — **the shell's row keeps its shape when it
    /// loses its colours.**
    ///
    /// The other half of the ruling, and it needs its own nail because the pin
    /// above is satisfied by *any* line glyph — `#i-panel` would pass it, and
    /// `#i-panel` is exactly the answer the first ruling of the day turned down.
    /// So: three drawings for five marks, each the one the coloured mark is, and
    /// a mark that already follows the theme is unchanged by the trip.
    ///
    /// MUTATIONS: collapse `in_line` to one glyph and the Ubuntu or the Git line
    /// names the console; make it total by mapping everything and `#i-file`
    /// stops being itself.
    #[test]
    fn a_profile_marks_line_rendition_is_its_own_drawing() {
        use crate::marks::ProfileGlyph;
        for (coloured, line) in [
            (ChromeMark::ProfilePowerShell, ProfileGlyph::Console),
            (ChromeMark::ProfileCmd, ProfileGlyph::Console),
            (
                ChromeMark::ProfileGeneric {
                    colour: crate::marks::MarkColour::Violet,
                },
                ProfileGlyph::Console,
            ),
            (ChromeMark::ProfileUbuntu, ProfileGlyph::Ubuntu),
            (ChromeMark::ProfileGit, ProfileGlyph::Git),
        ] {
            assert_eq!(
                coloured.in_line(),
                ChromeMark::ProfileLine(line),
                "{coloured:?} is one of the family's three drawings"
            );
        }
        // A mark that never carried colours of its own is already its own line
        // rendition — the fall-through is a statement, so it is pinned.
        for already in [ChromeMark::File, ChromeMark::Folder, ChromeMark::Copy] {
            assert_eq!(already.in_line(), already, "{already:?} was never coloured");
        }
    }

    /// PIN (user ruling 2026-08-25) — **a menu's length is a fact about its
    /// subject and nothing else.**
    ///
    /// This is what retiring the editor row bought back, and it is worth a nail
    /// of its own because it was true, then lost, and is true again. The three
    /// verb faces each have one length; only the `…` chip's varies, and it
    /// varies with a number its own subject carries. Nothing here asks the
    /// machine a question.
    ///
    /// RED EVIDENCE (2026-08-25): with the editor row still in
    /// `file_menu`'s `File` and `Document` arms, this cannot even be written —
    /// `file_menu` took a second argument, which is the finding.
    ///
    /// MUTATIONS: re-introduce any row conditional on anything but the subject.
    #[test]
    fn a_menus_length_is_a_fact_about_its_subject_and_nothing_else() {
        for (subject, rows) in [
            (FileMenuSubject::File, 6),
            (FileMenuSubject::Folder { expanded: false }, 6),
            (FileMenuSubject::Folder { expanded: true }, 6),
            (FileMenuSubject::Document, 4),
            (FileMenuSubject::FoldedPath { levels: 3 }, 3),
        ] {
            assert_eq!(
                file_menu(subject).rows.len(),
                rows,
                "{subject:?} draws one list, and it is this long"
            );
        }
        assert!(
            !file_menu(FileMenuSubject::File)
                .rows
                .iter()
                .chain(file_menu(FileMenuSubject::Document).rows.iter())
                .any(|row| matches!(row, FileMenuRow::Crumb(_))),
            "a folded level is a row of the chip's list and of nothing else"
        );
    }

    /// PIN (user ruling 2026-08-25, B5) — **`Rename` is a row of the two faces
    /// that stand on a name of their own, and of neither of the other two.**
    ///
    /// A file row and a folder row each *are* something on the disk, and renaming
    /// one is the ordinary verb every file manager puts on that menu. The other
    /// two faces are not: the `Document` face is the preview's own pill, and the
    /// document it names is renamed by double-clicking the last crumb — the same
    /// editor, one surface over, which is why a row here would be a second door
    /// onto a gesture already within reach; the `FoldedPath` face carries *no*
    /// verbs about any one of the folders it hides, which is a ruling of the day
    /// before this one and is not reopened by this one.
    ///
    /// It sits with the doors rather than with the path verbs, because
    /// [`FileMenuRow::hands_out_the_path`] is about handing the *text* of a path
    /// somewhere and this changes the thing the path points at.
    ///
    /// Red gate: put the row on `Document` or `FoldedPath` and the second half
    /// names the face that took it.
    #[test]
    fn the_two_faces_that_are_a_name_on_disk_can_rename_it() {
        for subject in [
            FileMenuSubject::File,
            FileMenuSubject::Folder { expanded: false },
            FileMenuSubject::Folder { expanded: true },
        ] {
            assert!(
                file_menu(subject).rows.contains(&FileMenuRow::Rename),
                "{subject:?} names something on the disk, so it may rename it"
            );
        }
        for subject in [
            FileMenuSubject::Document,
            FileMenuSubject::FoldedPath { levels: 3 },
        ] {
            assert!(
                !file_menu(subject).rows.contains(&FileMenuRow::Rename),
                "{subject:?} has no row of its own to rename"
            );
        }
        assert!(
            !FileMenuRow::Rename.hands_out_the_path(FileMenuSubject::File),
            "it changes the thing a path points at rather than handing the path out"
        );
    }

    /// PIN — **the breadcrumb's `Open ⌄` is the document face's own list** (user
    /// rulings 2026-08-24 and 2026-08-25).
    ///
    /// Two menus, one type, two lists — which is the shape the merge of the
    /// preview head's second row and the tree row's completed menu had to land
    /// on. What this face does *not* have is as load-bearing as what it does:
    /// no `Open preview`, because the preview is what raised it; no fold,
    /// because it is not a row of any tree. The first row says `Open in default
    /// app` rather than `Open with default app` because on this face there is
    /// nothing above it to be contrasted with.
    ///
    /// **Four rows since 2026-08-25**, and two of the four changed that day:
    /// the editor left this list with every other, and `Show in files column`
    /// gave its place to `Reveal in Explorer`. Both halves of that swap are
    /// rulings of the same afternoon and each one's reason is the other's — the
    /// breadcrumb above the pill now stands the column on any level of the path
    /// without leaving the tree, so a row for it was the surface repeating
    /// itself; and Explorer, refused here because the pane's foot carried it,
    /// has nowhere else to live now the feet are gone.
    ///
    /// **And it is above the rule**, which is the difference
    /// [`FileMenuRow::hands_out_the_path`] takes a subject for: on this face
    /// "open the folder it lives in" is the answer to *where is this*, not a
    /// third thing to do with the text of a path.
    ///
    /// MUTATIONS:
    /// ① give the `Document` arm `Open` or `Fold` — the label list goes red;
    /// ② let `OpenWith` wear one wording on every face — the first label goes
    ///    red on this menu or on the file row's, whichever way it is settled;
    /// ③ make `hands_out_the_path` subject-blind again — the rule lands one row
    ///    higher and the last assertion goes red.
    #[test]
    fn the_breadcrumbs_menu_is_the_document_faces_own_list() {
        let subject = FileMenuSubject::Document;
        let look = plain_look(subject);
        let layout = file_menu_layout(
            [300.0, 200.0],
            (960.0, 600.0),
            1.0,
            &look,
            &mut fake_measure,
        );
        let layer = one_layer(file_menu_build(&layout, &look, None));
        assert_eq!(
            layer
                .labels
                .iter()
                .map(|label| label.text.as_str())
                .collect::<Vec<_>>(),
            vec![
                crate::i18n::Text::FileMenuOpenDefaultApp.text(),
                reveal_in_explorer_text(),
                copy_path_text(),
                insert_path_text(),
            ],
        );
        let rows = file_menu(subject).rows;
        assert!(
            !rows.contains(&FileMenuRow::Open) && !rows.contains(&FileMenuRow::Fold),
            "no preview door and no fold on the face that is the preview"
        );
        // And the two verbs about the path are the same two the tree's rows
        // offer, in the same order, so a rename in one menu cannot drift.
        let tree_rows = file_menu(FileMenuSubject::File).rows;
        assert_eq!(rows[rows.len() - 2..], tree_rows[3..5]);
        // The rule falls under Explorer here and over it on a tree row: two
        // rows above the line on this face, one on that one.
        let rule = layout.separator.expect("a menu with both kinds of row");
        assert!(
            rule[1] >= layout.items[1].rect[3] && rule[3] <= layout.items[2].rect[1],
            "Explorer is a way in on a breadcrumb, so it stands above the rule"
        );
        let tree = plain_look(FileMenuSubject::File);
        let tree_layout = file_menu_layout(
            [300.0, 200.0],
            (960.0, 600.0),
            1.0,
            &tree,
            &mut fake_measure,
        );
        let tree_rule = tree_layout.separator.expect("both kinds of row");
        assert!(
            tree_rule[1] >= tree_layout.items[2].rect[3]
                && tree_rule[3] <= tree_layout.items[3].rect[1],
            "and on a tree row it is one of the three path verbs under the rule"
        );
        assert_eq!(
            tree_layout.items.last().map(|item| item.row),
            Some(FileMenuRow::Reveal),
            "which is where the tree row's own Explorer is: last, below the line"
        );
    }

    /// PIN (user ruling 2026-08-25) — **the `…` chip lists the levels it is
    /// standing in for, deepest first, and offers no verbs at all.**
    ///
    /// The chip used to raise the `Document` face on the *deepest* folder behind
    /// the fold, which is two wrong sentences at once: it offered `Copy path`
    /// and `Insert path` about one of several hidden folders without naming
    /// which, and it answered a control meaning *there are folders here you
    /// cannot see* with a list that never says what they are. Windows Explorer's
    /// breadcrumb `…` is the reference the ruling gave — the hidden levels, top
    /// to bottom, nearest first — and pressing one goes there.
    ///
    /// **Deepest first is the assertion, not an accident of the caller**: the
    /// row the chip stands in reads root-to-leaf, and this list reads the other
    /// way, so a builder that forgot to turn it round would draw a list that
    /// looks right in the fixture with two levels and wrong with three.
    ///
    /// RED EVIDENCE (2026-08-25): against the chip's old behaviour this test
    /// cannot be written at all — there was no `FoldedPath` subject and the chip
    /// raised `Document`. The narrower nail that *is* expressible against the
    /// old code is `the_chips_menu_offers_no_verbs_about_one_hidden_folder`
    /// below.
    ///
    /// MUTATIONS: hand `crumbs` root-first; give the face a `CopyPath`.
    #[test]
    fn the_chips_menu_lists_the_folded_levels_deepest_first() {
        let crumbs = [
            "test-assets".to_owned(),
            "BetterTerminal".to_owned(),
            "Developer".to_owned(),
        ];
        let subject = FileMenuSubject::FoldedPath {
            levels: crumbs.len(),
        };
        let look = FileMenuLook {
            subject,
            crumbs: &crumbs,
            terminal: ChromeMark::ProfilePowerShell,
        };
        let layout = file_menu_layout(
            [300.0, 200.0],
            (960.0, 600.0),
            1.0,
            &look,
            &mut fake_measure,
        );
        let layer = one_layer(file_menu_build(&layout, &look, None));
        assert_eq!(
            layer
                .labels
                .iter()
                .map(|label| label.text.as_str())
                .collect::<Vec<_>>(),
            vec!["test-assets", "BetterTerminal", "Developer"],
            "the nearest hidden level is the top row and the walk is towards the \
             root"
        );
        assert_eq!(
            layer
                .sprites
                .iter()
                .map(|sprite| sprite.mark)
                .collect::<Vec<_>>(),
            vec![ChromeMark::Folder; 3],
            "every row is a folder, because every row is a place"
        );
        assert!(
            layout.separator.is_none(),
            "a list with no verbs in it has nothing to divide"
        );
        // And each row answers for its own rectangle, in the same order.
        for (at, item) in layout.items.iter().enumerate() {
            assert_eq!(
                file_menu_hit(
                    &layout,
                    f64::from((item.rect[0] + item.rect[2]) / 2.0),
                    f64::from((item.rect[1] + item.rect[3]) / 2.0),
                ),
                Some(Some(FileMenuRow::Crumb(at))),
            );
        }
    }

    /// PIN (user ruling 2026-08-25) — **no face of this menu offers a path verb
    /// about a folder it has not named.**
    ///
    /// The half of the chip's ruling that is expressible as a property rather
    /// than as a list: `Copy path` and its two neighbours are only ever offered
    /// on a menu whose *subject* is one path, and the chip's subject is several.
    ///
    /// RED EVIDENCE (2026-08-25): against the old code — the chip raising
    /// `FileMenuSubject::Document` on the deepest hidden folder — the second
    /// assertion is the finding, and the first cannot be reached because there
    /// was no fourth subject to ask about.
    #[test]
    fn the_chips_menu_offers_no_verbs_about_one_hidden_folder() {
        let subject = FileMenuSubject::FoldedPath { levels: 4 };
        let rows = file_menu(subject).rows;
        assert!(
            rows.iter().all(|row| matches!(row, FileMenuRow::Crumb(_))),
            "the chip's list is places and nothing else"
        );
        assert!(
            !rows.iter().any(|row| row.hands_out_the_path(subject)),
            "a menu about several folders must not answer `Copy path` about one \
             of them"
        );
    }

    /// PIN — **a folder row's menu is its own two verbs and then the same three**
    /// (user ruling 2026-08-25).
    ///
    /// K143's "目录行不弹" is overturned, and this is the shape that replaces
    /// it: the fold and a shell standing in the folder above the rule, the three
    /// path verbs below it. The three below are *the same three* a file row
    /// offers, which is the whole reason both menus live in one enum.
    ///
    /// RED GATE: give the `Folder` arm of [`file_menu`] the file's rows.
    ///
    /// **The shell's mark is the default profile's own** (user ruling
    /// 2026-08-25) — see `the_new_terminal_row_wears_the_terminal_it_makes`,
    /// which is where that half is nailed; here it is only asserted that the row
    /// does not wear a folder, so the two cannot both be forgotten at once.
    #[test]
    fn a_folder_rows_menu_is_the_fold_a_shell_and_the_same_three_path_verbs() {
        let subject = FileMenuSubject::Folder { expanded: false };
        let look = plain_look(subject);
        let layout = file_menu_layout(
            [300.0, 200.0],
            (960.0, 600.0),
            1.0,
            &look,
            &mut fake_measure,
        );
        let layer = one_layer(file_menu_build(&layout, &look, None));
        assert_eq!(
            layer
                .labels
                .iter()
                .map(|label| label.text.as_str())
                .collect::<Vec<_>>(),
            vec![
                crate::i18n::Text::FolderMenuExpand.text(),
                crate::i18n::Text::FolderMenuNewTerminal.text(),
                crate::i18n::Text::FileMenuRename.text(),
                copy_path_text(),
                insert_path_text(),
                reveal_in_explorer_text(),
            ],
            "a folder has no preview seat, so its ways in are its own two — and              since B5 the name it has on the disk is a third"
        );
        let separator = layout.separator.expect("a menu with both kinds of row");
        assert!(separator[1] >= layout.items[2].rect[3] && separator[3] <= layout.items[3].rect[1]);
        // The path half is one list shared with the file row's menu, so the two
        // cannot drift apart by a rename in one of them.
        assert_eq!(
            file_menu(subject).rows[2..],
            file_menu(FileMenuSubject::File).rows[2..],
        );
    }

    /// PIN (user ruling 2026-08-25) — **`New terminal here` wears the terminal
    /// it makes, not the folder it is standing in.**
    ///
    /// The report was a screenshot of the folder menu with `#i-folder` beside
    /// `New terminal here`, one row under `Expand`'s triangle and two above
    /// `Reveal in Explorer`'s open folder — three rows, two of them folders, and
    /// the one that makes a shell was one of them. The row already knows its
    /// folder (you right-clicked it); what it makes is a tab of the default
    /// profile, so it wears the mark that profile's tab and pane head wear.
    ///
    /// **Asked of two different defaults**, because a mark hard-coded to
    /// whatever the fixture's default happens to be would pass while saying
    /// nothing.
    ///
    /// RED EVIDENCE (2026-08-25): with the arm returning `ChromeMark::Folder`
    /// this fails on the first assertion — "left: Folder, right:
    /// ProfilePowerShell".
    ///
    /// MUTATIONS: return any fixed mark from the `NewTerminal` arm.
    #[test]
    fn the_new_terminal_row_wears_the_terminal_it_makes() {
        for terminal in [ChromeMark::ProfilePowerShell, ChromeMark::ProfileUbuntu] {
            let look = FileMenuLook {
                terminal,
                ..plain_look(FileMenuSubject::Folder { expanded: false })
            };
            let layout = file_menu_layout(
                [300.0, 200.0],
                (960.0, 600.0),
                1.0,
                &look,
                &mut fake_measure,
            );
            let layer = one_layer(file_menu_build(&layout, &look, None));
            let at = file_menu(look.subject)
                .rows
                .iter()
                .position(|row| *row == FileMenuRow::NewTerminal)
                .expect("a folder row offers a shell");
            assert_eq!(
                layer.sprites[at].mark, terminal,
                "the row wears the shell it starts"
            );
            assert_ne!(
                layer.sprites[at].mark,
                ChromeMark::Folder,
                "and it is not the folder it is standing in"
            );
        }
    }

    /// PIN — **the fold row wears the face of the fold it is standing in**
    /// (user ruling 2026-08-25), word and mark turning together.
    ///
    /// `PaneMenuZoom`/`PaneMenuRestore`'s rule, one menu along: a toggle says one
    /// thing over a shut folder and another over an open one, and a single
    /// wording would be the menu describing what the row was a press ago. The
    /// mark is the tree's own triangle at the tree's own two angles, so a reader
    /// who has opened a folder here has already been taught what it means.
    ///
    /// RED GATE: return one wording from [`FileMenuRow::text`]'s `Fold` arm, or
    /// a fixed angle from its `mark`.
    #[test]
    fn the_fold_row_turns_its_word_and_its_triangle_together() {
        let shut = plain_look(FileMenuSubject::Folder { expanded: false });
        let open = plain_look(FileMenuSubject::Folder { expanded: true });
        assert_eq!(
            FileMenuRow::Fold.text(&shut),
            crate::i18n::Text::FolderMenuExpand.text()
        );
        assert_eq!(
            FileMenuRow::Fold.text(&open),
            crate::i18n::Text::FolderMenuCollapse.text()
        );
        assert_eq!(
            FileMenuRow::Fold.mark(&shut),
            crate::marks::tree_disclosure(0.0)
        );
        assert_eq!(
            FileMenuRow::Fold.mark(&open),
            crate::marks::tree_disclosure(1.0)
        );
        assert_ne!(
            FileMenuRow::Fold.mark(&shut),
            FileMenuRow::Fold.mark(&open),
            "the two ends of the turn are two different marks"
        );
        // And the rows either side of it say the same thing whichever fold the
        // menu came up in: only the toggle turns.
        for row in [FileMenuRow::NewTerminal, FileMenuRow::CopyPath] {
            assert_eq!(row.text(&shut), row.text(&open));
        }
    }

    /// PIN — **the default-app door says `with` on a tree row and `in` on a
    /// breadcrumb**, and it is one row rather than two.
    ///
    /// Both wordings survived the 2026-08-24/25 merge because both are used and
    /// each says something the other cannot: on a tree row the preposition
    /// contrasts this door with the `Open preview` above it, and on a breadcrumb
    /// there is nothing above it to contrast with. What must not survive is two
    /// *rows* for one act — the runtime dispatches this one variant through
    /// `open_local_path` whichever face raised it.
    ///
    /// RED GATE: collapse [`FileMenuRow::text`]'s `OpenWith` arm to one string.
    #[test]
    fn the_default_app_door_is_one_row_wearing_each_faces_preposition() {
        let file = plain_look(FileMenuSubject::File);
        let document = plain_look(FileMenuSubject::Document);
        assert_eq!(
            FileMenuRow::OpenWith.text(&file),
            crate::i18n::Text::FileMenuOpenWith.text()
        );
        assert_eq!(
            FileMenuRow::OpenWith.text(&document),
            crate::i18n::Text::FileMenuOpenDefaultApp.text()
        );
        assert_ne!(
            FileMenuRow::OpenWith.text(&file),
            FileMenuRow::OpenWith.text(&document),
        );
        // One glyph, because it is one act: the bare arrow that means *this
        // content leaves the window*.
        assert_eq!(
            FileMenuRow::OpenWith.mark(&file),
            FileMenuRow::OpenWith.mark(&document),
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
        assert_eq!(CHEVRON_HOVER_OPEN_DELAY, Duration::from_millis(250));
        let start = Instant::now();
        let mut gate = ChevronGate::default();
        gate.observe(ChevronPointer::Button, false, start);
        assert_eq!(gate.due(start), None);
        assert_eq!(gate.due(start + Duration::from_millis(249)), None);
        assert_eq!(
            gate.due(start + CHEVRON_HOVER_OPEN_DELAY),
            Some(ChevronAction::Open)
        );
        assert_eq!(
            gate.deadline(),
            Some(start + CHEVRON_HOVER_OPEN_DELAY),
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
            jittering.due(start + CHEVRON_HOVER_OPEN_DELAY),
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

    /// The other windows a pin that is not about them stands in front of.
    ///
    /// **Two of them, and not none**, since the ruling of 2026-08-25: with no
    /// second window there is no `Move to window ▸` row at all, and a pin about
    /// the menu's shape that quietly stood on the *shorter* menu would stop
    /// saying anything about the longer one. The menu with the row missing has a
    /// pin of its own.
    fn other_windows() -> Vec<String> {
        vec!["Window 2".to_owned(), "Window 3".to_owned()]
    }

    /// The verbs of a whole menu, in order — what a pin about *every* row of it
    /// asks for.
    ///
    /// A helper here rather than a `const` on the enum, since the ruling of
    /// 2026-08-25: a second list of these rows in the library is a second list
    /// the layout could come to disagree with, which is the very bug the ruling
    /// made reachable. The tests are entitled to build one; the menu is not.
    fn text_rows() -> Vec<PaneMenuRow> {
        PaneMenuRow::rows(true)
            .into_iter()
            .filter(|row| *row != PaneMenuRow::Picker)
            .collect()
    }

    /// RED (gesture audit 2026-08-26, 系统性发现 ②) — **a menu row prints the
    /// chord that runs the same verb, and prints the reader's own.**
    ///
    /// The audit's second systemic finding: `profiles.rs` had no accelerator
    /// column anywhere, so `Close pane` and `Ctrl+Shift+W` were introduced to a
    /// reader twice, as strangers — the hint card teaches the keyboard's side
    /// and nothing taught the menus'. 「加一列 accel 是把十几条乙升级成『互相
    /// 教』的最便宜的一次改动」.
    ///
    /// Four claims, and the last two are the ones worth the test:
    ///
    /// ① the rows that *are* table rows print their chord;
    /// ② a row wearing a `▸` prints none — one trailing slot, one thing in it,
    ///    and a chord beside a submenu heading would be describing the row
    ///    below it;
    /// ③ the column is read off the **effective** table, so a rebind follows
    ///    and an unbind takes the annotation away with it;
    /// ④ the frame is measured **with the column in it** — a menu that drew a
    ///    chord it had not reserved room for would print it over the name.
    ///
    /// MUTATIONS: drop `accel_claim` from `content` and ④ goes red; read
    /// `BINDINGS` instead of the effective table in
    /// [`crate::shortcuts::Shortcuts::accelerator`] and ③ goes red; drop the
    /// `has_submenu` guard in [`accelerator_of`] and ② goes red.
    #[test]
    fn a_menu_row_prints_the_chord_that_runs_the_same_verb() {
        let with_chords = pane_menu(false);
        let chord_of = |layout: &PaneMenuLayout, row: PaneMenuRow| {
            layout
                .rows
                .iter()
                .position(|it| *it == row)
                .and_then(|at| layout.accels[at].clone())
                .map(|(text, _)| text)
        };
        assert_eq!(
            chord_of(&with_chords, PaneMenuRow::ClosePane).as_deref(),
            Some("Ctrl+Shift+W")
        );
        assert_eq!(
            chord_of(&with_chords, PaneMenuRow::ZoomPane).as_deref(),
            Some("Ctrl+Shift+X")
        );
        assert_eq!(
            chord_of(&with_chords, PaneMenuRow::Duplicate).as_deref(),
            Some("Ctrl+Shift+D")
        );
        // ② The two headings, and the drawing.
        assert_eq!(chord_of(&with_chords, PaneMenuRow::SplitWith), None);
        assert_eq!(chord_of(&with_chords, PaneMenuRow::MoveToWindow), None);
        assert_eq!(chord_of(&with_chords, PaneMenuRow::Picker), None);
        // And a row that simply is not a table row.
        assert_eq!(chord_of(&with_chords, PaneMenuRow::MoveToNewWindow), None);

        // ③ The reader's table and not this build's.
        let mut rebound = crate::shortcuts::Shortcuts::defaults();
        rebound.apply_overrides(&[
            crate::shortcuts::Override {
                id: "close-pane".to_owned(),
                chord: Some("Ctrl+Shift+J".to_owned()),
            },
            crate::shortcuts::Override {
                id: "zoom-pane".to_owned(),
                chord: None,
            },
        ]);
        let edited = pane_menu_layout(
            [300.0, 120.0],
            (960.0, 600.0),
            1.0,
            None,
            false,
            &other_windows(),
            &rebound,
            &mut fake_measure,
        );
        assert_eq!(
            chord_of(&edited, PaneMenuRow::ClosePane).as_deref(),
            Some("Ctrl+Shift+J")
        );
        assert_eq!(
            chord_of(&edited, PaneMenuRow::ZoomPane),
            None,
            "a chord the reader gave back to their shell is not one a menu may print"
        );

        // ④ The column is paid for out of the frame, not out of the name.
        let bare = {
            let mut table = crate::shortcuts::Shortcuts::defaults();
            table.apply_overrides(&[
                crate::shortcuts::Override {
                    id: "close-pane".to_owned(),
                    chord: None,
                },
                crate::shortcuts::Override {
                    id: "zoom-pane".to_owned(),
                    chord: None,
                },
                crate::shortcuts::Override {
                    id: "duplicate-pane-split".to_owned(),
                    chord: None,
                },
            ]);
            pane_menu_layout(
                [300.0, 120.0],
                (960.0, 600.0),
                1.0,
                None,
                false,
                &other_windows(),
                &table,
                &mut fake_measure,
            )
        };
        assert!(
            bare.rows.iter().all(|row| chord_of(&bare, *row).is_none()),
            "a table with these three unbound has nothing to print"
        );
        let width = |layout: &PaneMenuLayout| layout.frame[2] - layout.frame[0];
        assert!(
            width(&with_chords) > width(&bare),
            "the chord column is reserved in the frame, not drawn over the names"
        );

        // **And the third family that has anything to print**: the profile
        // picker's `Files pane`, which calls the very function
        // `Action::FilesPane` dispatches to. The other five menu families carry
        // no row that is a table row at all, so their column is empty by fact
        // rather than by omission — the audit's own reading, not a shortfall.
        let picker = layout(
            [0.0, 0.0, 100.0, 30.0],
            MenuSide::Below,
            (960.0, 600.0),
            1.0,
            &[],
            &chord_table(),
            &mut fake_measure,
        );
        assert_eq!(
            picker.files_pane_accel.map(|(text, _)| text).as_deref(),
            Some("Ctrl+Shift+B")
        );
    }

    fn pane_menu(submenu_open: bool) -> PaneMenuLayout {
        pane_menu_layout(
            [300.0, 120.0],
            (960.0, 600.0),
            1.0,
            // The house's first child, which is the one every pin below was
            // written about — `Move to window` has its own.
            submenu_open.then_some(PaneMenuRow::SplitWith),
            // The ordinary posture: these pins are about the menu's shape, and
            // the zoom row's second face has a pin of its own.
            false,
            &other_windows(),
            &chord_table(),
            &mut fake_measure,
        )
    }

    /// RED GATE (the animation slice, 2026-08-26) — **every menu in this file
    /// says which way it grew, and a menu the window's edge pushed back up says
    /// so.**
    ///
    /// The four pixels a popup travels as it arrives are provenance and nothing
    /// else: they say *this came out of that*. A menu raised near the floor of
    /// the window is placed above the point that raised it, and one that slid
    /// down into place from the direction it is standing on would be four pixels
    /// of a lie — the one failure mode a constant direction cannot avoid, which
    /// is why the direction is derived per placement.
    ///
    /// Mutation: hard-code `Travel::Down` at any of the eight construction sites
    /// and the flipped case below reverses.
    #[test]
    fn a_menu_grows_down_out_of_the_press_and_up_when_the_floor_pushes_it_back() {
        assert_eq!(
            pane_menu(false).travel(),
            Travel::Down,
            "a menu with room below the press hangs off it"
        );
        // The same press, near the floor: the clamp puts the whole menu above
        // where the hand was, so it has to arrive from below.
        let low = pane_menu_layout(
            [300.0, 590.0],
            (960.0, 600.0),
            1.0,
            None,
            false,
            &other_windows(),
            &chord_table(),
            &mut fake_measure,
        );
        assert_eq!(
            low.travel(),
            Travel::Up,
            "a menu the floor pushed up above the press must arrive from below it"
        );
        // And the child hangs off a *row*, which makes its direction the one
        // horizontal answer in the window.
        assert_eq!(
            pane_menu(true).submenu_travel(),
            Some(Travel::Right),
            "a child with room to its right comes out of its parent's row"
        );
        // Pushed against the right edge, the child opens to the left and says so.
        let cornered = pane_menu_layout(
            [700.0, 120.0],
            (960.0, 600.0),
            1.0,
            Some(PaneMenuRow::SplitWith),
            false,
            &other_windows(),
            &chord_table(),
            &mut fake_measure,
        );
        assert_eq!(cornered.submenu_travel(), Some(Travel::Left));
    }

    /// PIN (user ruling, 2026-08-16): **the menu is a picker and six verbs,
    /// with a rule above the one that destroys.**
    ///
    /// The order is the ruling's own, and it is an order of *commitment*: point
    /// at a direction, name a profile, name a folder, repeat this pane, move this
    /// pane out of its tab, move it out of its window, end it. The separator's
    /// position is the claim about reading — the verbs that make or move a pane,
    /// then the one that ends one, because a destructive verb flush against
    /// constructive ones is a verb the hand finds by overshooting.
    ///
    /// **The sixth verb is F1c's** (multiwindow slice F1c, `plan.md` F1c). It
    /// stands directly under the row it is composed of — a pane out of its tab,
    /// then a pane out of its window — so the two lengths of one journey read as
    /// one pair rather than as two unrelated exits.
    ///
    /// Red gate: put the rule under the picker and the menu claims the four verbs
    /// below it are a different kind of thing from the diagram above; drop the
    /// picker out of `ALL` and the keyboard walk silently starts at `Split with`.
    ///
    /// **Also the pin that this menu holds no window verbs** (user ruling
    /// 2026-08-19). Focus mode had a row here for one day; the list is asserted
    /// whole and by name, so putting it — or anything else about the *window* —
    /// back into a menu whose every line acts on the pane under it turns this
    /// red on the first assertion.
    ///
    /// **The seventh verb is §7.1.6l's** (2026-08-24): `Zoom pane`, directly
    /// under the diagram, because those two entries are the pair that answers
    /// how much of the stage this pane gets. It is *not* the window verb this
    /// test's last paragraph turns away — a zoom is one pane's share of one
    /// tab's stage, which is what every other line here is about too.
    #[test]
    fn the_pane_menu_is_a_picker_and_eight_verbs_with_a_rule_above_the_close() {
        assert_eq!(
            PaneMenuRow::ALL,
            [
                PaneMenuRow::Picker,
                PaneMenuRow::ZoomPane,
                PaneMenuRow::SplitWith,
                PaneMenuRow::NewInFolder,
                PaneMenuRow::Duplicate,
                PaneMenuRow::MoveToNewTab,
                PaneMenuRow::MoveToNewWindow,
                // **The third exit** (B9, 2026-08-25), and it belongs to this
                // list for the reason the two above it do: it is a verb about
                // *this pane*, and it is the same journey at its third length.
                PaneMenuRow::MoveToWindow,
                PaneMenuRow::ClosePane,
            ]
        );
        let layout = pane_menu(false);
        let layer = one_layer(pane_menu_build(
            &layout,
            None,
            None,
            &equipped(),
            &[],
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
                picker_caption_text(),
                zoom_pane_text(),
                // **And the chord that runs the same verb** (gesture audit
                // 2026-08-26, 系统性发现 ②), drawn straight after the row it
                // belongs to. Three of the eight carry one; the other five are
                // not rows of the shortcut table — see
                // [`PaneMenuRow::accelerator`], which names all eight.
                "Ctrl+Shift+X",
                split_with_text(),
                new_in_folder_text(),
                duplicate_pane_text(),
                "Ctrl+Shift+D",
                move_to_new_tab_text(),
                move_to_new_window_text(),
                // `Move to window ▸` wears a chevron, and a row with a chevron
                // never wears a chord — one trailing slot, one thing in it.
                move_to_window_text(),
                close_pane_text(),
                "Ctrl+Shift+W",
            ],
            "the caption under the diagram, then eight rows with their chords,              and no heading over them"
        );
        assert_ne!(
            move_to_new_tab_text(),
            move_to_new_window_text(),
            "the two exits differ by the container they name, and by nothing else \
             — including by not being the same string"
        );
        let close = layout.item(PaneMenuRow::ClosePane);
        let above = layout.item(PaneMenuRow::MoveToWindow);
        assert!(
            layout.separator[1] >= above[3] && layout.separator[3] <= close[1],
            "the rule lies between the last constructive verb and `Close pane`"
        );
        assert!(
            layout.item(PaneMenuRow::Picker)[3] <= layout.item(PaneMenuRow::ZoomPane)[1],
            "and the picker stands above every word about this pane"
        );
        // The picker is the first entry again, which is the whole of what
        // withdrawing the focus row did to this geometry: nothing stands above
        // the diagram, and the menu carries one rule rather than two.
        assert_eq!(
            layout.item(PaneMenuRow::Picker)[1],
            layout.frame[1] + (FLOAT_WINDOW_BORDER_LOGICAL_PX).max(1.0) + MENU_PADDING_LOGICAL_PX,
            "the diagram sits directly under the menu's own padding"
        );
    }

    /// PIN (user ruling 2026-08-25, B9) — **`Move to window ▸` stands beside the
    /// two exits it completes, and it is the second row of this menu to hang a
    /// list off itself.**
    ///
    /// The three exits are one sentence read at three lengths: out of this tree
    /// into a tab, out of this window into a new one, out of this window into
    /// one that already exists. The third has to be a submenu because it is the
    /// only one whose destination is a *choice* — and a window is not something
    /// a menu row can name, so the submenu names them the way a reader can pick
    /// between them: by their place in the run and by how much is in them.
    ///
    /// Red gate: leave `has_submenu` keyed on `SplitWith` alone and the row
    /// draws no `▸`, opens nothing, and answers a press by running a verb that
    /// does not exist.
    #[test]
    fn the_third_exit_names_a_window_that_is_already_open() {
        assert_eq!(
            PaneMenuRow::ALL,
            [
                PaneMenuRow::Picker,
                PaneMenuRow::ZoomPane,
                PaneMenuRow::SplitWith,
                PaneMenuRow::NewInFolder,
                PaneMenuRow::Duplicate,
                PaneMenuRow::MoveToNewTab,
                PaneMenuRow::MoveToNewWindow,
                PaneMenuRow::MoveToWindow,
                PaneMenuRow::ClosePane,
            ],
            "the third exit stands directly under the second"
        );
        assert!(PaneMenuRow::MoveToWindow.has_submenu());
        assert!(PaneMenuRow::SplitWith.has_submenu());
        for row in PaneMenuRow::ALL {
            assert_eq!(
                row.has_submenu(),
                matches!(row, PaneMenuRow::SplitWith | PaneMenuRow::MoveToWindow),
                "{row:?}: exactly two rows hang a list off themselves"
            );
        }

        // The child hangs off the row that opened it, and says which list it is.
        let windows = [
            "Window 2 · 3 tabs".to_owned(),
            "Window 3 · 1 tab".to_owned(),
        ];
        let layout = pane_menu_layout(
            [300.0, 120.0],
            (960.0, 600.0),
            1.0,
            Some(PaneMenuRow::MoveToWindow),
            false,
            &windows,
            &chord_table(),
            &mut fake_measure,
        );
        assert_eq!(layout.submenu_kind(), Some(PaneMenuRow::MoveToWindow));
        let rows = layout.submenu_rows().expect("the child is up");
        assert_eq!(rows.len(), windows.len(), "one row per other window");
        let heading = layout.item(PaneMenuRow::MoveToWindow);
        assert!(
            rows[0][1] <= heading[3],
            "and it hangs off its own heading rather than off the first submenu's"
        );
        assert_eq!(
            pane_menu_hit(
                &layout,
                f64::from((rows[1][0] + rows[1][2]) / 2.0),
                f64::from((rows[1][1] + rows[1][3]) / 2.0),
            ),
            Some(PaneMenuHit::Submenu(1)),
            "a press on a row answers with its place in the list it is drawn from"
        );

        // **One window is no choice at all**, so there is no child — and since
        // the ruling of 2026-08-25 no heading either. The row's whole absence is
        // pinned next door; what this one asserts is that a menu asked for the
        // child of a row it is not showing answers with no child rather than
        // reaching for a box that does not exist.
        let alone = pane_menu_layout(
            [300.0, 120.0],
            (960.0, 600.0),
            1.0,
            Some(PaneMenuRow::MoveToWindow),
            false,
            &[],
            &chord_table(),
            &mut fake_measure,
        );
        assert!(
            alone.submenu_rows().is_none_or(<[[f32; 4]]>::is_empty),
            "a list of nowhere to go is not a list"
        );
    }

    /// PIN (user ruling 2026-08-25) — **with nowhere to go, `Move to window ▸`
    /// is not in the menu at all.**
    ///
    /// It stood there greyed with its `▸` still on it for a day, and a
    /// disclosure arrow over nothing is the one refusal this window may not
    /// make: the arrow's whole sentence is "there is more this way", and there
    /// was not. Nothing is lost by its going — `Move pane to new window`, the
    /// row directly above, is the verb for exactly the case where there is no
    /// second window, and this row was added *beside* it rather than instead of
    /// it.
    ///
    /// Asserted three ways over because a row can be absent from a list and
    /// still be on the glass: the list, the drawn captions, and the frame's own
    /// height, which is the one that catches a row left out of the walk but
    /// still paid for in pixels.
    ///
    /// **The pin next door is not in tension with this.**
    /// `a_menus_length_is_a_fact_about_its_subject_and_nothing_else` is about
    /// the *file* menu, whose subject is a row on the disk; this menu's subject
    /// is a pane, and "is there a second window to move it into" is a fact about
    /// the session the pane is in rather than a question about the machine. The
    /// two lists never meet.
    ///
    /// RED EVIDENCE (2026-08-25): before the change, `pane_menu_layout` walked
    /// `PaneMenuRow::ALL` and the row was minted whatever `windows` held —
    /// `assertion `left == right` failed: a lone window draws no row for moving
    /// a pane into another one` with `left: [Picker, ZoomPane, SplitWith,
    /// NewInFolder, Duplicate, MoveToNewTab, MoveToNewWindow, MoveToWindow,
    /// ClosePane]`.
    ///
    /// MUTATIONS: put the row back into `PaneMenuRow::rows` unconditionally, or
    /// key the layout's walk to `ALL` again, and the first or the third
    /// assertion goes red.
    #[test]
    fn a_lone_window_offers_no_row_for_moving_a_pane_into_another() {
        let lay = |windows: &[String]| {
            pane_menu_layout(
                [300.0, 120.0],
                (960.0, 600.0),
                1.0,
                None,
                false,
                windows,
                &chord_table(),
                &mut fake_measure,
            )
        };
        let alone = lay(&[]);
        assert_eq!(
            alone.rows(),
            [
                PaneMenuRow::Picker,
                PaneMenuRow::ZoomPane,
                PaneMenuRow::SplitWith,
                PaneMenuRow::NewInFolder,
                PaneMenuRow::Duplicate,
                PaneMenuRow::MoveToNewTab,
                PaneMenuRow::MoveToNewWindow,
                PaneMenuRow::ClosePane,
            ],
            "a lone window draws no row for moving a pane into another one"
        );
        let captions: Vec<String> = one_layer(pane_menu_build(
            &alone,
            None,
            None,
            &equipped(),
            &[],
            &mut fake_measure,
        ))
        .labels
        .iter()
        .map(|label| label.text.clone())
        .collect();
        assert!(
            !captions.iter().any(|text| text == move_to_window_text()),
            "and does not print its words either"
        );

        // A second window puts it back, in its own place — under the exit it is
        // the third length of.
        let peers = lay(&other_windows());
        assert_eq!(peers.rows(), PaneMenuRow::ALL, "a peer restores the row");
        let taller = peers.item(PaneMenuRow::ClosePane)[3] - peers.item(PaneMenuRow::Picker)[1];
        let shorter = alone.item(PaneMenuRow::ClosePane)[3] - alone.item(PaneMenuRow::Picker)[1];
        assert!(
            taller > shorter,
            "and the frame is paid for in pixels either way, never reserved for a \
             row that is not drawn"
        );
        // The rule still falls above `Close pane` on both, which is what keys it
        // to the row it stands over rather than to the one it happens to follow.
        for menu in [&alone, &peers] {
            let close = menu.item(PaneMenuRow::ClosePane);
            let above = menu.item(if menu.rows().contains(&PaneMenuRow::MoveToWindow) {
                PaneMenuRow::MoveToWindow
            } else {
                PaneMenuRow::MoveToNewWindow
            });
            assert!(
                menu.separator[1] >= above[3] && menu.separator[3] <= close[1],
                "the rule lies between the last mover and the one that ends a pane"
            );
        }
        // And the keyboard walks what is drawn: the row below the third exit is
        // `Close pane` on a lone window, not a highlight on nothing.
        assert_eq!(
            PaneMenuHover::step(
                Some(PaneMenuHover::Row(PaneMenuRow::MoveToNewWindow)),
                MenuStep::Down,
                0,
                alone.rows(),
            ),
            Some(PaneMenuHover::Row(PaneMenuRow::ClosePane))
        );
        assert_eq!(
            PaneMenuHover::step(
                Some(PaneMenuHover::Row(PaneMenuRow::MoveToNewWindow)),
                MenuStep::Down,
                0,
                peers.rows(),
            ),
            Some(PaneMenuHover::Row(PaneMenuRow::MoveToWindow))
        );
    }

    /// PIN (§7.1.6l) — **the zoom row has two faces, and the word and the mark
    /// change together.**
    ///
    /// A toggle that changed only one of the two would be a drawing saying one
    /// thing while the words say the other, which is the `Lock` ruling
    /// (§7.1.6c-8) read here: 「只改词不改图,图还在说谎」.
    ///
    /// It stands directly under the picker because those two entries are the
    /// pair that answers "how much of the stage is this pane's" — the diagram
    /// puts another pane beside it, the row puts every other pane away — and the
    /// four rows below make, move or end a pane.
    ///
    /// Red gate: return the same word for both states and the first pair of
    /// assertions goes red; return the same mark and the second does.
    #[test]
    fn the_zoom_row_changes_its_word_and_its_mark_with_the_state_it_names() {
        assert_eq!(
            text_rows()[0],
            PaneMenuRow::ZoomPane,
            "the row stands directly under the diagram"
        );
        assert_ne!(
            PaneMenuRow::ZoomPane.text_when(false),
            PaneMenuRow::ZoomPane.text_when(true),
            "a row that reads the same in both states lies in one of them"
        );
        assert_ne!(
            PaneMenuRow::ZoomPane.mark_when(false),
            PaneMenuRow::ZoomPane.mark_when(true),
            "and the mark turns with the word, or the drawing is the liar"
        );
        for row in PaneMenuRow::ALL {
            if row == PaneMenuRow::ZoomPane {
                continue;
            }
            assert_eq!(
                (row.text_when(false), row.mark_when(false)),
                (row.text_when(true), row.mark_when(true)),
                "{row:?} is not about the zoom and must not move with it"
            );
        }
        // Both faces are laid out and drawn: the frame is measured against the
        // words it is about to hold, so a zoomed pane's menu cannot come up
        // clipping its own first row.
        for zoomed in [false, true] {
            let layout = pane_menu_layout(
                [300.0, 120.0],
                (960.0, 600.0),
                1.0,
                None,
                zoomed,
                &[],
                &chord_table(),
                &mut fake_measure,
            );
            let layer = one_layer(pane_menu_build(
                &layout,
                None,
                None,
                &equipped(),
                &[],
                &mut fake_measure,
            ));
            assert_eq!(
                layer.labels[1].text,
                PaneMenuRow::ZoomPane.text_when(zoomed),
                "the row draws the face its layout was measured for"
            );
        }
    }

    /// PIN (§7.1.6l, on §7.1.6i's own argument) — **a lone pane's right-click
    /// segment carries no zoom row.**
    ///
    /// The segment already turns away `Move pane to new tab` because a lone pane
    /// 「本身就是整个 tab,无物可搬也无处可搬」. A lone pane is already the whole
    /// stage, so zoom has nothing to put away: the same sentence about a
    /// different verb.
    ///
    /// Red gate: add the row to `TERM_MENU_LONE_PANE_ROWS` and this goes red.
    #[test]
    fn a_lone_panes_terminal_menu_offers_no_zoom_because_it_is_already_the_stage() {
        assert!(
            !TERM_MENU_LONE_PANE_ROWS.contains(&PaneMenuRow::ZoomPane),
            "a lone pane has nothing to put away"
        );
        assert!(
            !TERM_MENU_LONE_PANE_ROWS.contains(&PaneMenuRow::MoveToNewTab),
            "the row this one is reasoned from is still out too"
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
        let rows = count();
        // The list the walk is over — the whole of it, because this pin is about
        // the compass rather than about which verbs a session offers. The menu
        // that shows one row fewer has a pin of its own
        // (`a_lone_window_offers_no_row_for_moving_a_pane_into_another`).
        let shown = PaneMenuRow::rows(true);
        // The list is entered at whichever end the key names. The picker is the
        // first entry again (user ruling 2026-08-19 withdrew the row that stood
        // above it), so `↓` into an unlit menu enters the compass.
        assert_eq!(
            H::step(None, MenuStep::Down, rows, &shown),
            Some(H::Zone(SplitZone::Right))
        );
        // And the picker is the top: aiming up twice from it clamps, exactly as
        // every other walk in this window clamps at its ends.
        assert_eq!(
            H::step(Some(H::Zone(SplitZone::Up)), MenuStep::Up, rows, &shown),
            None,
            "nothing stands above the diagram, so the walk clamps there"
        );
        assert_eq!(
            H::step(None, MenuStep::Up, rows, &shown),
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
                H::step(Some(H::Zone(from)), step, rows, &shown),
                Some(H::Zone(zone)),
                "{step:?} from {from:?} aims at {zone:?}"
            );
        }
        // Aiming where the highlight already points is not a movement: it is
        // the request to leave, and sideways there is nowhere to go.
        assert_eq!(
            H::step(
                Some(H::Zone(SplitZone::Right)),
                MenuStep::Right,
                rows,
                &shown
            ),
            None
        );
        assert_eq!(
            H::step(Some(H::Zone(SplitZone::Left)), MenuStep::Left, rows, &shown),
            None
        );
        // Aiming twice in the same direction walks out, and only downward leads
        // anywhere: the picker is the first entry, so `↑` from `Up` is the top of
        // the list and clamps (asserted above).
        // The first text row, whichever verb that is: §7.1.6l put `Zoom pane`
        // directly under the diagram, and the walk asks by position for exactly
        // this reason — a step named after a verb steps over the one inserted
        // above it.
        assert_eq!(
            H::step(Some(H::Zone(SplitZone::Down)), MenuStep::Down, rows, &shown),
            Some(H::Row(text_rows()[0]))
        );
        // Back in from below, landing on the zone nearest the row it came from.
        assert_eq!(
            H::step(Some(H::Row(text_rows()[0])), MenuStep::Up, rows, &shown),
            Some(H::Zone(SplitZone::Down))
        );
        // The flat part of the walk clamps at the bottom.
        assert_eq!(
            H::step(
                Some(H::Row(PaneMenuRow::ClosePane)),
                MenuStep::Down,
                rows,
                &shown
            ),
            Some(H::Row(PaneMenuRow::ClosePane))
        );
        // And the submenu has a walk of its own, clamped at both ends.
        assert_eq!(
            H::step(Some(H::Submenu(0)), MenuStep::Up, rows, &shown),
            Some(H::Submenu(0))
        );
        assert_eq!(
            H::step(Some(H::Submenu(0)), MenuStep::Down, rows, &shown),
            Some(H::Submenu(1))
        );
        assert_eq!(
            H::step(Some(H::Submenu(rows - 1)), MenuStep::Down, rows, &shown),
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
        for row in text_rows() {
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
        let layout = pane_menu_layout(
            [950.0, 596.0],
            surface,
            1.0,
            None,
            false,
            &[],
            &chord_table(),
            &mut fake_measure,
        );
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
            &[],
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
            &[],
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

    /// PIN (user reports 2026-08-19, both of them) — **the child meets the
    /// parent on the parent's own border: never over its rows, and never with a
    /// column of window between them.**
    ///
    /// Two screenshots on one day, from opposite sides of the same edge. The
    /// first: the child seated `border + padding` inside the parent covered the
    /// right-hand column of every row it stood beside, `▸` included, and the
    /// parent read as truncated. The second, after that was fixed by pushing the
    /// child *clear* by [`MENU_OFFSET_LOGICAL_PX`]: a hand crossing those four
    /// pixels was, for the width of them, on neither surface — so the pane
    /// chevron's leave grace ran against a hand that had left nothing, and a slow
    /// crossing shut the menu in the gap it was drawn across.
    ///
    /// So the seam is one border wide and it is an **overlap** — Qt's
    /// `PM_SubMenuOverlap` ("the horizontal overlap between a submenu and its
    /// parent", derived from the menu frame's own panel width), GTK's negative
    /// `horizontal-offset`, Radix's flush `SubContent`. Four claims here, because
    /// four things can go wrong separately:
    ///
    /// 1. With room on the right the child's near edge lands exactly on the
    ///    parent's border, so the two share a column and nothing else.
    /// 2. It covers the border and **not** the row content behind it — the first
    ///    report's claim, still owed.
    /// 3. Walking a pointer across the join finds no x where the menu says it
    ///    has been left — the second report's claim, and the one the geometry
    ///    exists for.
    /// 4. With no room on the right it flips, and both claims hold on that side
    ///    too — and it flips rather than clamping, because clamping is what put
    ///    it back on top of the parent in the first place.
    ///
    /// Red gate: restore `let seam = -px(MENU_OFFSET_LOGICAL_PX)` (the four-pixel
    /// gap) and claims 1 and 3 go red on both sides; restore `border + padding`
    /// and claim 2 goes red; restore `left_of.max(edge)` and the flipped case
    /// overlaps in a window this narrow.
    #[test]
    fn the_submenu_meets_the_parent_on_its_own_border_with_no_column_between_them() {
        let border = FLOAT_WINDOW_BORDER_LOGICAL_PX.max(1.0);
        let padding = MENU_PADDING_LOGICAL_PX;

        // Every column between the two rectangles belongs to one of them: no x
        // anywhere across the join answers "this pointer is not on the menu".
        let unbroken = |layout: &PaneMenuLayout, from: f32, to: f32, y: f32| {
            let mut at = from;
            while at <= to {
                assert!(
                    layout.contains(at, y),
                    "a hand at x={at} (y={y}) is on neither surface: parent {:?}, child {:?}",
                    layout.frame,
                    layout.submenu_frame()
                );
                at += 0.5;
            }
        };

        // Room on the right: the child hangs off the parent's right edge, one
        // border of it standing on the parent's own.
        let roomy = pane_menu_layout(
            [300.0, 120.0],
            (1600.0, 900.0),
            1.0,
            Some(PaneMenuRow::SplitWith),
            false,
            &[],
            &chord_table(),
            &mut fake_measure,
        );
        let parent = roomy.frame;
        let child = roomy.submenu_frame().expect("an open submenu has a frame");
        assert_eq!(
            child[0],
            parent[2] - border,
            "the child's near edge stands on the parent's border and nowhere else"
        );
        assert!(
            child[0] >= parent[2] - border && child[2] > parent[2],
            "it covers the hairline and not the column behind it — the old \
             `border + padding` seat would put it {padding} further in: \
             {parent:?} against {child:?}"
        );
        let heading = roomy.item(PaneMenuRow::SplitWith);
        let y = (heading[1] + heading[3]) / 2.0;
        unbroken(&roomy, parent[2] - 8.0, child[0] + 8.0, y);

        // The same menu opened hard against the window's right edge: no room on
        // that side, so it flips — and meets the parent the same way over there.
        let cramped = pane_menu_layout(
            [1500.0, 120.0],
            (1600.0, 900.0),
            1.0,
            Some(PaneMenuRow::SplitWith),
            false,
            &[],
            &chord_table(),
            &mut fake_measure,
        );
        let parent = cramped.frame;
        let child = cramped
            .submenu_frame()
            .expect("an open submenu has a frame");
        assert!(
            child[0] < parent[0],
            "with no room on the right the child flips to the left of the parent: \
             {parent:?} against {child:?}"
        );
        assert_eq!(
            child[2],
            parent[0] + border,
            "and stands on the parent's border on that side too"
        );
        let heading = cramped.item(PaneMenuRow::SplitWith);
        let y = (heading[1] + heading[3]) / 2.0;
        unbroken(&cramped, child[2] - 8.0, parent[0] + 8.0, y);
    }

    /// PIN — **the join survives every scale**, which is where a seam expressed
    /// in logical pixels and then rounded would quietly re-open.
    ///
    /// The overlap is one *physical* border, and rounding either frame is free to
    /// move an edge half a pixel. The claim is therefore the one that matters
    /// rather than the arithmetic: on whichever side the child took, the two
    /// rectangles share at least one whole physical column, so the union stays
    /// continuous at 100%, 125%, 150% and 200%.
    #[test]
    fn the_join_holds_at_every_scale() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let layout = pane_menu_layout(
                [300.0 * scale, 120.0 * scale],
                (1600.0 * scale, 900.0 * scale),
                scale,
                Some(PaneMenuRow::SplitWith),
                false,
                &[],
                &chord_table(),
                &mut fake_measure,
            );
            let parent = layout.frame;
            let child = layout.submenu_frame().expect("an open submenu has a frame");
            let shared = child[0].max(parent[0]).min(child[2].min(parent[2]));
            let overlap = (child[2].min(parent[2]) - child[0].max(parent[0])).max(0.0);
            assert!(
                overlap >= 1.0,
                "scale {scale}: the two frames share {overlap}px at x={shared} — \
                 anything under one physical pixel is a column that belongs to \
                 neither: {parent:?} against {child:?}"
            );
            let heading = layout.item(PaneMenuRow::SplitWith);
            let y = (heading[1] + heading[3]) / 2.0;
            let mut at = parent[2] - 4.0 * scale;
            while at <= child[0] + 4.0 * scale {
                assert!(
                    layout.contains(at, y),
                    "scale {scale}: x={at} is on neither surface"
                );
                at += 0.25;
            }
        }
    }

    /// PIN (user report 2026-08-19, second cause) — **the child's own border and
    /// padding belong to the child**, even though the hit test calls them
    /// `Surface` exactly as it calls the parent's.
    ///
    /// Measured on the machine before the fix: a pointer that hopped from the
    /// `Split with` row and landed anywhere in the child's leading five logical
    /// pixels shut the child *on arrival*. The chain is short and each link is
    /// reasonable on its own — `pane_menu_hit` answers `Surface` for a point on
    /// the child that is not on a row; the caller read `Surface` as "not on the
    /// child"; so it asked the safety triangle, whose base is the child's own
    /// left edge; and a pointer already inside the child is *past* that base,
    /// which is the one thing the triangle reads as "aiming somewhere else".
    ///
    /// So the question "is this the child's pixel" is asked of the child's
    /// frame, and this pins the gap between the two answers: every point in the
    /// leading strip is `on_submenu` while `pane_menu_hit` still says `Surface`.
    ///
    /// Red gate: make `on_submenu` delegate to `pane_menu_hit`'s `Submenu` arm
    /// and the leading-strip case goes red — which is the machine's failure,
    /// restored.
    #[test]
    fn the_submenus_own_border_and_padding_are_the_submenus() {
        let layout = pane_menu_layout(
            [300.0, 120.0],
            (1600.0, 900.0),
            1.0,
            Some(PaneMenuRow::SplitWith),
            false,
            &[],
            &chord_table(),
            &mut fake_measure,
        );
        let child = layout.submenu_frame().expect("an open submenu has a frame");
        let rows = layout
            .submenu_rows()
            .expect("an open submenu has rows to press");
        let inside_y = (rows[0][1] + rows[0][3]) / 2.0;

        // The leading strip: from the child's own edge up to where its first
        // column of content begins. Every pixel of it is the child's.
        let mut at = child[0];
        let mut saw_surface = false;
        while at < rows[0][0] {
            assert!(
                layout.on_submenu(at, inside_y),
                "x={at} is inside the child's frame {child:?} and must be the child's"
            );
            if matches!(
                pane_menu_hit(&layout, f64::from(at), f64::from(inside_y)),
                Some(PaneMenuHit::Surface)
            ) {
                saw_surface = true;
            }
            at += 0.5;
        }
        assert!(
            saw_surface,
            "the strip this pin is about is exactly the one the hit test calls \
             `Surface`; if that stopped being true the pin has lost its subject"
        );

        // And the claim has a floor: a point outside the frame is not the
        // child's, however close it stands.
        assert!(!layout.on_submenu(child[0] - 1.0, inside_y));
        assert!(!layout.on_submenu(child[2] + 1.0, inside_y));
        assert!(!layout.on_submenu(child[0] + 4.0, child[1] - 1.0));
        // A menu with no child open has no pixels that are the child's.
        let alone = pane_menu_layout(
            [300.0, 120.0],
            (1600.0, 900.0),
            1.0,
            None,
            false,
            &[],
            &chord_table(),
            &mut fake_measure,
        );
        assert!(!alone.on_submenu(child[0] + 4.0, inside_y));
    }

    /// PIN — **the diagonal is held one level up too**: a hand cutting across the
    /// parent's other rows toward the child is still *this menu's* hand, even
    /// where it is over neither rectangle.
    ///
    /// The seam fixes the straight crossing; this is the other half. The pane
    /// chevron's leave grace is driven by [`PaneMenuLayout::holds`] rather than
    /// by [`PaneMenuLayout::contains`] for exactly this trip — a child row below
    /// the parent's own bottom edge is reached by leaving both boxes — and
    /// without it the 150ms would start against a hand the submenu is already
    /// holding open.
    ///
    /// Red gate: make `holds` delegate to `contains` and the aimed case goes red
    /// while the abandoning case stays green, which is the pair that says the
    /// rule is about direction and not about slack.
    #[test]
    fn the_menu_keeps_a_hand_that_is_still_aiming_at_its_child() {
        let layout = pane_menu_layout(
            [300.0, 120.0],
            (1600.0, 900.0),
            1.0,
            Some(PaneMenuRow::SplitWith),
            false,
            &[],
            &chord_table(),
            &mut fake_measure,
        );
        let parent = layout.frame;
        let child = layout.submenu_frame().expect("an open submenu has a frame");
        let heading = layout.item(PaneMenuRow::SplitWith);
        let from = [heading[2] - 1.0, (heading[1] + heading[3]) / 2.0];

        // A point under both boxes' feet but inside the fan aimed at the child's
        // near edge. Off the parent, off the child, and still the menu's.
        let aimed = [(parent[2] + child[0]) / 2.0, child[3].max(parent[3]) - 1.0];
        if !layout.contains(aimed[0], aimed[1]) {
            assert!(
                layout.holds(Some(from), aimed),
                "a hand between the two surfaces, aimed at the child, has not left \
                 the menu: from {from:?} to {aimed:?}, parent {parent:?}, child {child:?}"
            );
        }

        // And the same point reached from the *other* side — a hand travelling
        // away from the child rather than toward it — is a hand that has gone.
        let leaving = [child[2] + 40.0, child[3] + 40.0];
        assert!(
            !layout.holds(Some(from), leaving),
            "and a hand that has walked off the far corner has left it: {leaving:?}"
        );

        // With no previous position there is no direction to read, and the
        // rectangles answer alone.
        assert!(!layout.holds(None, leaving));
        assert!(layout.holds(None, [child[0] + 4.0, child[1] + 4.0]));
    }

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

        let layers = pane_menu_build(&layout, None, Some(1), &equipped(), &[], &mut fake_measure);
        let [_parent, child]: [OverlayLayer; 2] = layers
            .try_into()
            .expect("a menu with a child draws two layers");
        let names: Vec<&str> = child
            .labels
            .iter()
            .map(|label| label.text.as_str())
            .filter(|text| *text != current_profile_hint_text())
            .collect();
        assert_eq!(
            names,
            (0..count()).map(title).collect::<Vec<_>>(),
            "every profile the strip offers, in the strip's own order"
        );
        assert_eq!(
            child
                .labels
                .iter()
                .filter(|label| label.text == current_profile_hint_text())
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
            shipped_five()
                .iter()
                .map(|profile| profile.mark)
                .collect::<Vec<_>>(),
            "each row wears its own profile's mark"
        );

        // A press on a submenu row is answered as that profile, and the child
        // wins the pixels where the two frames overlap.
        for index in 0..count() {
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
        let layers = pane_menu_build(&layout, None, Some(0), &bare(), &[], &mut fake_measure);
        let child = layers.last().expect("the child layer");
        assert_eq!(
            child
                .labels
                .iter()
                .filter(|l| l.text != current_profile_hint_text())
                .count(),
            count(),
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

    // ── the window-control marks in a menu row (rulings 2026-08-16, -19) ────

    /// PIN — **every edge-to-edge mark in a menu row is struck in the slot's
    /// smaller box, and every house mark takes the slot's own**, in whichever
    /// menu it appears.
    ///
    /// A deliberate deviation from the mock-up, whose title-bar symbols run edge
    /// to edge of their own `viewBox`: struck at the column's full width they
    /// out-weigh the folder and the copy glyph beside them, which are shapes
    /// drawn inside a box with a unit and a half of margin of their own. See
    /// [`item_mark_box_logical_px`], and `icons::MarkSlot` for the derivation.
    ///
    /// Red gate: apply the rule to `TabClose` alone and a menu that reaches for
    /// `PaneClose` — the same drawing under another name — gets the heavy cross
    /// back with nothing to say so. Apply it to the crosses alone and
    /// `Enter focus mode`'s `#i-max` comes back a third bigger than the rows
    /// around it, which is the 2026-08-19 report; apply it to the *names* rather
    /// than to the geometry and `#i-minus` never joins at all, which is the
    /// 2026-08-25 audit's P0.
    #[test]
    fn a_menu_rows_edge_to_edge_marks_are_struck_smaller_than_the_house_ones() {
        let house = crate::icons::MarkSlot::Menu.house_box_logical_px();
        let edge = crate::icons::MarkSlot::Menu.edge_to_edge_box_logical_px();
        assert!(edge < house, "the edge-to-edge family is given less room");
        // **Three, and two rulings are why it is not nine.** The three the
        // enumerated list never reached — `#i-minus`, `#i-chev`, the grip —
        // were edge-to-edge because they were cut in somebody else's box, which
        // is also what cost them their pen; re-cut into the house's sixteen
        // they carry the house's air and take the house's box, and so does
        // `#i-plus` beside them (P1). 裁2 took the tab's and the pane's cross
        // off the same way. What is left is the caption family, whose ten is
        // the platform's and not this design's.
        for edge_to_edge in [
            ChromeMark::WindowClose,
            ChromeMark::WindowMinimize,
            ChromeMark::WindowMaximize,
        ] {
            assert_eq!(
                item_mark_box_logical_px(edge_to_edge)[0],
                edge,
                "{edge_to_edge:?} is drawn to the edges of its own box"
            );
        }
        for other in [
            ChromeMark::Plus,
            ChromeMark::Minus,
            ChromeMark::chevron(0.0),
            ChromeMark::ResizeGrip,
            ChromeMark::Folder,
            ChromeMark::Copy,
            ChromeMark::Split,
            ChromeMark::SplitRight,
            ChromeMark::SplitDown,
            ChromeMark::Float,
            // A ten-unit box and *not* an edge-to-edge drawing: the disclosure
            // triangle carries more air than the house does, so shrinking it
            // would have shrunk it twice.
            crate::marks::tree_disclosure(0.0),
        ] {
            assert_eq!(
                item_mark_box_logical_px(other)[0],
                house,
                "{other:?} keeps the column's own size"
            );
        }

        // And it is true of the drawing, not merely of the table: read the whole
        // menu's icon column back and every row is in the box its family gets.
        // (`Enter focus mode` — the 2026-08-19 report's subject — was removed
        // from this menu the same day by the doors ruling; the ten-wide family
        // law it prompted survives it, checked on the `×` and the table above.)
        let layout = pane_menu(false);
        let layer = one_layer(pane_menu_build(
            &layout,
            None,
            None,
            &equipped(),
            &[],
            &mut fake_measure,
        ));
        let sprite_of = |glyph: ChromeMark| {
            let sprite = layer
                .sprites
                .iter()
                .find(|sprite| sprite.mark == glyph)
                .unwrap_or_else(|| panic!("{glyph:?} is one of this menu's rows"));
            sprite.rect
        };
        // **The cross is a house mark now** (裁2, 2026-08-26): `#i-cross` is
        // cut in the house's sixteen, so it takes the house's box and puts its
        // smaller picture inside it. The edge-to-edge box above is the caption
        // family's alone.
        let cross = sprite_of(ChromeMark::PaneClose);
        // A menu row *about* a folder, so the struck rendition.
        let folder = sprite_of(ChromeMark::FolderOutline);
        assert_eq!(cross[2] - cross[0], house);
        assert_eq!(folder[2] - folder[0], house);
        for (glyph, wanted) in [
            (ChromeMark::Split, house),
            // `Duplicate pane` since P1: it wore `#i-copy`, which is two sheets
            // of paper and means *put this text on the clipboard*. And `Move to
            // new tab`, which wore the files head's `#i-float`.
            (ChromeMark::Duplicate, house),
            (ChromeMark::TabNew, house),
        ] {
            let rect = sprite_of(glyph);
            assert_eq!(
                rect[2] - rect[0],
                wanted,
                "{glyph:?} is struck in its family's box"
            );
            assert_eq!(rect[3] - rect[1], wanted, "{glyph:?} is square");
        }
        // Within half a pixel of each other, which is as centred as two boxes of
        // different parity can be: both are snapped to whole device pixels — a
        // mark on a subpixel is a resampled mark — and two boxes of unequal
        // parity cannot both land symmetrically on the same column. Half a pixel
        // is the rounding, not a drift.
        let centre = |rect: [f32; 4]| (rect[0] + rect[2]) / 2.0;
        assert!(
            (centre(cross) - centre(folder)).abs() <= 0.5,
            "both are centred in the one 14px column, so the names line up:              {} against {}",
            centre(cross),
            centre(folder),
        );
    }

    // ── a tab's own context menu (丙2) ──────────────────────────────────────

    /// The other windows this menu's third exit has to choose between.
    ///
    /// **Two of them, and not none**, on the pane menu's own reasoning: with no
    /// second window there is no `Move to window ▸` row at all, and a pin about
    /// the menu's shape that quietly stood on the *shorter* menu would stop
    /// saying anything about the longer one. The short menu has a pin of its own.
    fn tab_menu_windows() -> Vec<String> {
        vec![
            "Window 2 · 3 tabs".to_owned(),
            "Window 3 · 1 tab".to_owned(),
        ]
    }

    /// A tab menu in a 960x600 window at 1x, raised well clear of every edge so
    /// that nothing below is a claim about the clamp.
    fn tab_menu(subject: TabMenuSubject, submenu_open: bool, windows: &[String]) -> TabMenuLayout {
        tab_menu_layout(
            [300.0, 120.0],
            (960.0, 600.0),
            1.0,
            subject,
            submenu_open,
            windows,
            &mut fake_measure,
        )
    }

    /// Where one row landed, **by name**.
    ///
    /// `SettingsLayout::row`'s arrangement, for that function's reason: a pin
    /// that indexed the item list directly would be a pin that silently moved to
    /// the row below the day a verb was inserted above it.
    fn tab_menu_box(layout: &TabMenuLayout, row: TabMenuRow) -> [f32; 4] {
        layout
            .items()
            .iter()
            .find(|item| item.row == row)
            .expect("every row this menu is showing has a box")
            .rect
    }

    /// The middle of a rectangle, as a hit test is asked about it.
    fn tab_menu_point(rect: [f32; 4]) -> (f64, f64) {
        (
            f64::from((rect[0] + rect[2]) / 2.0),
            f64::from((rect[1] + rect[3]) / 2.0),
        )
    }

    /// The words a menu actually draws, in the order it draws them.
    fn tab_menu_captions(layout: &TabMenuLayout, windows: &[String]) -> Vec<String> {
        one_layer(tab_menu_build(
            layout,
            None,
            windows,
            &equipped(),
            &mut fake_measure,
        ))
        .labels
        .iter()
        .map(|label| label.text.clone())
        .collect()
    }

    /// PIN (丙2, the gesture audit of 2026-08-26) — **the tab menu is six verbs
    /// about one tab, in this order, with a rule above the one that ends it.**
    ///
    /// The order is an argument rather than a habit. `Rename` leads because it
    /// is the only entry that changes nothing but the words, so a reader scanning
    /// down meets the harmless row first. The three that follow make or move a
    /// tab, in the order of how far they move it — nowhere, into a window that
    /// does not exist yet, into one that does — which is the same sentence at
    /// three lengths the pane menu's own exits are read at. `Close tab` is last
    /// and behind the rule, because it is where the sentence changes.
    ///
    /// **`Move to window ▸` is the pane menu's own string**, asserted here rather
    /// than merely reused in the source: two literals reading `Move to window`
    /// would be two menus that can come to call a window two different things,
    /// and nothing but a test would notice the day one of them was edited.
    ///
    /// Mutations: reorder `TabMenuRow::ALL` and the first two assertions
    /// disagree; strike the rule after `Close` instead of above it, or key it to
    /// the row above, and the third goes red on the menu that has no
    /// `Move to window ▸`.
    #[test]
    fn the_tab_menu_is_six_verbs_about_one_tab_with_a_rule_above_the_close() {
        assert_eq!(
            TabMenuRow::ALL,
            [
                TabMenuRow::Rename,
                TabMenuRow::Pin,
                TabMenuRow::Duplicate,
                TabMenuRow::MoveToNewWindow,
                TabMenuRow::MoveToWindow,
                TabMenuRow::Close,
            ]
        );
        let windows = tab_menu_windows();
        let layout = tab_menu(TabMenuSubject::default(), false, &windows);
        assert_eq!(
            tab_menu_captions(&layout, &windows),
            vec![
                tab_menu_rename_text(),
                tab_menu_pin_text(),
                tab_menu_duplicate_text(),
                tab_menu_move_to_new_window_text(),
                move_to_window_text(),
                tab_menu_close_text(),
            ],
            "six rows, in the order the enum lists them, and no heading over them"
        );
        assert_eq!(
            move_to_window_text(),
            crate::i18n::Text::PaneMenuMoveToWindow.text(),
            "the third exit borrows the pane menu's own words for the same list \
             of the same windows"
        );
        assert_ne!(
            tab_menu_move_to_new_window_text(),
            move_to_window_text(),
            "and the two exits differ by the window they name — including by not \
             being the same string"
        );

        // The rule falls between the last constructive verb and `Close tab`, on
        // the long menu and on the short one, which is the whole point of
        // striking it at the row it stands *above*.
        for windows in [tab_menu_windows(), Vec::new()] {
            let layout = tab_menu(TabMenuSubject::default(), false, &windows);
            let close = tab_menu_box(&layout, TabMenuRow::Close);
            let above = layout
                .items()
                .iter()
                .rev()
                .nth(1)
                .expect("a menu with a `Close tab` has a row above it")
                .rect;
            assert!(
                layout.separator[1] >= above[3] && layout.separator[3] <= close[1],
                "{} window(s): the rule lies between the last constructive verb \
                 and `Close tab`",
                windows.len()
            );
            assert_eq!(
                layout.items()[0].rect[1],
                layout.frame[1] + FLOAT_WINDOW_BORDER_LOGICAL_PX.max(1.0) + MENU_PADDING_LOGICAL_PX,
                "and the first row sits directly under the menu's own padding"
            );
        }
    }

    /// PIN (丙2; the pane menu's ruling of 2026-08-25, read on a tab) — **with
    /// nowhere to go, `Move to window ▸` is not in the menu at all.**
    ///
    /// A disclosure arrow over nothing is the one refusal this window may not
    /// make: the arrow's whole sentence is "there is more this way", and on a
    /// session with one window there is not. Nothing is lost by its going —
    /// `Move tab to new window` directly above is the verb for exactly that case,
    /// and this row was added beside it rather than instead of it.
    ///
    /// Asserted four ways over, because a row can be absent from a list and still
    /// be on the glass: the list, the drawn captions, the frame's own height —
    /// which is the one that catches a row left out of the walk but still paid
    /// for in pixels — and the child, which must not be hung off a heading that
    /// is not there.
    ///
    /// Mutations: return `ALL` unconditionally from `TabMenuRow::rows` and the
    /// first three assertions go red; drop the `rows.contains` filter on the
    /// child and the fourth hangs a list off nothing.
    #[test]
    fn a_lone_window_offers_no_row_for_moving_a_tab_into_another() {
        let alone = tab_menu(TabMenuSubject::default(), false, &[]);
        let rows: Vec<TabMenuRow> = alone.items().iter().map(|item| item.row).collect();
        assert_eq!(
            rows,
            vec![
                TabMenuRow::Rename,
                TabMenuRow::Pin,
                TabMenuRow::Duplicate,
                TabMenuRow::MoveToNewWindow,
                TabMenuRow::Close,
            ],
            "a lone window draws no row for moving a tab into another one"
        );
        assert!(
            !tab_menu_captions(&alone, &[]).contains(&move_to_window_text().to_owned()),
            "and it is not drawn either"
        );

        let windows = tab_menu_windows();
        let crowd = tab_menu(TabMenuSubject::default(), false, &windows);
        assert!(
            crowd
                .items()
                .iter()
                .any(|item| item.row == TabMenuRow::MoveToWindow),
            "with somewhere to go the row is back"
        );
        assert!(
            alone.frame[3] - alone.frame[1] < crowd.frame[3] - crowd.frame[1],
            "and the shorter menu is shorter — a row left out of the walk but \
             still paid for in pixels is a menu with a stripe of window in it"
        );

        // A child asked for on a menu with no such row is no child: the state
        // that says the list is open outlives one frame, so a window closing
        // while it stood open must not leave this reaching for a box that is not
        // there.
        let asked = tab_menu(TabMenuSubject::default(), true, &[]);
        assert!(
            asked.submenu_rows().is_none_or(<[[f32; 4]]>::is_empty),
            "a list of nowhere to go is not a list"
        );
        let open = tab_menu(TabMenuSubject::default(), true, &windows);
        let child = open.submenu_rows().expect("the child is up");
        assert_eq!(child.len(), windows.len(), "one row per other window");
        assert!(
            child[0][1] <= tab_menu_box(&open, TabMenuRow::MoveToWindow)[3],
            "and it hangs off its own heading"
        );
    }

    /// PIN (丙2) — **the hit test answers each row's own box, and refuses the row
    /// that cannot do what it says.**
    ///
    /// The refusal is the half worth pinning. `Duplicate tab` on a tab made of a
    /// folder or a file (§7.1.6h) has nothing to copy — no profile, no working
    /// folder — so the row is drawn and *not offered*: the pointer falls through
    /// it onto the menu's own body, and the keyboard steps over it. A row that
    /// lit under the hand and then did nothing would be worse than one that says
    /// so, and a row that vanished would be a menu whose shape moves.
    ///
    /// Mutations: drop `item.available &&` from [`tab_menu_hit`] and the greyed
    /// row starts answering presses; drop the filter from [`tab_menu_step`] and
    /// the walk stops on it.
    #[test]
    fn the_tab_menu_answers_each_row_and_refuses_the_one_that_cannot() {
        let windows = tab_menu_windows();
        let layout = tab_menu(TabMenuSubject::default(), false, &windows);
        for item in layout.items() {
            let (x, y) = tab_menu_point(item.rect);
            assert_eq!(
                tab_menu_hit(&layout, x, y),
                Some(TabMenuHit::Row(item.row)),
                "{:?} answers a press in its own box",
                item.row
            );
        }
        assert_eq!(
            tab_menu_hit(
                &layout,
                f64::from(layout.frame[0] + 1.0),
                f64::from(layout.frame[1] + 1.0)
            ),
            Some(TabMenuHit::Surface),
            "the menu's own padding swallows"
        );
        assert_eq!(
            tab_menu_hit(
                &layout,
                f64::from(layout.frame[0] - 4.0),
                f64::from(layout.frame[1] - 4.0)
            ),
            None,
            "and a point outside it is not this menu's at all"
        );

        // A tab with no shell behind it.
        let folder = tab_menu(
            TabMenuSubject {
                can_duplicate: false,
                ..TabMenuSubject::default()
            },
            false,
            &windows,
        );
        let greyed = tab_menu_box(&folder, TabMenuRow::Duplicate);
        let (x, y) = tab_menu_point(greyed);
        assert_eq!(
            tab_menu_hit(&folder, x, y),
            Some(TabMenuHit::Surface),
            "a tab with no shell has nothing to duplicate, and the row does not \
             answer"
        );
        assert!(
            folder
                .items()
                .iter()
                .any(|item| item.row == TabMenuRow::Duplicate && !item.available),
            "it is drawn, and drawn as unavailable"
        );
        assert_eq!(
            tab_menu_step(Some(TabMenuRow::Pin), true, folder.items()),
            Some(TabMenuRow::MoveToNewWindow),
            "and the keyboard steps over it rather than landing on it"
        );
        assert_eq!(
            tab_menu_step(Some(TabMenuRow::Pin), true, layout.items()),
            Some(TabMenuRow::Duplicate),
            "while on a tab that has a shell it is the very next row"
        );
        assert_eq!(
            tab_menu_step(None, false, layout.items()),
            Some(TabMenuRow::Close),
            "an empty highlight enters the list at whichever end the key names"
        );
        assert_eq!(
            tab_menu_step(Some(TabMenuRow::Close), true, layout.items()),
            Some(TabMenuRow::Close),
            "and the walk clamps at its ends"
        );

        // The child's rows answer by their place on the glass, which is what
        // `submenu_row` turns back into a window.
        let open = tab_menu(TabMenuSubject::default(), true, &windows);
        let child = open.submenu_rows().expect("the child is up").to_vec();
        let (x, y) = tab_menu_point(child[1]);
        assert_eq!(
            tab_menu_hit(&open, x, y),
            Some(TabMenuHit::Submenu(1)),
            "a press on a child row answers with its place in the list it was \
             drawn from"
        );
        assert_eq!(open.submenu_row(1), Some(1));
    }

    /// PIN (丙2; the `Lock` ruling, §7.1.6c-8) — **the pin row's word and its
    /// drawing turn together, and nothing else in the menu turns at all.**
    ///
    /// Change only the word and the drawing goes on lying; change only the
    /// drawing and the reader has to decide which half to believe. The row is
    /// [`PaneMenuRow::ZoomPane`]'s arrangement on this menu's own subject, and
    /// the fill is where the state rides — `ChromeMark::Pin`'s standing rule,
    /// which is the tab strip's rule and now this row's.
    ///
    /// Both halves are asserted on the *glass* as well as on the enum, because a
    /// `mark_when` that answered correctly while the painter reached for
    /// `ActionIcon`'s stateless face would pass the first half and draw the
    /// wrong pin.
    ///
    /// Mutations: return the unpinned mark from both arms and the sprite counts
    /// go red; make any other row consult the subject and the loop names it.
    #[test]
    fn the_tab_menus_pin_row_changes_its_word_and_its_mark_together() {
        let unpinned = TabMenuSubject::default();
        let pinned = TabMenuSubject {
            pinned: true,
            ..TabMenuSubject::default()
        };
        assert_eq!(TabMenuRow::Pin.text_when(unpinned), tab_menu_pin_text());
        assert_eq!(TabMenuRow::Pin.text_when(pinned), tab_menu_unpin_text());
        assert_ne!(
            tab_menu_pin_text(),
            tab_menu_unpin_text(),
            "two faces, two words"
        );
        assert_eq!(
            TabMenuRow::Pin.mark_when(unpinned),
            ChromeMark::Pin { filled: false }
        );
        assert_eq!(
            TabMenuRow::Pin.mark_when(pinned),
            ChromeMark::Pin { filled: true }
        );
        for row in TabMenuRow::ALL {
            if row == TabMenuRow::Pin {
                continue;
            }
            assert_eq!(
                row.text_when(unpinned),
                row.text_when(pinned),
                "{row:?} says the same thing whatever the tab is"
            );
            assert_eq!(
                row.mark_when(unpinned),
                row.mark_when(pinned),
                "{row:?} draws the same thing whatever the tab is"
            );
        }

        let windows = tab_menu_windows();
        for subject in [unpinned, pinned] {
            let layout = tab_menu(subject, false, &windows);
            let layer = one_layer(tab_menu_build(
                &layout,
                None,
                &windows,
                &equipped(),
                &mut fake_measure,
            ));
            assert!(
                layer
                    .labels
                    .iter()
                    .any(|label| label.text == TabMenuRow::Pin.text_when(subject)),
                "pinned={}: the word on the glass is the face the subject names",
                subject.pinned
            );
            assert_eq!(
                layer
                    .sprites
                    .iter()
                    .filter(|sprite| sprite.mark
                        == ChromeMark::Pin {
                            filled: subject.pinned
                        })
                    .count(),
                1,
                "pinned={}: and so is the drawing",
                subject.pinned
            );
            assert!(
                !layer.sprites.iter().any(|sprite| sprite.mark
                    == ChromeMark::Pin {
                        filled: !subject.pinned
                    }),
                "pinned={}: with no second pin anywhere in the list",
                subject.pinned
            );
        }
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
            git_filter_remotes_text()
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

    /// PIN — the first row says where *this* menu's press leads, so the menu
    /// cannot promise a preview on the surface that is the preview.
    ///
    /// **Re-judged 2026-08-25.** The wording used to be the caller's, handed in
    /// as `open_text`, on the argument that only the caller knew where the row
    /// led. The subject knows, and it now carries two rulings' worth of
    /// difference — a different first row, not only a different string — so the
    /// question is asked of [`file_menu`] and answered by [`FileMenuRow::text`].
    ///
    /// RED GATE: give both faces the same first row, or the same first wording.
    #[test]
    fn the_first_row_says_where_this_particular_press_is_going() {
        for (subject, first, says) in [
            (
                FileMenuSubject::File,
                FileMenuRow::Open,
                crate::i18n::Text::FileMenuOpenPreview.text(),
            ),
            (
                FileMenuSubject::Document,
                FileMenuRow::OpenWith,
                crate::i18n::Text::FileMenuOpenDefaultApp.text(),
            ),
        ] {
            let look = plain_look(subject);
            let layout = file_menu_layout(
                [300.0, 200.0],
                (960.0, 600.0),
                1.0,
                &look,
                &mut fake_measure,
            );
            let layer = one_layer(file_menu_build(&layout, &look, None));
            assert_eq!(file_menu(subject).rows[0], first);
            assert_eq!(
                layer.labels.first().map(|label| label.text.as_str()),
                Some(says)
            );
        }
    }

    /// PIN — a menu raised in the bottom-right corner is pulled whole back
    /// inside the window, on **both** axes.
    ///
    /// The red gate: the root menu clamps only horizontally, because a button on
    /// the top strip cannot be near the bottom. A file row can be — it is the
    /// last row of a tall column — and an unclamped drop puts every one of its
    /// verbs under the window's edge, where the menu is visible and unusable.
    #[test]
    fn a_menu_raised_in_the_corner_is_pulled_back_inside_on_both_axes() {
        let surface = (960.0, 600.0);
        // The tallest face this window has — a tree row's five — because the
        // clamp is only tested by the menu that has the most to fit.
        let look = plain_look(FileMenuSubject::File);
        let layout = file_menu_layout([950.0, 596.0], surface, 1.0, &look, &mut fake_measure);
        assert!(layout.frame[2] <= surface.0 - MENU_EDGE_MARGIN_LOGICAL_PX);
        assert!(layout.frame[3] <= surface.1 - MENU_EDGE_MARGIN_LOGICAL_PX);
        assert!(layout.frame[0] >= MENU_EDGE_MARGIN_LOGICAL_PX);
        assert!(layout.frame[1] >= MENU_EDGE_MARGIN_LOGICAL_PX);
        let last = *layout.items.last().expect("a menu has rows");
        assert_eq!(last.row, FileMenuRow::Reveal);
        assert!(
            file_menu_hit(
                &layout,
                f64::from(last.rect[0] + 1.0),
                f64::from((last.rect[1] + last.rect[3]) / 2.0),
            ) == Some(Some(last.row)),
            "and the row that would have fallen off is still the one that answers"
        );
    }

    /// PIN — rows answer, the body swallows, outside is nobody's; and the rule
    /// is body, so a press on a hairline commits no verb.
    ///
    /// Asked of **every** subject, because the hit test walks a list whose
    /// length depends on which row raised the menu.
    #[test]
    fn the_file_menu_answers_a_press_on_each_of_its_rows() {
        let crumbs = ["one".to_owned(), "two".to_owned()];
        for look in [
            plain_look(FileMenuSubject::File),
            plain_look(FileMenuSubject::Folder { expanded: true }),
            plain_look(FileMenuSubject::Document),
            FileMenuLook {
                subject: FileMenuSubject::FoldedPath {
                    levels: crumbs.len(),
                },
                crumbs: &crumbs,
                terminal: ChromeMark::ProfilePowerShell,
            },
        ] {
            let layout = file_menu_layout(
                [300.0, 200.0],
                (960.0, 600.0),
                1.0,
                &look,
                &mut fake_measure,
            );
            let middle = |rect: [f32; 4]| {
                (
                    f64::from((rect[0] + rect[2]) / 2.0),
                    f64::from((rect[1] + rect[3]) / 2.0),
                )
            };
            assert_eq!(layout.items.len(), file_menu(look.subject).rows.len());
            for item in &layout.items {
                let (x, y) = middle(item.rect);
                assert_eq!(file_menu_hit(&layout, x, y), Some(Some(item.row)));
            }
            // The rule is body, so a press on a hairline commits no verb — and
            // the one face with no verbs in it has no hairline to press.
            match layout.separator {
                Some(rule) => {
                    let (x, y) = middle(rule);
                    assert_eq!(file_menu_hit(&layout, x, y), Some(None));
                }
                None => assert!(matches!(look.subject, FileMenuSubject::FoldedPath { .. })),
            }
            assert_eq!(
                file_menu_hit(
                    &layout,
                    f64::from(layout.frame[0] - 4.0),
                    f64::from(layout.frame[1] - 4.0)
                ),
                None
            );
        }
    }

    /// PIN — the keyboard walk stops at both ends instead of wrapping round,
    /// which is the law the tree beside it already keeps (D45).
    ///
    /// **And it walks the list this menu is actually showing**: a folder's menu
    /// has no `Open`, a breadcrumb's has no `Reveal`, and the `…` chip's has
    /// nothing but places. A walk that stepped through a fixed table would offer
    /// a row that is not on the glass — a fold on a file, a folded level on a
    /// path that is not folded.
    ///
    /// RED GATE: hand [`file_menu_step`] one subject's rows for every subject.
    #[test]
    fn the_file_menus_keyboard_walk_clamps_at_both_ends_of_the_list_it_is_on() {
        let file = FileMenuSubject::File;
        let step = |subject, current, forwards| {
            file_menu_step(subject, current, forwards).expect("this menu has rows")
        };
        assert_eq!(step(file, None, true), FileMenuRow::Open);
        assert_eq!(step(file, None, false), FileMenuRow::Reveal);
        assert_eq!(
            step(file, Some(FileMenuRow::Open), false),
            FileMenuRow::Open,
            "up from the first row stays on the first row"
        );
        assert_eq!(
            step(file, Some(FileMenuRow::Reveal), true),
            FileMenuRow::Reveal,
            "and down from the last stays on the last"
        );
        assert_eq!(
            step(file, Some(FileMenuRow::Open), true),
            FileMenuRow::OpenWith
        );
        assert_eq!(
            step(file, Some(FileMenuRow::CopyPath), false),
            FileMenuRow::Rename,
            "B5 put a row between the doors and the path verbs"
        );

        let folder = FileMenuSubject::Folder { expanded: false };
        assert_eq!(step(folder, None, true), FileMenuRow::Fold);
        assert_eq!(step(folder, None, false), FileMenuRow::Reveal);
        assert_eq!(
            step(folder, Some(FileMenuRow::Fold), true),
            FileMenuRow::NewTerminal
        );
        assert_eq!(
            step(folder, Some(FileMenuRow::CopyPath), false),
            FileMenuRow::Rename
        );
        assert_eq!(
            step(folder, Some(FileMenuRow::Rename), false),
            FileMenuRow::NewTerminal,
            "and it steps over a row this subject does not have"
        );

        let document = FileMenuSubject::Document;
        assert_eq!(
            step(document, None, false),
            FileMenuRow::InsertPath,
            "the face with no Explorer row ends one row earlier"
        );
        assert_eq!(
            step(document, Some(FileMenuRow::Open), true),
            FileMenuRow::InsertPath,
            "a hover this face has not got does not wedge the walk: the step \
             lands at the end it was travelling towards"
        );
        assert_eq!(
            step(document, Some(FileMenuRow::OpenWith), true),
            FileMenuRow::Reveal,
            "and Explorer is this face's second row, not its last"
        );

        // The chip's list walks like any other, over rows that are places.
        let folded = FileMenuSubject::FoldedPath { levels: 3 };
        assert_eq!(step(folded, None, true), FileMenuRow::Crumb(0));
        assert_eq!(step(folded, None, false), FileMenuRow::Crumb(2));
        assert_eq!(
            step(folded, Some(FileMenuRow::Crumb(2)), true),
            FileMenuRow::Crumb(2)
        );
        assert_eq!(
            file_menu_step(FileMenuSubject::FoldedPath { levels: 0 }, None, true),
            None,
            "a menu with no rows has no row to step to — and does not panic"
        );

        // A hover left over from a menu that does not share this one's rows does
        // not wedge the walk: the step lands at the end it was travelling
        // towards, rather than at the end a fresh menu would have offered.
        assert_eq!(
            step(file, Some(FileMenuRow::Crumb(0)), true),
            FileMenuRow::Reveal
        );
        assert_eq!(
            step(file, Some(FileMenuRow::Fold), false),
            FileMenuRow::Open
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
        // **Every row wears a mark since P1.** `Find…` was the one empty cell
        // in the column, on the argument that the house had no magnifier —
        // which stopped being true the day it struck one.
        for row in TERM_MENU_ROWS {
            assert!(row.mark().is_some(), "{row:?} has no mark");
        }
        assert_eq!(R::Find.mark(), Some(ChromeMark::Search));
        // And `Restart shell…` is off the refresh: it throws a process away,
        // where rereading a repository and reloading a page fetch the same
        // thing again.
        assert_eq!(R::RestartShell.mark(), Some(ChromeMark::Restart));
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
    /// Clamped rather than cyclic, which is [`file_menu_step`]'s ruling and
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
        let row = TermMenuEntry::Term;
        let idle = TermMenuSubject::default();
        assert_eq!(term_menu_step(None, idle, true, false), Some(row(R::Paste)));
        assert_eq!(
            term_menu_step(None, idle, false, false),
            Some(row(R::RestartShell))
        );
        assert_eq!(
            term_menu_step(Some(row(R::Paste)), idle, false, false),
            Some(row(R::Paste)),
            "the top of the walk is the top of what is walkable"
        );
        assert_eq!(
            term_menu_step(Some(row(R::RestartShell)), idle, true, false),
            Some(row(R::RestartShell)),
            "and the bottom clamps rather than wrapping"
        );

        let mid_restart = TermMenuSubject {
            has_selection: true,
            restart_in_flight: true,
            ..idle
        };
        assert_eq!(
            term_menu_step(None, mid_restart, true, false),
            Some(row(R::Copy))
        );
        assert_eq!(
            term_menu_step(Some(row(R::ClearScrollback)), mid_restart, true, false),
            Some(row(R::ClearScrollback)),
            "a restart in flight makes Clear scrollback the last walkable row"
        );
        assert_eq!(
            term_menu_step(None, mid_restart, false, false),
            Some(row(R::ClearScrollback))
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
        let layout = term_menu_layout(
            [958.0, 599.0],
            surface,
            1.0,
            &look,
            &chord_table(),
            &mut fake_measure,
        );
        let frame = layout.frame;
        assert!(frame[2] <= surface.0 - MENU_EDGE_MARGIN_LOGICAL_PX);
        assert!(frame[3] <= surface.1 - MENU_EDGE_MARGIN_LOGICAL_PX);
        assert!(frame[0] >= MENU_EDGE_MARGIN_LOGICAL_PX);
        assert!(frame[1] >= MENU_EDGE_MARGIN_LOGICAL_PX);

        let last = *layout.items.last().expect("the last row is laid out");
        assert_eq!(last.entry, TermMenuEntry::Term(TermMenuRow::RestartShell));
        assert!(last.rect[3] <= frame[3], "the last row is inside the frame");
        assert_eq!(
            term_menu_hit(
                &layout,
                f64::from(last.rect[0] + 1.0),
                f64::from((last.rect[1] + last.rect[3]) / 2.0),
            ),
            Some(TermMenuHit::Row(TermMenuEntry::Term(
                TermMenuRow::RestartShell
            )))
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
            &chord_table(),
            &mut fake_measure,
        );
        let row = |wanted: TermMenuRow| {
            *layout
                .items
                .iter()
                .find(|item| item.entry == TermMenuEntry::Term(wanted))
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
            Some(TermMenuHit::Surface),
            "the pointer falls through a greyed row onto the menu's own body"
        );
        assert_eq!(
            term_menu_hit(&layout, 1.0, 1.0),
            None,
            "and misses the menu entirely outside it"
        );

        // Drawn all the same, which is the other half of "greyed, not hidden":
        // one label per entry, whatever each of them can answer.
        let layers = term_menu_build(&layout, &look, None, &equipped(), &mut fake_measure);
        assert_eq!(layers.len(), 1);
        let names: Vec<&str> = layers[0]
            .labels
            .iter()
            .map(|label| label.text.as_str())
            .collect();
        assert_eq!(
            names.iter().filter(|name| **name != "Ctrl+F").count(),
            TERM_MENU_ROWS.len(),
            "every row is painted, including the ones that cannot answer"
        );
        // **And one of the seven carries a chord** (gesture audit 2026-08-26,
        // 系统性发现 ②). `Find…` is the row of this menu that is also a row of
        // the shortcut table; `Copy` and `Paste` are decided above that table,
        // in `input`, so this column has nothing true to print beside them —
        // see [`TermMenuRow::accelerator`], which names all seven.
        assert_eq!(
            names.iter().filter(|name| **name == "Ctrl+F").count(),
            1,
            "`Find…` prints the chord that raises the same capsule"
        );
    }

    // ── §7.1.6i's floor: the lone pane's pane-verb segment ──────────────────

    /// PIN (§7.1.6i, 「两案共同、且不随选型摇摆的一件」) — **a lone pane's right
    /// click carries three pane verbs under a rule of their own, and a pane with
    /// a sibling carries none.**
    ///
    /// The three are `docs/DESIGN.md`'s own list — `Split with ▸` / `New
    /// terminal in folder…` / `Duplicate pane` — and they are asserted as
    /// [`PaneMenuRow`] values rather than as words, which is the whole claim: it
    /// is the `⌄` menu's own list borrowed, not a second list that agrees with
    /// it today. `Move pane to new tab` and `Close pane` are named in the
    /// negative because the ruling names them in the negative: on a lone pane
    /// the first has nothing to move and nowhere to move it, and the second
    /// closes the whole tab while saying it closes a pane.
    ///
    /// Red gate: the shipped menu is seven rows whatever the tree looks like, so
    /// the first assertion fails on the length.
    #[test]
    fn a_lone_panes_terminal_menu_carries_the_pane_verbs_and_a_split_one_does_not() {
        use PaneMenuRow as P;
        use TermMenuEntry as E;
        let lone = term_menu_entries(true);
        assert_eq!(
            lone,
            TERM_MENU_ROWS
                .into_iter()
                .map(E::Term)
                .chain([
                    E::Pane(P::SplitWith),
                    E::Pane(P::NewInFolder),
                    E::Pane(P::Duplicate),
                    E::Pane(P::MoveToNewWindow),
                ])
                .collect::<Vec<_>>(),
            "the seven terminal verbs, then the pane menu's three making verbs \
             and the one exit a lone pane can spend, in the pane menu's own order"
        );
        assert_eq!(
            term_menu_entries(false),
            TERM_MENU_ROWS.into_iter().map(E::Term).collect::<Vec<_>>(),
            "a pane with a sibling has its head eighteen pixels away, and a menu \
             that repeats it is where two lists start to disagree"
        );
        for cut in [P::MoveToNewTab, P::ClosePane] {
            assert!(
                !lone.contains(&E::Pane(cut)),
                "{cut:?} is ruled out of the segment by §7.1.6i"
            );
        }
        assert!(
            lone.contains(&E::Pane(P::MoveToNewWindow)),
            "and the sibling §7.1.6i named in the same breath is in it: a lone \
             pane has nowhere to go inside this window and a window of its own \
             to go to (F1c)"
        );
        // Borrowed whole: the words and the marks are the `⌄` menu's, asked of
        // the `⌄` menu's own row. A segment that spelled its own would be the
        // two-lists failure this test exists to forbid.
        for row in TERM_MENU_LONE_PANE_ROWS {
            assert_eq!(E::Pane(row).text(), row.text());
            assert_eq!(E::Pane(row).mark(), row.mark());
        }
        assert!(
            E::Pane(P::SplitWith).has_submenu(),
            "the segment's first row is a heading, exactly as it is in the head"
        );
    }

    /// PIN (§7.1.6i) — **the segment stands under a rule of its own, and the
    /// `Split with` heading hangs the profile list off itself.**
    ///
    /// The second rule is the load-bearing half: without it the three pane verbs
    /// run on from `Restart shell…` as though they were more things to do to a
    /// shell, and the menu stops saying that the list changed subject.
    ///
    /// The submenu is asserted through [`term_menu_hit`] rather than through its
    /// rectangle alone, because the child is drawn *over* the parent and a hit
    /// test that answered the row underneath would be a child nothing can press.
    ///
    /// Red gate: today there is no second rule, no heading and no child.
    #[test]
    fn the_lone_segment_stands_under_its_own_rule_and_opens_the_profile_list() {
        let look = TermMenuLook {
            lone: true,
            submenu_open: true,
            ..TermMenuLook::default()
        };
        let layout = term_menu_layout(
            [200.0, 120.0],
            (960.0, 600.0),
            1.0,
            &look,
            &chord_table(),
            &mut fake_measure,
        );
        let entry = |wanted: TermMenuEntry| {
            *layout
                .items
                .iter()
                .find(|item| item.entry == wanted)
                .expect("every entry is laid out")
        };
        let restart = entry(TermMenuEntry::Term(TermMenuRow::RestartShell));
        let heading = entry(TermMenuEntry::Pane(PaneMenuRow::SplitWith));
        let rule = layout
            .lone_separator
            .expect("the segment brings a rule of its own");
        assert!(
            restart.rect[3] <= rule[1] && rule[3] <= heading.rect[1],
            "the second rule stands between the shell's verbs and the pane's"
        );
        assert!(
            rule[1] > layout.separator[1],
            "and below the first, which is still where it always was"
        );

        let child = layout
            .submenu_frame()
            .expect("an open submenu has a frame of its own");
        assert!(
            child[0] >= heading.rect[0],
            "it hangs beside its heading, not under it"
        );

        for index in 0..count() {
            let rect = layout.submenu_rows().expect("an open submenu has rows")[index];
            assert_eq!(
                term_menu_hit(
                    &layout,
                    f64::from((rect[0] + rect[2]) / 2.0),
                    f64::from((rect[1] + rect[3]) / 2.0),
                ),
                Some(TermMenuHit::Submenu(index)),
                "the child wins the pixels where the two frames overlap"
            );
        }
        assert_eq!(
            term_menu_hit(
                &layout,
                f64::from(heading.rect[0] + 1.0),
                f64::from((heading.rect[1] + heading.rect[3]) / 2.0),
            ),
            Some(TermMenuHit::Row(TermMenuEntry::Pane(
                PaneMenuRow::SplitWith
            ))),
        );

        // Two layers, as the pane menu's own child draws: what covers what is a
        // statement rather than an accident of loop order.
        let layers = term_menu_build(&layout, &look, Some(0), &equipped(), &mut fake_measure);
        assert_eq!(layers.len(), 2);
    }

    /// PIN (§7.1.6i) — **the keyboard walks into the segment and stops at its
    /// end, and the pane verbs are never greyed.**
    ///
    /// The three borrowed rows carry `pane_menu_build`'s own availability
    /// argument with them: a split the solver has no room for is refused after
    /// the press, and the folder chooser is Windows' to answer for. Nothing here
    /// is a promise this build knows it cannot keep, so nothing here greys.
    ///
    /// Red gate: today the walk ends at `Restart shell…` on every tree.
    #[test]
    fn the_walk_runs_on_into_the_lone_segment_and_stops_at_its_last_row() {
        use PaneMenuRow as P;
        use TermMenuEntry as E;
        let idle = TermMenuSubject::default();
        assert_eq!(
            term_menu_step(None, idle, false, false),
            Some(E::Term(TermMenuRow::RestartShell)),
            "with no segment the walk still ends where it always did"
        );
        assert_eq!(
            term_menu_step(None, idle, false, true),
            Some(E::Pane(P::MoveToNewWindow)),
            "and with one it ends at the segment's last row"
        );
        assert_eq!(
            term_menu_step(Some(E::Term(TermMenuRow::RestartShell)), idle, true, true),
            Some(E::Pane(P::SplitWith)),
            "the rule is not a stop — the walk crosses it"
        );
        assert_eq!(
            term_menu_step(Some(E::Pane(P::Duplicate)), idle, true, true),
            Some(E::Pane(P::MoveToNewWindow)),
            "and the row F1c added is one more step down, not a stop"
        );
        assert_eq!(
            term_menu_step(Some(E::Pane(P::MoveToNewWindow)), idle, true, true),
            Some(E::Pane(P::MoveToNewWindow)),
            "and clamps at the bottom rather than wrapping"
        );
        assert_eq!(
            term_menu_step(Some(E::Pane(P::SplitWith)), idle, false, true),
            Some(E::Term(TermMenuRow::RestartShell)),
            "back across the rule the same way"
        );

        // Never greyed, whatever the pane is doing.
        for subject in [
            idle,
            TermMenuSubject {
                has_selection: true,
                restart_in_flight: true,
                can_search: false,
            },
        ] {
            for row in TERM_MENU_LONE_PANE_ROWS {
                assert!(term_menu_entry_available(E::Pane(row), subject));
            }
        }
    }

    // ── the table, the file and the page (§7.1.6c-6) ───────────────────────
    //
    // Every case below moves **its own** `Registry` rather than the process's,
    // which is `crate::i18n`'s ruling verbatim: `cargo test` runs this crate in
    // one process with many threads, and a case that reordered the window's own
    // table for a microsecond would race every other case that asks how many
    // profiles there are.

    /// One file entry, spelled the way `profiles.json` spells an untouched
    /// built-in.
    fn named(id: &str) -> ProfileEntryV1 {
        ProfileEntryV1 {
            id: id.to_owned(),
            ..ProfileEntryV1::default()
        }
    }

    fn file(entries: Vec<ProfileEntryV1>) -> ProfilesV1 {
        ProfilesV1 {
            schema_version: PROFILES_SCHEMA_VERSION,
            profiles: entries,
        }
    }

    fn ids(profiles: &[Profile]) -> Vec<&str> {
        profiles.iter().map(|profile| profile.id.as_str()).collect()
    }

    /// PIN — **a machine with no `profiles.json` is this window before this
    /// slice existed, row for row and byte for byte.**
    ///
    /// The first red gate of the whole slice, and the one everything else is
    /// allowed to build on: the table is data now, the file is optional, and the
    /// ordinary machine — which has no such file and never will — must not be
    /// able to tell. Nothing is written until something is changed, either: a
    /// feature does not announce itself by putting an empty document in
    /// everybody's `%APPDATA%` (`schemes.rs`'s own judgment).
    ///
    /// Red gate: seed the table from anything but `shipped()`, or let a missing
    /// file mean "empty table", and every surface in the window changes at once.
    #[test]
    fn a_machine_with_no_profiles_file_gets_the_shipped_five_unchanged() {
        let (built, faults) = merge(shipped(), &ProfilesV1::default());
        assert_eq!(built, shipped());
        assert!(faults.is_empty());
        assert_eq!(ids(&built), ["pwsh", "winps", "wsl", "gitbash", "cmd"]);
        assert!(
            built.iter().all(|profile| !profile.hidden),
            "and none of them arrives hidden"
        );
    }

    /// PIN — **the array is the order, and a built-in writes only what differs.**
    ///
    /// There is deliberately no `order` key: two places saying the same thing
    /// drift, and a JSON list already reads as an order to anybody editing it by
    /// hand. An entry that is nothing but an id is the shipped profile in that
    /// position, which is what lets a later build retune one for everybody who
    /// never touched it.
    ///
    /// Red gate: read the file as a set and the reorder does nothing; apply a
    /// bare entry as a blank profile and every untouched row loses its program.
    #[test]
    fn the_array_is_the_order_and_a_builtin_writes_only_its_differences() {
        let (built, faults) = merge(
            shipped(),
            &file(vec![
                named("cmd"),
                ProfileEntryV1 {
                    display_title: Some("Ubuntu".to_owned()),
                    ..named("wsl")
                },
                named("gitbash"),
                named("winps"),
                named("pwsh"),
            ]),
        );
        assert!(faults.is_empty());
        assert_eq!(ids(&built), ["cmd", "wsl", "gitbash", "winps", "pwsh"]);
        assert_eq!(built[1].display_title, "Ubuntu");
        assert_eq!(
            built[1].id, "wsl",
            "renaming a row does not rename what the disk calls it"
        );
        let shipped = shipped();
        for profile in &built {
            let seed = shipped
                .iter()
                .find(|seed| seed.id == profile.id)
                .expect("every row here is a built-in");
            assert_eq!(profile.program, seed.program, "{}", profile.id);
            assert_eq!(profile.args, seed.args, "{}", profile.id);
            assert_eq!(profile.integration, seed.integration, "{}", profile.id);
            assert_eq!(
                profile.compared_title, seed.compared_title,
                "{} keeps the word its script announces, renamed or not",
                profile.id
            );
        }
    }

    /// PIN — **a built-in this build ships and the file never mentions arrives
    /// at the end, visible.**
    ///
    /// The only honest answer to "the upgrade brought a sixth profile": it
    /// appears, last, not hidden. Dropping it would make a new shell invisible to
    /// everybody who has ever touched this dialog; inserting it at the top would
    /// move somebody's list around for a row they did not ask for.
    #[test]
    fn a_shipped_id_the_file_never_names_is_appended_and_visible() {
        let (built, faults) = merge(shipped(), &file(vec![named("cmd"), named("wsl")]));
        assert!(faults.is_empty());
        assert_eq!(ids(&built), ["cmd", "wsl", "pwsh", "winps", "gitbash"]);
        assert!(built[2..].iter().all(|profile| !profile.hidden));
    }

    /// PIN — **an entry that is neither a built-in nor a startable profile is
    /// dropped, named once, and takes nothing else with it.**
    ///
    /// `schemes`' register applied to a table: skip it, say which one, never
    /// crash, never go quiet. A row with no program is a row that cannot start,
    /// and a list with an unstartable row in it is a list that lies.
    #[test]
    fn an_entry_that_is_neither_a_builtin_nor_a_program_is_dropped_and_named() {
        let (built, faults) = merge(
            shipped(),
            &file(vec![named("pwsh"), named("fish"), named("cmd")]),
        );
        assert_eq!(
            faults,
            vec![ProfileFault::Unusable {
                id: "fish".to_owned()
            }]
        );
        assert_eq!(ids(&built), ["pwsh", "cmd", "winps", "wsl", "gitbash"]);
        assert!(
            crate::i18n::profile_entry_fault(&faults[0]).contains("fish"),
            "the sentence names the entry, because the id is what the reader has \
             to search their own file for"
        );
    }

    /// PIN — **the first entry to claim an id keeps it.**
    ///
    /// A stable id has to name one thing: the seeds already on disk pointing at
    /// it were written when only the first existed, so the first is the one they
    /// meant. The second is refused and named rather than silently merged, which
    /// would leave a file whose two halves disagree and no way to find out.
    #[test]
    fn a_second_entry_claiming_a_taken_id_is_refused_and_named() {
        let (built, faults) = merge(
            shipped(),
            &file(vec![
                ProfileEntryV1 {
                    display_title: Some("first".to_owned()),
                    ..named("pwsh")
                },
                ProfileEntryV1 {
                    display_title: Some("second".to_owned()),
                    ..named("pwsh")
                },
            ]),
        );
        assert_eq!(
            faults,
            vec![ProfileFault::Duplicate {
                id: "pwsh".to_owned()
            }]
        );
        assert_eq!(built[0].display_title, "first");
        assert_eq!(
            built.iter().filter(|profile| profile.id == "pwsh").count(),
            1
        );
    }

    /// PIN — **a hidden built-in leaves the pickers and stays a profile.**
    ///
    /// Hiding is the whole of what "I do not want to see this" can mean for a
    /// row that cannot be deleted, and it must not reach the seat that is
    /// already running it: a restart resolves through the seat's own
    /// `profile_id`, which is untouched by any of this.
    #[test]
    fn a_hidden_builtin_leaves_the_pickers_but_stays_a_profile() {
        let (built, _) = merge(
            shipped(),
            &file(vec![ProfileEntryV1 {
                hidden: true,
                ..named("cmd")
            }]),
        );
        let table = ProfileTable { profiles: built };
        assert_eq!(
            table
                .offered()
                .into_iter()
                .map(|index| table.profiles()[index].id.as_str())
                .collect::<Vec<_>>(),
            ["pwsh", "winps", "wsl", "gitbash"],
            "the `˅` menu, the `+` and the picker are all one list, and `cmd` is \
             not on it"
        );
        assert_eq!(
            table.position_of_id("cmd"),
            Some(0),
            "but a seat that names it still resolves to it"
        );
    }

    // ── the file changing under a running window (§7.1.6c-6d) ───────────────
    //
    // The watcher's own arithmetic is `dir_news`'s and the read is
    // `ProfilesStore::reread`'s; what is claimed here is what the *table* does
    // when a document written by somebody else is installed on top of it, which
    // is the half every surface in the window is drawn from.

    /// One profile of the reader's own, spelled the way their file spells it.
    fn mine(id: &str) -> ProfileEntryV1 {
        ProfileEntryV1 {
            display_title: Some(id.to_owned()),
            program: Some(ProgramV1::Path(r"C:\bin\claude.exe".to_owned())),
            ..named(id)
        }
    }

    /// PIN — **a row deleted in an editor leaves every list, and the default it
    /// was falls back to the built-in floor without the stored name being
    /// touched.**
    ///
    /// The two halves are one claim: the row goes because the array is the
    /// order and an entry that is not in it is not in the table, and the
    /// default goes to the floor because `default_profile` has answered "an id
    /// nothing holds" that way since the day the setting existed. No rule was
    /// invented for a deletion made on disk — a deletion made on disk is simply
    /// a file that no longer names it.
    ///
    /// And no card: `Undo` is the verb of a deletion *this window* performed,
    /// and an editor's deletion is a fact rather than an offer.
    ///
    /// Red gate: keep a row the file dropped, or rewrite `default_profile` when
    /// its subject disappears, and putting the entry back stops putting the
    /// default back.
    #[test]
    fn a_row_deleted_on_disk_leaves_the_table_and_the_default_falls_to_the_floor() {
        let registry = Registry::shipped();
        registry.install(&file(vec![named("pwsh"), mine("claude-7f3a")]));
        let before = registry.revision();
        let table = registry.table();
        let index = table
            .position_of_id("claude-7f3a")
            .expect("the reader's own row is in their file");
        assert_eq!(
            default_profile_in(&table, "claude-7f3a", |_| true),
            index,
            "while it is there it is what the `+` opens"
        );

        // Somebody opens `profiles.json` in an editor and deletes the entry.
        registry.install(&file(vec![named("pwsh")]));
        let after = registry.table();
        assert_eq!(after.position_of_id("claude-7f3a"), None, "the row is gone");
        assert!(
            registry.revision() > before,
            "and the window is told to re-measure every width its name decided"
        );
        assert_eq!(
            default_profile_in(&after, "claude-7f3a", |_| true),
            fallback_profile_in(&after),
            "the default falls to the floor rather than to nothing"
        );
        assert_eq!(
            after.get(fallback_profile_in(&after)).map(|row| &*row.id),
            Some(WINDOWS_POWERSHELL_ID),
            "and the floor is the built-in every machine has"
        );
    }

    /// PIN — **a table that moves under a running pane does not move the pane.**
    ///
    /// A profile is a birth certificate and not a contract: a session already
    /// running keeps the program and the environment it was started with,
    /// because they are in a process this window cannot re-argue with. What it
    /// *can* lose is its own name for itself — the pane holds an index, and an
    /// index into a table somebody is reordering in an editor is a pointer at
    /// whichever row slid into that slot. Asking by id across the change is the
    /// whole of the repair, and it is `index_of_id`'s existing rule rather than
    /// a second one.
    ///
    /// Red gate: keep the index across a rescan and reordering two lines in a
    /// text editor repaints the marks of panes that have been running for hours.
    #[test]
    fn a_seat_keeps_the_profile_it_was_born_from_when_the_file_is_reordered() {
        let registry = Registry::shipped();
        registry.install(&file(vec![
            named("pwsh"),
            mine("claude-7f3a"),
            named("cmd"),
        ]));
        let before = registry.table();
        let seat = before
            .position_of_id("claude-7f3a")
            .expect("the pane was born from the reader's own row");
        let born_from = before.get(seat).expect("a seat stands on a row").id.clone();

        // The same three rows, in another order — one drag in an editor.
        registry.install(&file(vec![
            mine("claude-7f3a"),
            named("pwsh"),
            named("cmd"),
        ]));
        let after = registry.table();
        assert_ne!(
            after.get(seat).map(|row| &*row.id),
            Some("claude-7f3a"),
            "the slot the pane was holding now belongs to somebody else, which \
             is the whole hazard"
        );
        assert_eq!(
            after
                .get(index_of_id_in(&after, &born_from))
                .map(|row| &*row.id),
            Some("claude-7f3a"),
            "the seat follows its own row rather than its old slot"
        );

        // And when the row is deleted outright, the standing answer applies: the
        // seat costs its shell choice, never the seat.
        registry.install(&file(vec![named("cmd"), named("pwsh")]));
        let after = registry.table();
        assert_eq!(
            index_of_id_in(&after, &born_from),
            fallback_profile_in(&after)
        );
    }

    /// PIN — **a document that lands twice is the same table twice**, which is
    /// what lets the watcher answer this window's own writes by comparing rather
    /// than by remembering who wrote them.
    ///
    /// `Registry::publish` already refuses to advance the revision for a table
    /// equal to the one in force; this reads that rule from the file's end,
    /// where it is what keeps a keystroke in the editor from throwing away every
    /// measured string in the window a moment after it was measured.
    #[test]
    fn the_same_document_installed_twice_moves_nothing() {
        let registry = Registry::shipped();
        registry.install(&file(vec![named("cmd"), mine("claude-7f3a")]));
        let table = registry.table();
        let revision = registry.revision();

        registry.install(&file(vec![named("cmd"), mine("claude-7f3a")]));
        assert_eq!(registry.table().profiles(), table.profiles());
        assert_eq!(registry.revision(), revision);
    }

    // ── the editor (§7.1.6c-6b) ──────────────────────────────────────────────

    /// PIN — **an edit reaches the file and the file reaches the table back**,
    /// which is the whole contract of a dialog with no Save button.
    ///
    /// Red gate: write the field into the table and not into the entry, and the
    /// rename survives exactly as long as the process does.
    #[test]
    fn an_edited_field_reaches_the_file_and_the_file_reaches_a_fresh_table() {
        let registry = Registry::shipped();
        assert_eq!(registry.rename(0, "Seven"), NameVerdict::Written);
        registry.edit(0, |profile| {
            profile.args = vec!["-NoLogo".to_owned(), "-NoProfile".to_owned()];
            true
        });

        let written = registry.to_file();
        let (read, faults) = merge(shipped(), &written);
        assert!(faults.is_empty(), "{faults:?}");
        assert_eq!(read[0].display_title, "Seven");
        assert_eq!(read[0].args, ["-NoLogo", "-NoProfile"]);
        assert_eq!(
            read[0].compared_title.as_deref(),
            Some("PowerShell 7"),
            "the byte-compared word is a protocol constant and is not renamed \
             with the row"
        );
        assert_eq!(read[0].id, "pwsh", "a rename is not a change of identity");
    }

    /// PIN — **a name another row already draws is refused, and nothing moves.**
    ///
    /// A row is told apart by its mark and its name, and two rows called
    /// `PowerShell 7` one line apart in a picker is the failure that made the two
    /// PowerShells carry their versions in the first place. The refusal is exact
    /// rather than fuzzy, because a rule a person cannot predict is worse than
    /// no rule: `powershell 7` is a different name and is allowed.
    #[test]
    fn a_name_another_row_already_draws_is_refused_and_the_table_does_not_move() {
        let registry = Registry::shipped();
        let before = registry.table().profiles().to_vec();

        assert_eq!(registry.rename(0, "Command Prompt"), NameVerdict::Taken);
        assert_eq!(registry.rename(0, "   "), NameVerdict::Blank);
        assert_eq!(registry.table().profiles(), before.as_slice());
        assert_eq!(registry.revision(), 0, "a refusal is not a change");

        assert_eq!(registry.rename(0, "command prompt"), NameVerdict::Written);
        assert_eq!(
            registry.rename(0, "PowerShell 7"),
            NameVerdict::Written,
            "a row may always be given the name it already had back"
        );
    }

    /// PIN — **the five identity colours are not this product's to repaint, and
    /// a profile of the reader's own is theirs** (S98/S31, user ruling
    /// 2026-08-17 Q5).
    ///
    /// Enforced in the table and not only in the dialog, because a rule that
    /// lives in a control is a rule a hand-edited file walks around.
    #[test]
    fn a_builtins_colour_is_refused_and_a_profile_of_your_own_can_be_repainted() {
        let registry = Registry::shipped();
        assert!(
            !registry.set_colour(0, MarkColour::Amber),
            "PowerShell's blue is Microsoft's"
        );
        assert_eq!(
            registry.table().get(0).unwrap().mark,
            ChromeMark::ProfilePowerShell
        );

        let copy = registry.duplicate(0).expect("pwsh is a row");
        assert_eq!(
            registry.table().get(copy).unwrap().mark,
            ChromeMark::ProfilePowerShell,
            "a copy of a PowerShell is a PowerShell, and the mark says so"
        );

        // And a copy of WSL carries no machine qualifier, before the file is
        // written or after it is read back — one row read twice, and a row that
        // renamed itself across a restart would be the table disagreeing with
        // itself.
        let wsl = registry.table().position_of_id("wsl").unwrap();
        let of_wsl = registry.duplicate(wsl).expect("wsl is a row");
        assert_eq!(
            registry.table().get(of_wsl).unwrap().qualifier,
            Qualifier::None
        );
        let (read, _) = merge(shipped(), &registry.to_file());
        assert_eq!(read[of_wsl].qualifier, Qualifier::None);

        assert!(registry.set_colour(copy, MarkColour::Amber));
        assert_eq!(
            registry.table().get(copy).unwrap().mark,
            ChromeMark::ProfileGeneric {
                colour: MarkColour::Amber
            }
        );

        // And it survives the file, which is where the object form of `mark`
        // earns its place.
        let (read, faults) = merge(shipped(), &registry.to_file());
        assert!(faults.is_empty(), "{faults:?}");
        assert_eq!(
            read[copy].mark,
            ChromeMark::ProfileGeneric {
                colour: MarkColour::Amber
            }
        );
    }

    /// PIN — **a deleted profile comes back with its own id, in its own place.**
    ///
    /// The id is the whole of what must survive: every seed on disk naming this
    /// profile points at that string, and a row rebuilt with a fresh suffix would
    /// leave all of them degraded to the floor. The position is what makes the
    /// undo look like an undo rather than like a second creation.
    #[test]
    fn a_deleted_profile_comes_back_with_its_own_id_in_its_own_place() {
        let registry = Registry::shipped();
        let copy = registry.duplicate(1).expect("winps is a row");
        let held = registry.table().get(copy).unwrap().clone();

        assert!(
            registry.delete(0).is_none(),
            "a built-in is hidden, never deleted"
        );
        let removed = registry.delete(copy).expect("a profile of the user's own");
        assert_eq!(removed.id, held.id);
        assert!(!ids(registry.table().profiles()).contains(&held.id.as_str()));

        let at = registry.reinsert(removed, copy);
        assert_eq!(at, copy);
        assert_eq!(registry.table().get(copy), Some(&held));
    }

    /// PIN — **the default and the floor cannot be hidden** (plan §2.4, R5).
    ///
    /// Both are guards rather than politeness: hiding the default leaves no new
    /// tab to open, and hiding the floor puts a hole in the bottom of every
    /// degradation chain in the product — which is exactly why the floor moved
    /// off `pwsh` in the first place.
    #[test]
    fn hiding_the_default_and_hiding_the_floor_are_both_refused() {
        let registry = Registry::shipped();
        let floor = registry
            .table()
            .position_of_id(WINDOWS_POWERSHELL_ID)
            .unwrap();
        assert!(!registry.set_hidden(0, true, 0), "0 is the default here");
        assert!(!registry.set_hidden(floor, true, 0));
        assert!(registry.table().profiles().iter().all(|row| !row.hidden));

        let cmd = registry.table().position_of_id("cmd").unwrap();
        assert!(registry.set_hidden(cmd, true, 0));
        assert!(
            !registry.table().offered().contains(&cmd),
            "hiding is being out of the pickers"
        );
        assert_eq!(
            registry.table().position_of_id("cmd"),
            Some(cmd),
            "and it is still a profile: a seat already on disk restarts through \
             its own id"
        );
        assert!(registry.set_hidden(cmd, false, 0));
    }

    /// PIN — **`Restore all defaults` puts every field back and leaves the two
    /// that are not this profile's**: where it sits in the list, and whether it
    /// is hidden.
    ///
    /// Both are decisions about the *list* rather than about the profile, and one
    /// press undoing two things is what this house keeps taking apart.
    #[test]
    fn restoring_a_builtin_leaves_its_place_and_its_hiding_alone() {
        let registry = Registry::shipped();
        registry.rename(0, "Seven");
        registry.edit(0, |profile| {
            profile.args = vec!["-NoProfile".to_owned()];
            profile.env = vec![("A".to_owned(), "1".to_owned())];
            profile.start_at = StartAt::Home;
            true
        });
        let cmd = registry.table().position_of_id("cmd").unwrap();
        registry.set_hidden(cmd, true, 1);
        registry.rename(cmd, "Console");
        registry.move_profile(0, true);
        let moved = registry.table().position_of_id("pwsh").unwrap();
        assert_eq!(moved, 1);

        assert!(registry.restore_defaults(moved));
        let row = registry.table().get(moved).unwrap().clone();
        assert_eq!(
            row,
            Profile {
                ..shipped().remove(0)
            }
        );
        assert_eq!(
            registry.table().position_of_id("pwsh"),
            Some(1),
            "the row stays where the reader put it"
        );

        assert!(registry.restore_defaults(cmd));
        assert_eq!(
            registry.table().get(cmd).unwrap().display_title,
            "Command Prompt"
        );
        assert!(
            registry.table().get(cmd).unwrap().hidden,
            "a hidden row's editor is reached by opening the row that is dimmed; \
             putting it back in the picker is a question nobody asked"
        );
    }

    /// PIN — **an argument line survives being split and joined**, because the
    /// field is written from the table on every visit and a joiner that quoted
    /// differently from the splitter would rewrite somebody's arguments the
    /// second time they opened the page.
    ///
    /// The rule is the row's own sentence: spaces separate, double quotes group,
    /// `""` inside a group is one literal quote — and a backslash is a
    /// backslash, because a Windows path is full of them.
    #[test]
    fn an_argument_line_survives_being_split_and_joined() {
        for words in [
            vec![],
            vec!["-NoLogo".to_owned()],
            vec!["--cd".to_owned(), r"C:\Program Files\Git".to_owned()],
            vec![r"C:\bin\".to_owned(), "a b".to_owned(), "\"q\"".to_owned()],
            vec![String::new()],
        ] {
            let line = join_arguments(&words);
            assert_eq!(split_arguments(&line), words, "{line:?}");
        }
        assert_eq!(
            split_arguments("  -a   \"b c\"  d  "),
            ["-a", "b c", "d"],
            "runs of whitespace end a word and a quoted group keeps its own"
        );
        assert_eq!(
            split_arguments(r"--path C:\bin\ --x"),
            ["--path", r"C:\bin\", "--x"],
            "a trailing backslash is a backslash and not an escape"
        );
    }

    /// PIN — **`Inherit` is what this window did before the field existed**, and
    /// the other two answers are the two things it could not say.
    #[test]
    fn the_three_starting_answers_are_inherit_home_and_one_fixed_place() {
        let machine = FakeMachine::fully_equipped();
        let inherited = Some(PathBuf::from(r"D:\Developer"));

        let place = place_for(
            &StartAt::Inherit,
            &StartingDir::WindowsHome,
            PathNamespace::Windows,
            inherited.clone(),
            &machine,
        );
        assert_eq!(place.working_directory, inherited);

        let place = place_for(
            &StartAt::Home,
            &StartingDir::WindowsHome,
            PathNamespace::Windows,
            inherited.clone(),
            &machine,
        );
        assert_eq!(
            place.working_directory,
            machine.var_os("USERPROFILE").map(PathBuf::from),
            "Home refuses an inheritance that was there, which is the whole of \
             what it says"
        );

        let place = place_for(
            &StartAt::Fixed(PathBuf::from(r"E:\work")),
            &StartingDir::WindowsHome,
            PathNamespace::Windows,
            inherited.clone(),
            &machine,
        );
        assert_eq!(place.working_directory, Some(PathBuf::from(r"E:\work")));

        // And a fixed folder crosses into the profile's own namespace through
        // the one door every crossing in this module goes through.
        let place = place_for(
            &StartAt::Fixed(PathBuf::from(r"D:\Developer")),
            &StartingDir::LauncherFlag {
                flag: "--cd".to_owned(),
                home: "~".to_owned(),
            },
            PathNamespace::Wsl,
            None,
            &machine,
        );
        assert_eq!(place.arguments, ["--cd", "/mnt/d/Developer"]);

        // A pair that cannot cross falls to the profile's own home rather than
        // to a guess — `cwd_for_spawn`'s rule, not a second one.
        let place = place_for(
            &StartAt::Fixed(PathBuf::from(r"\\server\share")),
            &StartingDir::LauncherFlag {
                flag: "--cd".to_owned(),
                home: "~".to_owned(),
            },
            PathNamespace::Wsl,
            None,
            &machine,
        );
        assert_eq!(place.arguments, ["--cd", "~"]);
    }

    /// PIN — **a new profile wears the chassis and a duplicate wears the brand.**
    ///
    /// The two verbs say different things: `Duplicate` says "another one of
    /// these" and its copy really is a PowerShell, while `New profile` takes the
    /// default only as a template for what to run — and the first thing anybody
    /// does with it is point it somewhere else, at which moment a Microsoft blue
    /// would be a brand on a program that is not theirs.
    #[test]
    fn a_new_profile_wears_the_chassis_and_a_duplicate_wears_the_brand() {
        let registry = Registry::shipped();
        let made = registry.create(0).expect("pwsh is a row");
        assert_eq!(made, registry.table().len() - 1, "it lands at the foot");
        let row = registry.table().get(made).unwrap().clone();
        assert_eq!(row.origin, Origin::User);
        assert_eq!(row.compared_title, None);
        assert_eq!(
            row.mark,
            ChromeMark::ProfileGeneric {
                colour: MarkColour::Blue
            }
        );
        assert_eq!(
            row.program,
            ProgramSource::PowerShellSeven,
            "the template supplies the program; it is the identity that is new"
        );

        let again = registry.create(0).expect("pwsh is a row");
        assert_eq!(
            registry.table().get(again).unwrap().mark,
            ChromeMark::ProfileGeneric {
                colour: MarkColour::Teal
            },
            "two rows in one list must not wear one colour"
        );
    }

    /// PIN — **the environment table round-trips through the file**, which is
    /// the whole of what 5b owes it: the slot is written and read back, and
    /// nothing puts it into a session yet (that is 5c).
    #[test]
    fn the_environment_table_round_trips_through_the_file() {
        let registry = Registry::shipped();
        let copy = registry.duplicate(0).expect("pwsh is a row");
        registry.edit(copy, |profile| {
            profile.env = vec![
                ("FORCE_HYPERLINK".to_owned(), "0".to_owned()),
                ("ANTHROPIC_LOG".to_owned(), "debug".to_owned()),
            ];
            true
        });

        let written = registry.to_file();
        let (read, faults) = merge(shipped(), &written);
        assert!(faults.is_empty(), "{faults:?}");
        assert_eq!(
            read[copy].env,
            [
                ("ANTHROPIC_LOG".to_owned(), "debug".to_owned()),
                ("FORCE_HYPERLINK".to_owned(), "0".to_owned()),
            ],
            "an environment is a mapping, so the file sorts it and the bytes are \
             a function of the content"
        );
    }

    /// PIN — **pointing a row at a program re-derives what it can spell**, so a
    /// profile whose shell became `wsl.exe` stops being told its directories in
    /// Win32.
    #[test]
    fn a_program_a_reader_chose_re_derives_the_namespace_it_speaks() {
        let registry = Registry::shipped();
        let copy = registry.duplicate(2).expect("wsl is a row");
        assert_eq!(
            registry.table().get(copy).unwrap().paths,
            PathNamespace::Wsl
        );
        registry.edit(copy, |profile| {
            profile.program = ProgramSource::Path(PathBuf::from(r"C:\bin\fish.exe"));
            profile.paths = derived_paths(profile);
            true
        });
        assert_eq!(
            registry.table().get(copy).unwrap().paths,
            PathNamespace::Windows,
            "only wsl.exe behind a bash init file crosses the namespace"
        );
    }

    /// PIN — **what the table writes, the table reads back.**
    ///
    /// The round trip is where "only differences are written" is proved to be a
    /// *representation* rather than a lossy summary: reorder, hide, rename and
    /// copy, write, read, and land on exactly the same table.
    #[test]
    fn the_table_survives_the_round_trip_through_its_own_file() {
        let registry = Registry::shipped();
        registry.install(&file(vec![
            named("cmd"),
            ProfileEntryV1 {
                display_title: Some("Ubuntu".to_owned()),
                ..named("wsl")
            },
            ProfileEntryV1 {
                hidden: true,
                ..named("gitbash")
            },
            named("pwsh"),
            named("winps"),
        ]));
        registry.duplicate(0).expect("cmd is a row");
        let before = registry.table().profiles().to_vec();

        let written = registry.to_file();
        let (read, faults) = merge(shipped(), &written);
        assert!(faults.is_empty(), "{faults:?}");
        assert_eq!(read, before);

        let wire = serde_json::to_value(&written).unwrap();
        assert_eq!(
            wire["profiles"][4].as_object().map(serde_json::Map::len),
            Some(1),
            "an untouched built-in is its id and nothing else, so the next build \
             is still free to retune it: {:?}",
            wire["profiles"][4]
        );
    }

    /// PIN — **a press that moves nothing advances nothing.**
    ///
    /// `profile_rev` throws away every measured string in the window, so a
    /// revision that ticked when the first row's `↑` was pressed would be a
    /// window re-measuring itself for a press that did nothing. The dark button
    /// is still drawn, and still a focus stop — it just has no effect to have.
    #[test]
    fn moving_a_row_advances_the_revision_and_moving_the_first_row_up_does_not() {
        let registry = Registry::shipped();
        assert_eq!(registry.revision(), 0);

        assert!(
            !registry.move_profile(0, false),
            "nothing is above the first"
        );
        assert_eq!(registry.revision(), 0);
        let last = registry.table().len() - 1;
        assert!(!registry.move_profile(last, true));
        assert_eq!(registry.revision(), 0);

        assert!(registry.move_profile(1, false));
        assert_eq!(registry.revision(), 1);
        assert_eq!(
            ids(registry.table().profiles()),
            ["winps", "pwsh", "wsl", "gitbash", "cmd"]
        );
        assert!(
            registry.move_profile(0, true),
            "and it goes back the way it came"
        );
        assert_eq!(registry.revision(), 2);
        assert_eq!(ids(registry.table().profiles()), ids(&shipped()));
    }

    /// PIN — **a copy lands under its original, carries its mark, and takes an
    /// id no built-in holds.**
    ///
    /// The five shipped slugs are reserved words: a user profile that took one
    /// would answer to every seed on disk that named the built-in. The copy also
    /// loses its `compared_title`, because no script this build ships will ever
    /// announce a name this build did not choose.
    #[test]
    fn a_duplicate_lands_under_its_original_with_an_id_no_builtin_holds() {
        let registry = Registry::shipped();
        let at = registry.duplicate(0).expect("pwsh is a row");
        assert_eq!(at, 1);
        let table = registry.table();
        let copy = &table.profiles()[1];
        assert_eq!(copy.display_title, "PowerShell 7 copy");
        assert_eq!(
            copy.mark,
            table.profiles()[0].mark,
            "a copy of a PowerShell is a PowerShell"
        );
        assert_eq!(copy.program, table.profiles()[0].program);
        assert_eq!(copy.origin, Origin::User);
        assert_eq!(copy.compared_title, None);
        assert!(
            shipped().iter().all(|seed| seed.id != copy.id),
            "{} took a reserved slug",
            copy.id
        );
        assert!(
            copy.id.starts_with("powershell-7-copy-"),
            "the id is readable, because the whole point of a file of its own is \
             that a person can read it: {}",
            copy.id
        );
        assert_eq!(registry.revision(), 1);
    }

    /// PIN — **a copy of a copy numbers itself from the original's name.**
    ///
    /// The scheme customiser struck this a slice earlier and it is the same
    /// sentence here: `X copy`, `X copy 2`, and never `X copy copy`.
    #[test]
    fn a_copy_of_a_copy_numbers_itself_from_the_original_s_name() {
        let registry = Registry::shipped();
        registry.duplicate(0);
        registry.duplicate(1);
        registry.duplicate(2);
        assert_eq!(
            registry
                .table()
                .profiles()
                .iter()
                .map(|profile| profile.display_title.as_str())
                .take(4)
                .collect::<Vec<_>>(),
            [
                "PowerShell 7",
                "PowerShell 7 copy",
                "PowerShell 7 copy 2",
                "PowerShell 7 copy 3"
            ]
        );
    }

    /// PIN — **a renamed built-in still drops the word its script announces.**
    ///
    /// R7, the thing this slice had to land before any rename was possible.
    /// `folio.ps1` sends `$PSVersionTable.PSEdition`, which is the shipped title
    /// whatever the row has since been called; a pane head compares an
    /// announcement against the set and drops it either way, so neither the
    /// script's word nor the user's own can leak onto a head as a program title.
    #[test]
    fn a_renamed_builtin_still_drops_the_word_its_script_announces() {
        let (built, _) = merge(
            shipped(),
            &file(vec![ProfileEntryV1 {
                display_title: Some("七号".to_owned()),
                ..named("pwsh")
            }]),
        );
        let renamed = &built[0];
        let names = announcement_names(renamed, "七号");
        assert!(
            names.iter().any(|name| name == "PowerShell 7"),
            "the shipped word survives the rename invisibly: {names:?}"
        );
        assert!(names.iter().any(|name| name == "七号"));

        let copy = Profile {
            compared_title: None,
            display_title: "Claude".to_owned(),
            ..renamed.clone()
        };
        assert_eq!(
            announcement_names(&copy, "Claude"),
            ["Claude"],
            "a profile of the user's own has no shipped word, so there is no \
             second string to compare"
        );
    }

    /// PIN — **every capability sentence agrees with
    /// `docs/shell-integration.md`'s own matrix, row by row.**
    ///
    /// J85: this page draws a row of that table rather than building a second
    /// one. The two would otherwise drift the first time a shell gained or lost
    /// a marker, and the page would go on making a promise the terminal had
    /// stopped keeping.
    ///
    /// Red gate: change a `yes` to a `no` in the doc, or reword a sentence to
    /// claim a marker it does not have, and this names the row.
    #[test]
    fn every_capability_sentence_agrees_with_the_shell_integration_matrix() {
        const DOC: &str = include_str!("../../../docs/shell-integration.md");
        let row_for = |label: &str| -> Vec<String> {
            DOC.lines()
                .find(|line| line.starts_with(&format!("| {label} |")))
                .unwrap_or_else(|| panic!("the matrix has a row for {label}"))
                .split('|')
                .map(|cell| cell.trim().to_owned())
                .collect()
        };
        // The columns, as the matrix heads them.
        const PROMPT_MARK: usize = 2; // `133;A`
        const EXIT_CODE: usize = 5; // `133;D` + exit code
        const DIRECTORY: usize = 6; // `OSC 7`
        const HYPERLINK: usize = 8; // `FORCE_HYPERLINK`

        for (id, label) in [
            ("pwsh", "**PowerShell** (7, script installed)"),
            ("winps", "**Windows PowerShell** (5.1, script installed)"),
            ("wsl", "**WSL** (bash login shell)"),
            ("gitbash", "**Git Bash**"),
            ("cmd", "**Command Prompt**"),
        ] {
            let profile = shipped()
                .into_iter()
                .find(|profile| profile.id == id)
                .expect("a shipped id");
            // Lower-cased before the comparison: the sentence opens with a
            // capital because it is a sentence, and the markers it names are
            // the same markers in either case.
            let sentence = capability_text(&profile)
                .in_lang(crate::i18n::Lang::English)
                .to_lowercase();
            let cells = row_for(label);
            let claims = |marker: &str| {
                sentence.contains(marker) && !sentence.contains(&format!("no {marker}"))
            };
            let says_yes = |column: usize| cells[column].trim_matches('*') == "yes";
            assert_eq!(
                claims("prompt marks"),
                says_yes(PROMPT_MARK),
                "{id}: the sentence and the matrix disagree about prompt marks"
            );
            assert_eq!(
                claims("exit codes"),
                says_yes(EXIT_CODE),
                "{id}: the sentence and the matrix disagree about exit codes"
            );
            assert_eq!(
                claims("directory"),
                says_yes(DIRECTORY),
                "{id}: the sentence and the matrix disagree about the directory"
            );
            // The hyperlink column is spelled three ways in the matrix, because
            // three different mechanisms answer it: `yes` where this module
            // declares it, `script` where `folio.ps1` does, and `yes, via
            // WSLENV` where the declaration has a boundary to cross. All three
            // are a session that gets links.
            let declares = cells[HYPERLINK].starts_with("yes") || cells[HYPERLINK] == "script";
            assert_eq!(
                claims("hyperlinks"),
                declares,
                "{id}: the sentence and the matrix disagree about hyperlinks"
            );
        }
        // **And the row this slice added** (§7.1.6c-6c): a profile of the
        // reader's own, served by no door, which is the case `Integration::None`
        // exists for.
        let cells = row_for("**a profile of the reader's own**, no door");
        for column in [PROMPT_MARK, EXIT_CODE, DIRECTORY] {
            assert_eq!(cells[column].trim_matches('*'), "no");
        }
        assert_eq!(cells[HYPERLINK], "yes");
        let theirs = Profile {
            integration: IntegrationChoice::Named(Integration::None),
            ..shipped().swap_remove(0)
        };
        let sentence = capability_text(&theirs)
            .in_lang(crate::i18n::Lang::English)
            .to_lowercase();
        assert!(!sentence.contains("prompt marks"), "{sentence}");
        assert!(
            capability_of_parts(Integration::None, PathNamespace::Windows, true, true)
                .in_lang(crate::i18n::Lang::English)
                .to_lowercase()
                .contains("hyperlinks are declared anyway"),
            "the editor's own length says the one thing that is not lost"
        );
    }

    /// PIN — **a profile that switched its links off stops claiming them**
    /// (§7.1.6c-6c, J85 closed).
    ///
    /// The sentence is derived from three things and not one: the door, the
    /// namespace, and the two environment rows that can silence a link. Red
    /// gate: leave the sentence on the door alone and a profile carrying
    /// `FORCE_HYPERLINK=0` reads `Prompt marks, directory, exit codes and
    /// hyperlinks` while every program in it prints plain text.
    #[test]
    fn a_sentence_never_claims_a_link_the_environment_took_away() {
        let claims_links = |profile: &Profile| {
            let sentence = capability_text(profile)
                .in_lang(crate::i18n::Lang::English)
                .to_lowercase();
            sentence.contains("hyperlinks") && !sentence.contains("no hyperlinks")
        };
        for id in ["pwsh", "winps", "wsl", "gitbash", "cmd"] {
            let shipped_row = shipped()
                .into_iter()
                .find(|profile| profile.id == id)
                .expect("a shipped id");
            assert!(claims_links(&shipped_row), "{id}");
            let off = Profile {
                env: vec![("FORCE_HYPERLINK".to_owned(), "0".to_owned())],
                ..shipped_row.clone()
            };
            assert!(!claims_links(&off), "{id}");
            let sentence = capability_text(&off).in_lang(crate::i18n::Lang::English);
            assert!(
                sentence.to_lowercase().contains("no hyperlinks"),
                "{id}: a row that lost them says so rather than passing over \
                 it: {sentence}"
            );
            // And the markers it still has are the ones it always had, which is
            // what makes this a third dimension and not a fifth sentence.
            let markers = |text: &str| {
                ["prompt marks", "exit codes", "directory"]
                    .into_iter()
                    .filter(|marker| {
                        text.contains(marker) && !text.contains(&format!("no {marker}"))
                    })
                    .count()
            };
            assert_eq!(
                markers(
                    &capability_text(&shipped_row)
                        .in_lang(crate::i18n::Lang::English)
                        .to_lowercase()
                ),
                markers(&sentence.to_lowercase()),
                "{id}"
            );
        }
    }

    /// PIN — **the namespace every shipped profile states is the one it
    /// derives** (§7.1.6c-6c).
    ///
    /// It used to be derived for a profile of the reader's own and stated for
    /// the five, which is two rules for one fact — and left a built-in repointed
    /// at another program still translating directories in the namespace of the
    /// one it no longer runs. Deriving it everywhere is only safe because the
    /// derivation agrees with all five, which is what this proves.
    #[test]
    fn the_namespace_every_shipped_profile_states_is_the_one_it_derives() {
        for profile in shipped() {
            assert_eq!(
                derived_paths(&profile),
                profile.paths,
                "{}: the stated namespace and the derived one disagree",
                profile.id
            );
        }
    }

    /// PIN — **the door survives the round trip, rule and answer alike.**
    ///
    /// `auto` is a fifth word on the wire and not an absent key, because absence
    /// already means two other things here: on a built-in it is "whatever this
    /// build ships", and on a row of the reader's own it is their own defaults.
    /// Red gate: leave `Auto` unwritten and a row switched back to it reads as
    /// the door it happened to be on when it was written.
    #[test]
    fn the_door_a_reader_chose_survives_the_file_and_so_does_auto() {
        let registry = Registry::shipped();
        let cmd = registry
            .table()
            .profiles()
            .iter()
            .position(|profile| profile.id == "cmd")
            .expect("cmd is a row");
        registry.edit(cmd, |profile| {
            profile.integration = IntegrationChoice::Named(Integration::None);
            profile.paths = derived_paths(profile);
            true
        });
        let there_and_back = |registry: &Registry| {
            let (read, faults) = merge(shipped(), &registry.to_file());
            assert!(faults.is_empty(), "{faults:?}");
            read
        };
        assert_eq!(
            there_and_back(&registry)[cmd].integration,
            IntegrationChoice::Named(Integration::None)
        );
        registry.edit(cmd, |profile| {
            profile.integration = IntegrationChoice::Auto;
            profile.paths = derived_paths(profile);
            true
        });
        assert_eq!(
            there_and_back(&registry)[cmd].integration,
            IntegrationChoice::Auto,
            "a row switched back to the rule must not read as the answer it left"
        );
        assert_eq!(
            served_by(&there_and_back(&registry)[cmd]),
            Integration::CmdPrompt
        );
    }

    /// PIN — **a shell this machine does not have says why, and says nothing
    /// else.**
    ///
    /// `.row.unavailable`'s existing rule met by a row that happened to have two
    /// sentences: a shell that is not here has no capabilities to report, and the
    /// one line the row has room for is the reason it cannot start.
    #[test]
    fn an_unavailable_row_gives_its_reason_and_drops_its_capability_line() {
        let lines = page_lines(&bare(), fallback_profile());
        assert_eq!(lines.len(), count());
        let git = lines
            .iter()
            .find(|line| line.index == index_of_id("gitbash"))
            .expect("Git Bash is a row even where Git is not installed");
        assert!(!git.available);
        assert_eq!(git.capability, None);
        assert!(
            git.command.contains("Git Bash"),
            "the sentence became the reason: {}",
            git.command
        );

        let floor = lines
            .iter()
            .find(|line| line.index == fallback_profile())
            .expect("the floor is always a row");
        assert!(floor.available && floor.is_default);
        assert_eq!(
            floor.capability,
            Some(crate::i18n::Text::CapPowerShell.text())
        );
        assert_eq!(
            floor.command, "powershell.exe -NoLogo",
            "the row says what it starts, in the executable's own name"
        );
    }

    /// PIN — the line under a name is the program's own leaf and the words
    /// handed to it, including the launcher's place flag.
    ///
    /// The full path is sixty characters and the row has about fifty-eight; the
    /// executable's name is the half that identifies it. `wsl.exe` alone would
    /// say nothing about where the tab opens, which is what `--cd ~` is.
    #[test]
    fn the_line_under_a_name_is_the_executable_and_its_words() {
        let lines = page_lines(&equipped(), fallback_profile());
        let of = |id: &str| {
            lines
                .iter()
                .find(|line| line.index == index_of_id(id))
                .map(|line| line.command.clone())
                .expect("a shipped id")
        };
        assert_eq!(of("pwsh"), "pwsh.exe -NoLogo");
        assert_eq!(of("wsl"), "wsl.exe --cd ~");
        assert_eq!(of("gitbash"), "bash.exe --login -i");
        assert_eq!(of("cmd"), "cmd.exe");
    }

    /// **A page's Recent row is a Recent row** (W2 slice ③; `docs/DESIGN.md`
    /// §7.7 ⑥ — 「预览戴它那块 pane 自己的记号」).
    ///
    /// Same list, same three questions, two different answers where the two
    /// genuinely differ: the mark, which is the web class's globe through the one
    /// door every preview row asks, and the caption, which is the page's *site*
    /// because this vault stores a place and never a title. The tip is the whole
    /// address either way, which is where this list has always put what a caption
    /// crops.
    ///
    /// Red gate: answer `ChromeMark::File` for both and the first assertion
    /// fails; caption a page with `cwd_leaf` and the second answers the whole URL,
    /// because a URL has no backslash for that rule to split on.
    #[test]
    fn a_recent_row_for_a_page_wears_the_globe_and_is_captioned_by_its_site() {
        const URL: &str = "http://localhost:5173/app?tab=logs#top";
        let page = Seed::Preview {
            path: URL.to_owned(),
            source: bt_persist::PreviewSourceV1::Url,
        };
        let file = Seed::Preview {
            path: r"D:\work\folio\README.md".to_owned(),
            source: bt_persist::PreviewSourceV1::File,
        };
        let nothing = crate::favicon::Favicons::default();
        assert_eq!(
            recent_mark(&page, &nothing),
            ChromeMark::Globe { favicon: None }
        );
        assert_eq!(
            recent_mark(&file, &nothing),
            ChromeMark::File,
            "and the file beside it is unchanged"
        );
        assert_eq!(recent_label(&page), "localhost:5173");
        assert_eq!(recent_label(&file), "README.md");
        assert_eq!(recent_tip(&page), URL, "the tip is the whole address");
        assert_eq!(recent_tip(&file), r"D:\work\folio\README.md");
    }

    /// **Red gate (the favicon slice, `docs/DESIGN.md` §7.13): a Recent row for a site this session has
    /// seen wears that site's icon, and the file beside it cannot be given
    /// one.**
    ///
    /// The surface the slice was called for, in its hardest form: a Recent row
    /// has no pane and never did, so it can only be answered by a store keyed by
    /// site. The second half is the guard — `preview_row_mark` refuses an icon
    /// handed to something that is not a page, so a caller's mistake cannot put
    /// a server's drawing on `README.md`.
    ///
    /// MUTATION: pass `None` from `recent_mark`'s page arm and the row is a
    /// globe again, which is the state of the tree before this slice. MUTATION:
    /// drop `preview_row_mark`'s `is_page` branch and the file wears the icon.
    #[test]
    fn a_recent_row_wears_its_sites_icon_where_the_session_learned_one() {
        let mut store = crate::favicon::Favicons::default();
        let mut buffer = image::RgbaImage::new(16, 16);
        for pixel in buffer.pixels_mut() {
            *pixel = image::Rgba([3, 4, 5, 255]);
        }
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(buffer)
            .write_to(&mut png, image::ImageFormat::Png)
            .expect("an in-memory PNG encodes");
        assert!(store.learn("http://localhost:5173", png.get_ref()));
        let learned = store
            .of_url("http://localhost:5173/app")
            .expect("the store holds it");

        let page = Seed::Preview {
            path: "http://localhost:5173/app?tab=logs#top".to_owned(),
            source: bt_persist::PreviewSourceV1::Url,
        };
        assert_eq!(
            recent_mark(&page, &store),
            ChromeMark::Globe {
                favicon: Some(learned)
            },
            "a row with a URL is all a site-keyed store needs to be asked"
        );

        // A file whose *path* would parse as nothing at all, beside a page on a
        // site the store knows: the row that must stay `#i-file`.
        let file = Seed::Preview {
            path: r"D:\work\folio\README.md".to_owned(),
            source: bt_persist::PreviewSourceV1::File,
        };
        assert_eq!(recent_mark(&file, &store), ChromeMark::File);

        // And a page on a server nobody has been to is the globe — the other
        // half of §7.7 ②, with nothing to report about it.
        let elsewhere = Seed::Preview {
            path: "https://unvisited.test/".to_owned(),
            source: bt_persist::PreviewSourceV1::Url,
        };
        assert_eq!(
            recent_mark(&elsewhere, &store),
            ChromeMark::Globe { favicon: None }
        );
    }

    /// **Red gate (the favicon slice, `docs/DESIGN.md` §7.13): a switcher row asks with its own
    /// address.**
    ///
    /// The 2026-08-23 ruling's own example — 「切换器/Recent 里网页行全是地球标、
    /// 只能靠读文字区分」. What makes it answerable is [`PreviewMenuItem::page_url`]:
    /// the row already carries the pin target it would keep, and that target is
    /// the address.
    ///
    /// MUTATION: have `page_url` answer `Some` for a file row and a document
    /// named after a URL would be handed that site's icon.
    #[test]
    fn a_switcher_row_that_is_a_page_can_name_the_site_it_asks_about() {
        let page = PreviewMenuItem {
            name: "Logs".to_owned(),
            dirty: false,
            current: true,
            keep: Some(PreviewMenuTarget {
                kind: bt_persist::PinKind::Url,
                target: "http://localhost:5173/app".to_owned(),
            }),
            pinned: false,
            pool: Some(0),
        };
        let file = PreviewMenuItem {
            keep: Some(PreviewMenuTarget {
                kind: bt_persist::PinKind::File,
                target: r"D:\work\folio\README.md".to_owned(),
            }),
            ..page.clone()
        };
        let unkeepable = PreviewMenuItem {
            keep: None,
            ..page.clone()
        };
        assert_eq!(page.page_url(), Some("http://localhost:5173/app"));
        assert_eq!(file.page_url(), None);
        assert_eq!(unkeepable.page_url(), None);
        assert!(page.is_page());
        assert!(!file.is_page());
    }
}
