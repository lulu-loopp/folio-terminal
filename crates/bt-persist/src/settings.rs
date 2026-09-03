//! `settings.json` v1 — docs/M2-persistence-schema-v1.md §2.
//!
//! Deliberately one field. §2's own words: "只收录已经在 DESIGN/M2 文档里落定的
//! 用户可见项,不为『将来大概率要做』的功能预留占位字段" — `BT_BG`, zoom, wheel
//! overrides, `detect_image_paths`, and `FORCE_HYPERLINK` were each considered
//! and explicitly rejected for v1 (§2's table, ratified in §7). Adding them
//! back here would be exactly the "只写字段 = 死规格" mistake that ruling
//! guards against.

use serde::{Deserialize, Serialize};

/// Current `schema_version` for `settings.json`.
///
/// v2 adds `display_formulas`, v3 adds `inline_formulas`, v4 adds
/// `default_profile`, v5 adds `git_panel`, v6 adds `split_direction`, v7 adds
/// `language`, v8 adds `terminal_font_family`, `terminal_font_size` and
/// `psreadline_invite`, v9 adds `light_scheme` and `dark_scheme`, v10 adds
/// `background_image`, `background_fit`, `background_image_opacity`,
/// `background_opacity`, `acrylic` and `always_on_top`. §2's
/// "只收录已经在 DESIGN/M2 文档里落定的用户可见项" is satisfied the way §1.3
/// intends it to be: each field arrives in the same change that gives it a
/// reader, not ahead of one.
///
/// **v8 carries three fields and it is one bump**, because all three arrive with
/// their readers in the same change: the Appearance block's two font rows and
/// the Terminal page's PSReadLine row. What v8 deliberately does *not* carry is
/// `interface_font_family` — the chrome's own face stays fixed in this version,
/// so a field for it would be §2's "只写字段 = 死规格" exactly.
///
/// **v9 carries two fields and it is also one bump**, for v8's reason and one of
/// its own. The reason of its own is that the two are not two decisions: a
/// person choosing colour schemes chooses the pair, because `theme_mode` still
/// decides which of the two is in force and neither half answers the question
/// alone. A file naming only a dark scheme would leave the Light side to be
/// guessed at, and a build that guessed would be picking on the user's behalf on
/// every machine whose Windows is set to light. Both arrive with their reader in
/// the same change — the Appearance block's scheme row, which offers the pair
/// and writes the pair.
///
/// **v10 carries six fields and it is one bump**, v8's reason again and at its
/// widest: the six are the whole of one page's new block and they all arrive
/// with their readers in the same change. Four of them are one subject read
/// downwards — whether there is a picture, how it meets the window, how much of
/// it comes through, and how much of the desktop comes through behind it — and
/// a file that recorded three of the four would leave the fourth to be guessed
/// at on the first launch that read it. The other two (`acrylic`,
/// `always_on_top`) are window postures rather than ground, and they are here
/// rather than in a v11 because a schema version is a **file format** and not a
/// changelog: bumping twice for one afternoon's rows would cost every reader a
/// second migration step to express the same one-day difference.
///
/// **v11 carries one field**, `advanced_open`, and it is a bump of its own for
/// the reason v10 gives for *not* being two: it arrived on a different day with
/// a different reader. It is also the first list-valued field in this document,
/// which is a shape change and not only a new key — see [`SettingsV1::advanced_open`].
///
/// **v12 carries one field**, `tables`, and it is v11's reason again: a different day, a
/// different reader. It carries a behaviour forward rather than choosing a new default — see
/// `migrate_settings_v11_to_v12`.
///
/// **v13 carries `block_max_height`**, the Rendered blocks page's own `Maximum height` row, and
/// it is the same shape once more: one key, its own day, and a migration that writes the answer
/// every build before it gave — no cap at all.
///
/// **v14 carries `scrollback_lines`**, the Terminal page's own `Scrollback` row, and it is
/// the same shape a fourth time: one key, its own day, and a migration that writes the
/// answer every build before it gave. The difference from v13 is only that the answer is a
/// number somebody once wrote in Rust rather than an absence — `M0_FROZEN_LINE_QUOTA`, one
/// hundred thousand lines a pane, in force since M0-alpha — and this step moves it into the
/// file without moving it.
///
/// **v15 carries `focus_mode`**, the Appearance page's own `Focus mode` row (DESIGN §7.1.6b′),
/// and it is the same shape a fifth time: one key, its own day, and a migration that writes the
/// answer every build before it gave. Here that answer is genuinely an absence — no build before
/// this one had a focus mode to be in — so the step writes `false`, and a reader who has never
/// met the row opens exactly the window they opened yesterday. The key is what makes the mode a
/// place somebody can *live* rather than a verb they re-press every morning: a window closed
/// with the card column up reopens with it up.
///
/// **v16 carries `minimum_contrast`**, the Appearance page's own `Minimum contrast` row
/// (DESIGN §2.6), and it is the same shape a sixth time: one key, its own day, and a migration
/// that writes the answer every build before it gave. That answer is `Off`, and here the word
/// is load-bearing rather than merely conservative: every value above it *changes colours a
/// program asked for*, so a file that had never been asked this question must not come back
/// from a migration having been answered `4.5:1` on the reader's behalf. A terminal that
/// silently re-inked yesterday's output on the day it learned how would be the one thing this
/// feature exists to prevent, pointed the other way.
///
/// **v17 carries `terminal_notifications`**, the Terminal page's own `Notifications` row
/// (DESIGN §7.6), and it is the same shape a seventh time: one key, its own day. It differs from
/// the six before it in what the migration writes — `true`, the product's default for a feature
/// shipping new, rather than the answer earlier builds gave. Those builds gave no answer at all:
/// no build before this one could raise a desktop notification, so there is no behaviour to carry
/// forward and `false` would freeze an absence rather than preserve a status quo. That is
/// `migrate_settings_v2_to_v3`'s distinction, drawn a second time.
///
/// **v18 carries `powershell_integration_offer`**, the Terminal page's `Offer PowerShell
/// integration` row and the switch the notice strip's `Don't show again` writes (DESIGN
/// §7.1.6j). One key, its own day, an eighth time. The migration writes `true` for
/// `migrate_settings_v16_to_v17`'s reason and not `migrate_settings_v14_to_v15`'s: no build that
/// could write a v17 file ever offered anything, so there is no answer to carry forward and
/// `false` would freeze an absence. What is being defaulted on here is an *offer* and not a
/// change — a pane that already loads the script is never asked, and a reader who is asked has
/// one press that ends the asking for good.
///
/// **v19 carries `focus_card_height`**, the Appearance page's own `Focus card height` row
/// (`docs/DESIGN.md` §7.1.6b′), and it is the shape v13–v16 share: one key, its own day, and a
/// migration that writes the answer every build before it gave — 160, the body a focus card has
/// stood on since F2. The row exists because 2026-08-20 turned that number once for everybody and
/// could not have known whose pane spends its bottom thirteen rows on an agent's status bar.
///
/// **The number it stamps is the one it was handed second.** A line adding
/// `powershell_integration_offer` reached `main` first and took v18, and this table is
/// walked one step at a time (`crate::migrate::migrate_value`), so a branch cannot reserve
/// a number whose step it does not also build. The standing rule
/// (`docs/HANDOFF-2026-08-21.md` §2) settles it the way it was written for: whichever
/// merges second yields, and yielding is renaming the step rather than renumbering a file
/// anybody already has.
///
/// **v21 carries `line_wrapping`**, whether a logical line too long for the pane wraps onto the
/// next row or is flattened onto one row and read through a horizontal window
/// (`docs/plans/horizontal-scroll/plan.md` §5.7, ladder one level two). It lands the way v13–v16
/// and v19 did rather than the way v17–v20 did: this is not a feature arriving with its row, it is
/// the only thing every terminal ever written here has done, so `true` is a behaviour being
/// carried forward and not a product default being chosen.
/// **v22 carries `key_hints`**, whether holding a modifier still raises the card that lists the
/// shortcuts it starts (`docs/DESIGN.md` §7.1.5e′). It lands the way v17–v20 did rather than the
/// way v13–v16, v19 and v21 did, and the distinction is the one those steps are written under:
/// there is no habit here to carry forward, because no build before this one drew the card at all.
/// What is being defaulted on is an **offer** — a surface that appears only when a hand has
/// deliberately stopped, and that goes away the instant it is touched — and the row that ends the
/// offering is one press away on the General page.
///
/// **v23 carries `turn_end_notification`**, whether the end of an agent's turn is allowed to reach
/// the desktop at all — a taskbar flash on a window that has not got the keyboard, a toast on one
/// that is minimised (`docs/plans/attention/plan.md` §11.7, user ruling 2026-08-25). It lands the
/// way v17–v20 and v22 did rather than the way v13–v16, v19 and v21 did: no build before this one
/// had a turn-end lane at all, so `true` is the product's default for a feature shipping new and
/// not a habit being carried forward.
/// **v24 carries `cards_gesture_hint_offer`**, whether entering Cards still owes the reader the
/// one-time bubble that says `Alt`+wheel scrolls a card (`docs/DESIGN.md` §7.21, user ruling
/// 2026-08-27). It lands the way v22 did, and for v22's own reason read across two surfaces:
/// what is being defaulted on is an **offer**, and no build before this one made it.
///
/// It is the second bit in this file of `powershell_integration_offer`'s kind and it is written
/// deliberately in that bit's shape — `true` means *still owed*, `false` means *spent* — because
/// the two are one sentence about one file: a thing this window has to say once, and a record of
/// whether it has said it. The difference between them is only how the debt is discharged: the
/// PowerShell strip is dismissed by a reader who says "stop asking", and this one is spent by
/// being read. It is cleared back to `true` by `Reset to defaults` on the Appearance page, which
/// is the page Cards lives on — the one verb in this product that says "put this page back the
/// way it shipped", and the reason this bit is in this file rather than in a record of its own.
///
/// **v25 carries `copy_on_select`**, whether dragging a selection in the terminal writes it to the
/// clipboard the moment the drag lets go. The 2026-08-26 gesture audit
/// (`docs/plans/ui-style/invisible-gestures-2026-08-26.md`, 丙4) judged the write invisible: nothing
/// on screen says the clipboard was just overwritten, and Windows Terminal's own `copyOnSelect`
/// ships `false`, so a reader arriving from that product carries no muscle memory for it either.
/// The row exists so the behaviour has a name and a door to be shown out of — it does not exist to
/// choose the behaviour, which was chosen long before this row was. It lands the way v13–v16, v19
/// and v21 did rather than the way v17–v20 and v22–v24 did: every build before this one already
/// copied a drag's selection the instant it was let go, so `true` is a habit being carried forward,
/// unlike the two notes above it, which each chose a default for something that had never existed.
///
/// **It was written as v24 and became v25 on 2026-08-27**, when the Cards bubble's key reached
/// `main` first. Two keys cannot share a rung — `SETTINGS_MIGRATIONS` is the only map of the road
/// from an old file to this one — so the later slice takes the later number, which is a rule and
/// not a judgement about which key mattered more.
///
/// **v26 carries `update_check`**, whether this build asks the releases page once a day whether a
/// newer one exists. It is the first key in this file that governs a network request, and it is
/// the only one, which is why it defaults **on** rather than off: a reader who has installed a
/// preview has no other way to learn that the bug they hit was fixed a month ago, and the whole
/// of what the switch permits is one `GET` of one address that carries nothing about them
/// (`docs/PRIVACY.md`). It lands the way v13-v16, v19 and v21 did — a default chosen for a thing
/// that did not exist before, stated here so that the choice is a decision somebody reads rather
/// than `bool::default()` answering by accident.
///
/// **v27 carries the two keys the quake terminal has** (0.2 shortcut terminal, `docs/DESIGN.md`
/// §7.54): `quake_height`, how much of the monitor's work area the summoned window covers, and
/// `quake_dismiss_on_blur`, whether it goes away when the keyboard leaves it. One bump for two
/// keys, because they arrive together and a rung is a rung — `SETTINGS_MIGRATIONS` is the only map
/// of the road from an old file to this one, and two steps for one release would put a version
/// number on a document nobody ever wrote.
///
/// Both land the v13-v16 way: a default chosen for a thing that did not exist before. `quake_height`
/// is 40 percent, which is what the window opens as until somebody moves it; `quake_dismiss_on_blur`
/// is **on**, because a terminal that hangs across the top of the screen after you have clicked back
/// into your editor is a terminal covering the thing you clicked into.
///
/// **v28 carries `quake_width`**, the summoned window's third key, added when the shape v27
/// shipped met a 4K ultrawide (user ruling, 2026-09-02, `docs/DESIGN.md` §7.54). It lands the v25
/// way and not the v13–v16 way, and it is the only step on this ladder that does: the key does not
/// name a default for something that never existed, it **replaces a fact that used to be wired
/// shut**. A migrated file could honestly have been given `100`, which is what every build before
/// this one did; it is given `60`, because the old width was not a preference anybody expressed —
/// there was no row to express it on — so carrying it forward would be preserving an accident
/// rather than a choice.
pub const SETTINGS_SCHEMA_VERSION: u32 = 28;

/// The profile id a `settings.json` that has never named one is read as.
///
/// The empty string rather than `"pwsh"`, and the difference is the whole point:
/// this crate does not know what profiles exist. `"pwsh"` written here would be
/// this file asserting a fact about `bt-app`'s table — a spelling that would go
/// on being written into every settings file long after the table had been
/// renamed around it. An empty id names no profile, which every reader already
/// has to handle (a file written by a *newer* build can name a profile this one
/// has never heard of), so "not chosen" arrives through the path "chosen, but
/// gone" already goes down instead of through a second one.
pub const DEFAULT_PROFILE_UNSET: &str = "";

/// The terminal font family a `settings.json` that has never named one is read as.
///
/// The empty string rather than `"Consolas"`, and it is [`DEFAULT_PROFILE_UNSET`]'s
/// reasoning applied to a face instead of a shell: this crate does not know which
/// families the machine has. `"Consolas"` written here would be a settings file
/// asserting that a particular face exists — a spelling that goes on being written
/// into every file long after the reader's default has moved, and one that cannot
/// be told apart from a user who deliberately picked Consolas out of the list. An
/// unnamed family means "whatever this build's default face is", which every
/// reader already has to handle, because a family the file names may equally have
/// been uninstalled since.
pub const DEFAULT_TERMINAL_FONT_FAMILY: &str = "";

/// The terminal font size, in logical pixels, of a file that has never named one.
///
/// 16 because that is the number `bt_render`'s `BASE_FONT_SIZE_LOGICAL_PX` has
/// been since the first frame this product drew; the row writes the answer down
/// rather than changing it.
pub const DEFAULT_TERMINAL_FONT_SIZE: u8 = 16;

/// The scheme in force under a Light theme when the file has never named one.
///
/// The empty string, and it is [`DEFAULT_PROFILE_UNSET`]'s ruling reaching a
/// third table — first a shell, then a face, now a palette. This crate does not
/// know which schemes exist: the list is a product table plus whatever the user
/// has dropped into the schemes folder, and it is assembled a whole crate away.
/// `"Folio Light"` written here would be a settings file asserting a fact about
/// that table, would go on being written into every file long after the built-in
/// default had been renamed or retired around it, and — the half that actually
/// costs something — could not be told apart from a user who opened the list and
/// picked Folio Light deliberately. An unnamed scheme means "whatever this
/// build's default palette is", which every reader already has to handle,
/// because a scheme the file names may equally have been deleted since.
pub const DEFAULT_LIGHT_SCHEME: &str = "";

/// The scheme in force under a Dark theme when the file has never named one.
///
/// [`DEFAULT_LIGHT_SCHEME`]'s reasoning, unchanged, on the other side of the
/// theme. The two are a pair rather than one field with a mode attached because
/// `theme_mode` already owns the mode: it decides which of the two is read, and
/// these two decide what is read *as*. A single `scheme` field would force a
/// user who follows the system to accept one palette in both, which is the one
/// thing a light-and-dark product must not make them do.
pub const DEFAULT_DARK_SCHEME: &str = "";

/// The picture drawn behind the window when the file has never named one.
///
/// The empty string, and here it is not [`DEFAULT_PROFILE_UNSET`]'s deferral but
/// a plain absence: there is no picture this build could have meant, so "" is
/// the value itself and not a stand-in for one. A **path** and not a copied
/// file, because a wallpaper is a file the user already owns and already
/// organises; copying it into the settings folder would leave two of it, and the
/// one they later edit would be the wrong one. The path is not validated here —
/// a file that has since been moved, renamed or unplugged is the ordinary case,
/// and the reader's answer is §5.4 逐叶降级: draw no picture and keep the name,
/// so that plugging the drive back in restores it without a second trip through
/// the chooser.
pub const DEFAULT_BACKGROUND_IMAGE: &str = "";

/// How much of the picture reaches the window, as a whole percentage.
///
/// 100 because a person who has just chosen a picture wants to see whether they
/// chose the right one; fading it is the second decision, and a row that
/// arrived pre-faded would make the first one impossible to judge.
pub const DEFAULT_BACKGROUND_IMAGE_OPACITY: u8 = 100;

/// How much of the window's ground is there at all, as a whole percentage.
///
/// 100 — an opaque window, which is what every build before this one drew. The
/// migration writes this same number for the same reason `v5_to_v6` wrote
/// `Auto`: it records the behaviour that was already in force rather than
/// choosing a new one.
pub const DEFAULT_BACKGROUND_OPACITY: u8 = 100;

/// The floor under [`DEFAULT_BACKGROUND_OPACITY`] (user ruling 2026-08-17).
///
/// Thirty percent and not zero. Below roughly a third the ground stops being a
/// surface and becomes a hole: the desktop behind it competes with the grid for
/// every pixel that is not a glyph, panes stop reading as panes because their
/// own fills vanish with the clear, and the window's edges are the only thing
/// left saying where it is. A floor is also what makes "text stays opaque" a
/// promise worth making — there is no setting from which this window can be
/// made unreadable.
pub const MINIMUM_BACKGROUND_OPACITY: u8 = 30;

/// **How much of the monitor's work area a summoned terminal covers**, as a whole
/// percentage, when nobody has said otherwise (v27, `docs/DESIGN.md` §7.54).
///
/// Forty, which is the proportion the window has to be to be worth summoning:
/// tall enough that a command's output does not scroll away as it arrives, and
/// short enough that the window it came down over is still readable underneath —
/// which is the whole reason a person reaches for this shape rather than opening
/// an ordinary window.
pub const DEFAULT_QUAKE_HEIGHT: u8 = 40;

/// The floor under [`DEFAULT_QUAKE_HEIGHT`].
///
/// Twenty percent, and a floor rather than zero for [`MINIMUM_BACKGROUND_OPACITY`]'s
/// reason turned on a different axis: below about a fifth of a screen the window
/// is a strip with a tab bar and no room for a shell under it, and a summon that
/// produced one would read as a window that failed to open. There is no setting
/// from which this window can be made useless.
pub const MINIMUM_QUAKE_HEIGHT: u8 = 20;

/// **How much of the monitor's work area a summoned terminal covers**, as a whole percentage of
/// its *width*, when nobody has said otherwise (v28, `docs/DESIGN.md` §7.54).
///
/// Sixty, and it is a number a real machine handed back rather than one this file reasoned to.
/// The window shipped spanning the whole work area, because spanning it was thought to be the
/// shape; on a 4K ultrawide the reader who has to work in it reported it as simply too wide — a
/// line of output starts at one edge of the desk and ends at the other, and the eye crosses the
/// whole of it to read a prompt. Sixty leaves the window wider than any ordinary terminal a person
/// opens by hand while keeping the whole of it inside one turn of the head.
pub const DEFAULT_QUAKE_WIDTH: u8 = 60;

/// The floor under [`DEFAULT_QUAKE_WIDTH`].
///
/// Thirty percent, [`MINIMUM_QUAKE_HEIGHT`]'s argument turned onto the other axis: below about a
/// third of a screen the window is narrower than the lines the shell in it will draw, and a summon
/// whose every line wrapped would read as a window that opened wrong. There is no setting from
/// which this window can be made useless.
pub const MINIMUM_QUAKE_WIDTH: u8 = 30;

/// The height cap a `settings.json` that has never named one is read as: **none**.
///
/// Zero is the value and "no limit" is the sentence, and it is the default because it is what
/// every build before the row existed did. A product that started capping blocks the day it grew
/// a control for capping them would be answering a question on the reader's behalf with the one
/// answer they cannot have asked for.
pub const DEFAULT_BLOCK_MAX_HEIGHT: u32 = 0;

/// How many lines of past output a pane keeps when the file has never named a number:
/// **100,000**, per pane.
///
/// Not a new answer. It is `bt_app`'s `M0_FROZEN_LINE_QUOTA`, which has been the capacity of
/// every pane this product has ever drawn, moved to the place a user can now reach it from —
/// so the row ships without changing what anybody's terminal does. A product that started
/// keeping less history the day it grew a control for history would be answering a question
/// on the reader's behalf with the one answer they cannot have asked for, which is the same
/// sentence [`DEFAULT_BLOCK_MAX_HEIGHT`] is written under.
pub const DEFAULT_SCROLLBACK_LINES: u32 = 100_000;

/// How tall a focus card's body stands when the file has never named a number: **160** logical
/// pixels.
///
/// Not a new answer. It is `bt_render`'s `DEFAULT_FOCUS_MINI_HEIGHT_LOGICAL_PX`, the height every
/// card has had since F2 (2026-08-20), moved to the place a reader can now reach it from — so the
/// row ships without changing what anybody's column looks like. The number is spelled here rather
/// than imported for [`DEFAULT_SCROLLBACK_LINES`]'s reason: this crate is the file format and does
/// not depend on the renderer, and the two are held together by the test that reads them both.
pub const DEFAULT_FOCUS_CARD_HEIGHT: u32 = 160;

/// `settings.json` v25 — docs/M2-persistence-schema-v1.md §2:
/// ```json
/// {
///   "schema_version": 25,
///   "theme_mode": "System" | "Light" | "Dark",
///   "display_formulas": true | false,
///   "inline_formulas": true | false,
///   "tables": true | false,
///   "block_max_height": 0 | 120 | 240 | 480,
///   "default_profile": "pwsh" | "wsl" | "gitbash" | "cmd" | "",
///   "git_panel": true | false,
///   "split_direction": "Auto" | "Right" | "Down",
///   "language": "System" | "English" | "Chinese",
///   "terminal_font_family": "Consolas" | "Cascadia Mono" | … | "",
///   "terminal_font_size": 10..=24,
///   "psreadline_invite": "NotAsked" | "Declined" | "Installed" | "Dismissed",
///   "light_scheme": "Solarized Light" | … | "",
///   "dark_scheme": "Nord" | … | "",
///   "background_image": "C:\\Users\\me\\Pictures\\ridge.jpg" | "",
///   "background_fit": "Stretch" | "Fill" | "Tile",
///   "background_image_opacity": 0..=100,
///   "background_opacity": 30..=100,
///   "acrylic": true | false,
///   "always_on_top": true | false,
///   "advanced_open": ["appearance", …],
///   "scrollback_lines": 25000 | 50000 | 100000 | 200000,
///   "focus_mode": true | false,
///   "minimum_contrast": "Off" | "Ratio2" | "Ratio3" | "Ratio45",
///   "terminal_notifications": true | false,
///   "powershell_integration_offer": true | false
///   "focus_card_height": 160 | 240 | 320,
///   "line_wrapping": true | false,
///   "key_hints": true | false,
///   "turn_end_notification": true | false,
///   "cards_gesture_hint_offer": true | false,
///   "copy_on_select": true | false,
///   "update_check": true | false,
///   "quake_height": 20..=100,
///   "quake_width": 30..=100,
///   "quake_dismiss_on_blur": true | false
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsV1 {
    pub schema_version: u32,
    pub theme_mode: ThemeModeV1,
    /// Whether detected display math (`$$…$$`, LaTeX environments) is *drawn*
    /// as a typeset band. Off leaves detection entirely alone — the scanner,
    /// the ownership ledger and every guard keep running, and the source text
    /// simply stays on screen instead of being covered. This is a presentation
    /// policy, not a detection one; see `MathLayoutOptions` in bt-term for the
    /// detection-side bits, which this deliberately does not touch.
    pub display_formulas: bool,
    /// Whether a `$…$` run *inside a command's output* is drawn as a typeset
    /// inline formula. The sibling of `display_formulas`, and presentation policy
    /// in exactly the same sense: off leaves the scanner and every guard running
    /// and simply keeps the source text on screen.
    ///
    /// It is a separate switch rather than a second meaning for `display_formulas`
    /// because the two carry different risk. A `$$…$$` pair is a whole-line
    /// delimiter that ordinary terminal text effectively never produces by
    /// accident; a lone `$` is the most overloaded byte a shell prints. Someone
    /// who wants typeset blocks but wants every `$` in a log left alone must be
    /// able to say so, and that is one switch, not a preference we guess.
    pub inline_formulas: bool,
    /// Whether a detected GFM pipe table in command output is *drawn* as a
    /// rendered block. The third switch on the Rendered blocks page, and
    /// presentation policy in exactly the sense the two above it are: off leaves
    /// the scanner running and simply keeps the pipe text on screen, so turning
    /// it back on costs one frame and re-arms the same proven tables.
    ///
    /// A switch of its own rather than a third meaning for `display_formulas`,
    /// for the reason `inline_formulas` gives: the two features fail
    /// differently. A `$$` pair is a delimiter a program writes on purpose; a
    /// pipe is ordinary punctuation, and someone who wants typeset formulas with
    /// every `|` in their logs left alone has to be able to say exactly that.
    pub tables: bool,
    /// **How tall a rendered block may stand before it scrolls inside itself**, in logical
    /// pixels, and `0` for no limit at all (the Rendered blocks page's `Maximum height` row,
    /// mock-up 4370).
    ///
    /// `0` and not `Option<u32>`, because the file is meant to be read and edited by hand and
    /// `"block_max_height": 0` says "no limit" in the same shape every other number in this
    /// document says its own value; a key that is sometimes `null` and sometimes a number asks a
    /// person editing it to know which of two grammars this line is in. The type that *does*
    /// carry the distinction is `bt_term::MathLayoutOptions::block_max_height_px`, which is a
    /// `NonZeroU32` — and the conversion happens once, at the door, where zero stops being a
    /// number and becomes an absence.
    ///
    /// **Logical pixels, not rows.** The cap is applied to a *picture*: a formula raster and a
    /// table's laid-out box are both scaled by the window's DPI and neither is a whole number of
    /// text rows, so a limit counted in rows would have to be converted into the units the
    /// clamp actually works in and would land somewhere between two of them. The mock-up's own
    /// list — 120, 240, 480 — is in pixels for that reason and this stores exactly what it
    /// offers.
    ///
    /// This crate does not clamp it. A file naming a height this build's picker does not offer
    /// is honoured as written — the number is meaningful at every value, unlike a scheme name or
    /// a profile id — and the picker simply shows no tick. See `settings::block_max_height_index`.
    pub block_max_height: u32,
    /// Which profile a new tab — and the window's opening tab — starts from.
    ///
    /// **A profile id, never an index.** The mock-up's `state.defaultProfile` is
    /// a number into its `PROFILES` array, and it can be, because that array is a
    /// literal in the same file. Here the list is a product table that reorders
    /// between builds and a number would silently come to mean a different shell
    /// — `docs/DESIGN.md` §7.1.4's "稳定 profile_id（不是标题、不是展示对象）" is
    /// the same rule this file already follows for a session's leaves.
    ///
    /// This crate does not validate it. An id naming a profile the reading build
    /// does not have is the ordinary case rather than corruption — a profile
    /// removed from the table, or a file written by a newer build — and the
    /// reader's answer is `§5.4 逐叶降级`: fall to the profile it can always
    /// start. [`DEFAULT_PROFILE_UNSET`] is the same case reached from the other
    /// side.
    pub default_profile: String,
    /// Whether a Files column offers its second page at all (user ruling,
    /// 2026-08-15).
    ///
    /// **The Git panel's master switch, and it is a switch and not a preference.**
    /// Off is not "the page is hidden": it is the page not existing — no `Files |
    /// Git` strip above the tree, no chord that reaches it, and, the reason the
    /// switch was asked for, **not one process spawned against the repository**.
    /// A product that reads a git repository whenever a folder is open owes the
    /// user a way to say no that is actually a no, and a switch that merely hid
    /// the drawing would not be one.
    ///
    /// On by default. The panel is the feature this build shipped; a feature that
    /// arrives switched off is a feature nobody finds.
    pub git_panel: bool,
    /// Which way a split that was never told a direction cuts (user ruling,
    /// 2026-08-16).
    ///
    /// **It governs only the splits that have no direction of their own**, and
    /// that is the whole of the setting rather than a caveat about it. `Alt+Shift+-`
    /// draws a horizontal rule and `Alt+Shift+=` a vertical one; the four zones of
    /// the pane menu's picker *are* four directions. None of those five asks this
    /// question. What asks it is every verb whose sentence stops at "split": the
    /// duplicate chord, `Split with…`, `New terminal in folder…`, `Duplicate pane`
    /// — and for seven months each of those silently answered `Auto`.
    ///
    /// [`SplitDirectionV1::Auto`] by default, because that is what the answer was
    /// before there was a question, and a setting that arrives having changed
    /// something is a setting that broke a habit to announce itself.
    pub split_direction: SplitDirectionV1,
    /// **Where a web preview's address field sends something that is not an
    /// address** (`docs/DESIGN.md` §7.7 ②, 方案 §0's five extras: 「非 URL 输入
    /// 走默认搜索引擎(可设)」).
    ///
    /// A named engine and not a URL template, and the reason is what a template
    /// would be: a string a person can put anything in, read out of a settings
    /// file, and handed to a browser engine — which is the one shape §3's whole
    /// URL policy exists to refuse. Three names, three constants in this build,
    /// and every one of them still goes through `webnav::address_bar` on its way
    /// out.
    pub search_engine: SearchEngineV1,
    /// Which language the window's own words are drawn in (user ruling,
    /// 2026-08-10; shipped 2026-08-17).
    ///
    /// **The mode, never the resolved language** — `ThemeModeV1`'s rule, and it
    /// is the same rule because it is the same shape of question. Someone who
    /// picks `System` is saying "ask Windows", and a file that recorded
    /// `Chinese` instead would freeze the Windows they had that day: switch the
    /// OS to English later and Folio would still come up Chinese, with nothing
    /// in the file to say the user had never asked for that.
    ///
    /// It lives here rather than in `session.json` for the reason `theme_mode`
    /// does: a language is a preference a person holds about a program, not a
    /// shape this one window happened to be left in. (`cursor_style` and
    /// `tab_layout` are in the session file, and that is history rather than a
    /// pattern to follow.)
    ///
    /// **Read exactly once, at startup.** The window's widths are measured and
    /// cached, and there is no language revision to invalidate them with — see
    /// `bt_app::i18n`, which argues it at length and owns the row's promise that
    /// a change applies at the next start.
    pub language: LanguageV1,
    /// Which face the **grid** is drawn in — never the window's own chrome
    /// (`docs/DESIGN.md` §7.1.6c-3b).
    ///
    /// The distinction is the field's whole meaning and not a caveat about it.
    /// A terminal's font is a monospace grid: every cell is one advance wide,
    /// the renderer measures that advance once and every glyph, box-drawing rule
    /// and cursor position is derived from it. The chrome's labels are set in a
    /// proportional sans whose cap height is measured at construction and treated
    /// as a property of the face; there is no field here that moves it, on
    /// purpose, and §7.1.6c-3b names the two things that would have to change
    /// first.
    ///
    /// A family **name**, never a path: the file the name resolves to moves with
    /// a Windows update, and two machines that both have "Cascadia Mono" do not
    /// have it in the same place. [`DEFAULT_TERMINAL_FONT_FAMILY`] is the
    /// unnamed case; a named family that this machine does not have degrades the
    /// same way, to the build's default face, per §5.4 逐叶降级.
    pub terminal_font_family: String,
    /// How large the grid's face is drawn, in **logical** pixels — the number
    /// before the monitor's scale factor multiplies it.
    ///
    /// Logical and not physical for the reason `theme_mode` stores a mode: a
    /// physical size would freeze the monitor the choice was made on, so moving
    /// the window to a 150% display would shrink the text the user had just
    /// sized. The renderer multiplies by the scale factor of whichever monitor
    /// the window is on, every time it changes, and has since the first frame.
    ///
    /// A `u8` because the row offers a list, not a spinner, and no monospace
    /// grid on a terminal wants three digits. This crate does not clamp it: a
    /// file written by a newer build may name a size this one's list does not
    /// contain, and the reader's answer is to draw at that size, which it can.
    pub terminal_font_size: u8,
    /// Whether the user has been offered the patched PSReadLine, and what they
    /// said (`docs/DESIGN.md` §7.1.6c-3b).
    ///
    /// It is a preference and not a fact about the machine, which is why it is in
    /// this file rather than being derived at startup from what is installed. The
    /// fact — which PSReadLine the machine actually has — is read out of band
    /// every run and is never written down, because it changes without Folio.
    /// What cannot be re-derived is whether this person has already been asked
    /// and said no; asking again every launch is the behaviour this field exists
    /// to make impossible.
    pub psreadline_invite: PsReadLineInviteV1,
    /// Which colour scheme the window is painted in while the theme resolves to
    /// Light (`docs/DESIGN.md` §7.1.6c-4a).
    ///
    /// **The whole window, grid and chrome alike** — and that is the opposite of
    /// `terminal_font_family`'s distinction, deliberately. A scheme is
    /// twenty-one colours: the ANSI sixteen, plus a background, a foreground, a
    /// cursor, a selection and an accent. The window's own hundred-and-thirty-odd
    /// surface colours are *derived* from those by the renderer and stored
    /// nowhere, which is what stops a scheme from being a skin: nobody has to
    /// name a divider, and no scheme can produce a window whose tab strip
    /// disagrees with the terminal beside it. Nothing about that derivation is
    /// this crate's business; what is stored here is a name.
    ///
    /// It is stored **beside** `theme_mode` rather than folded into it because
    /// the two answer different questions and a user changes them at different
    /// rates. `theme_mode` says light or dark, possibly by deferring to Windows;
    /// this says what light *looks like* when it comes. Someone who follows the
    /// system gets both of their choices honoured, one per side, without the
    /// file having to record which side happened to be showing the day they
    /// chose.
    ///
    /// A scheme **name**, never a path or an index: the file a scheme lives in
    /// moves when the user reorganises their schemes folder, and a number into a
    /// list that is part built-in and part user-supplied means a different
    /// palette the moment either half changes. This crate does not validate it —
    /// a name no scheme answers to is the ordinary case (deleted, renamed,
    /// written by a newer build), and the reader's answer is §5.4 逐叶降级: fall
    /// to the build's default palette, which is where
    /// [`DEFAULT_LIGHT_SCHEME`] arrives from the other side.
    pub light_scheme: String,
    /// Which colour scheme the grid is painted in while the theme resolves to
    /// Dark — [`Self::light_scheme`]'s twin, and everything said there holds
    /// here.
    ///
    /// Both are read on every startup and whenever the resolved theme flips;
    /// only one of the two is in force at a time, and which one is
    /// `theme_mode`'s answer, not this field's. The pair is why a user who runs
    /// Windows on a light-at-noon schedule does not get one palette forced on
    /// them at both ends of the day.
    pub dark_scheme: String,
    /// The file a picture is drawn from, behind the whole window and beneath
    /// every pane — [`DEFAULT_BACKGROUND_IMAGE`] when there is none.
    ///
    /// One path and not one per pane, because a split is two views of one place
    /// and a picture cut in half at every divider would move every time a
    /// divider did. It is also one path and not one per theme: a picture is not
    /// a colour, nothing about it is derived from the canvas, and a reader who
    /// wanted a different picture at night has said nothing that this file could
    /// have inferred from the two scheme rows above.
    pub background_image: String,
    /// How that picture meets a window that is not its shape.
    pub background_fit: BackgroundFitV1,
    /// How much of the picture reaches the window, 0–100 whole percent.
    ///
    /// A whole percentage and not a float, for two reasons that agree: the row
    /// that writes it is a slider stepping in fives, so no value between two
    /// integers can ever be produced; and a `f32` in this struct would cost
    /// [`SettingsV1`]'s `Eq`, which is what lets a settings write compare itself
    /// against what is already loaded and cost nothing when nothing moved.
    ///
    /// Out-of-range values are the reader's problem and not this crate's — a
    /// file written by hand may say 400, and the answer is the same one a
    /// missing scheme gets: clamp at the surface that has to draw it.
    pub background_image_opacity: u8,
    /// How much of the window's ground is there at all, 30–100 whole percent —
    /// [`MINIMUM_BACKGROUND_OPACITY`] is the floor and the reason it exists.
    ///
    /// The ground is the clear plus the panes' own fills. Everything drawn on
    /// top of it — every glyph, every menu, every dialog and float — stays
    /// opaque at every setting, which is not a policy this crate enforces but
    /// the shape of the renderer that reads it: only the clear carries this
    /// alpha, and every later draw blends over it.
    pub background_opacity: u8,
    /// Whether Windows blurs whatever is behind the window
    /// (`DWMWA_SYSTEMBACKDROP_TYPE` = `DWMSBT_TRANSIENTWINDOW`).
    ///
    /// Stored even where the running Windows has never heard of the attribute,
    /// because a settings file outlives the machine it was written on: a user
    /// who turns it on, copies their profile to a newer laptop and finds it off
    /// has been silently overruled by a build that had no right to an opinion.
    pub acrylic: bool,
    /// Whether the window sits above other windows (`HWND_TOPMOST`).
    ///
    /// A window posture and not a ground colour, and the one field in this block
    /// that is visible with a picture, a blur and an opacity all switched off.
    pub always_on_top: bool,
    /// **Which settings pages have their `Advanced` group open** (user ruling
    /// 2026-08-17, `docs/DESIGN.md` §7.1.6c-5).
    ///
    /// A list of page keys and not a flag per page, and the shape is the ruling
    /// read literally: disclosure is per page, so what is recorded is *which*
    /// pages a reader has opened — and a page that has never been opened leaves
    /// no line in the file, exactly as an unedited shortcut leaves none in
    /// `keybindings.json`. A fresh install writes `[]`.
    ///
    /// This crate does not validate the keys. A key naming a page the reading
    /// build has no page for is the ordinary case rather than corruption — a
    /// page retired between builds, or a file written by a newer one — and the
    /// reader's answer is §5.4 逐叶降级: `bt_app::settings::AdvancedOpen::from_keys`
    /// drops what it does not know and honours everything beside it.
    ///
    /// It is a preference and not a window shape, which is why it is here and
    /// not in `session.json`: a person who has decided they want to see the
    /// background-picture rows has decided it about the product, not about the
    /// window they happened to have open.
    #[serde(default)]
    pub advanced_open: Vec<String>,
    /// **How many lines of past output one terminal pane keeps** — the Terminal page's
    /// `Scrollback` row, and the frozen-history capacity `bt_transcript::TranscriptStore`
    /// enforces.
    ///
    /// **Lines and not bytes**, because a line is the unit the reader scrolls in and the
    /// unit the picture on the right edge is drawn from. Bytes would make the same file
    /// mean a different amount of history on a machine that prints wider lines, which is
    /// the one thing a number in a settings file must not do.
    ///
    /// **Never zero and never a sentinel**, which is where this key differs from
    /// [`SettingsV1::block_max_height`]. There, `0` is a legal answer that means "no
    /// limit"; here there is no such answer to spell, because P2-9 has ruled that真·无限
    /// 回滚 is not a thing this product does — unbounded history means writing output to
    /// disk, which is the "输出历史" honeypot under another name. Every value of this key
    /// is a real capacity, so a reader editing the file by hand cannot write a number that
    /// silently means its opposite.
    ///
    /// This crate does not clamp it. A file naming a capacity this build's picker does not
    /// offer is honoured as written — every positive number is meaningful, unlike a scheme
    /// name or a profile id — and the picker simply shows no tick. Zero is the one value
    /// that has no meaning as a capacity, and it is answered at the door rather than here:
    /// `bt_app::scrollback_quota` turns it into the same `NonZeroUsize` every other value
    /// becomes. See `settings::scrollback_index`.
    #[serde(default = "default_scrollback_lines")]
    pub scrollback_lines: u32,
    /// **What shape a new window opens in** — the Appearance page's `Focus mode` row, and the
    /// only half of that mode which outlives the process (DESIGN §7.1.6b′, §2.4 rule three).
    ///
    /// The bit a window is *currently* in lives on the window (`WindowRuntime::focus_mode`),
    /// because "what is this window doing right now" is a fact about one window and five doors
    /// can turn it. This key is the other half of the same `dwm_dark_mode` shape: the setting
    /// says what every *new* window is born as, and the row in Appearance is what makes the
    /// answer survive a restart. A layout mode somebody may live in has to be a place they can
    /// come back to; a mode that reset itself every launch would be a verb wearing a
    /// preference's clothes.
    ///
    /// `false` is the default and is not a judgement about the mode — it is what every build
    /// before v15 did, which is the sentence every migration in this file is written under.
    ///
    /// It is emphatically **not** a second spelling of `session.json`'s `tab_layout` /
    /// `sidebar_mode`. Focus mode supersedes the chrome those two describe while it is on and
    /// writes neither of them, so both survive it untouched and there is nothing to restore on
    /// the way out.
    #[serde(default)]
    pub focus_mode: bool,
    /// **The floor a cell's ink is held to against the paper it is printed on** — the
    /// Appearance page's `Minimum contrast` row (DESIGN §2.6).
    ///
    /// The one setting in this document that overrides a colour a *program* asked for, which
    /// is exactly why it exists: a scheme is free to give ANSI bright-black a value within a
    /// hair of its own background, and when it does, everything printed in that colour is
    /// gone. `Off` is the default and the honest one — a terminal's first duty is to show what
    /// was sent — but a reader who cannot read their own prompt is not being served by that
    /// duty, so the floor is offered rather than assumed.
    #[serde(default)]
    pub minimum_contrast: MinimumContrastV1,

    /// **Whether a program may put a message on the desktop** — the Terminal page's
    /// `Notifications` row, and the switch behind `OSC 9` / `OSC 777;notify` (DESIGN §7.6).
    ///
    /// **It is the door those sequences knock on, and not only the final write** (DESIGN
    /// §7.1.5o ③′, user ruling 2026-08-26). An announcement a program wrote down its own tty is a
    /// desktop message, which is this row's own sentence, so with the row off the lane is not
    /// walked at all — no trace, no bit set. It used to be `turn_end_notification` that decided
    /// this, which meant a build script's `OSC 9;deploy failed` went missing because somebody had
    /// turned off being told when their agent stopped talking. Two unrelated statements, one
    /// switch, and neither row's words hinted at it.
    ///
    /// Off is silence and not concealment: no toast is raised, and the pane's own unread dot goes
    /// on saying what it always said. It does not un-write the AppUserModelID — that key is where
    /// Windows keeps the *user's* choices about Folio's notifications, and taking it away would
    /// throw those away rather than honour them (see `bt_platform::Notifier`).
    ///
    /// `true` is the default, and it is a ruling with the terminals split down the middle behind
    /// it: foot, WezTerm and Ghostty ship these sequences enabled, Windows Terminal ships
    /// `compatibility.allowOSC777` off and iTerm2 ships its escape-generated alerts off. What
    /// decides it here is that Folio's own gate is the strict one — nothing is raised while the
    /// pane is on screen in a focused window — so the case the conservative terminals are
    /// guarding against, a `cat` of a hostile file interrupting the person watching it, cannot
    /// happen. A feature that has to be found in Settings before it works once is a feature most
    /// of its users never learn they have.
    #[serde(default = "default_terminal_notifications")]
    pub terminal_notifications: bool,

    /// **Whether a PowerShell pane with no integration is offered one** — the Terminal page's
    /// `Offer PowerShell integration` row, and what the notice strip's `Don't show again` writes
    /// (DESIGN §7.1.6j).
    ///
    /// `true` is the default because the offer is the only way the fact reaches anybody. Folio's
    /// PowerShell integration is opt-in by necessity — `pwsh` has one startup file and no
    /// argument that would source a second one after it — so a reader whose `$PROFILE` does not
    /// dot-source `folio.ps1` gets no prompt marks, no exit-code dots and no busy breathing, and
    /// nothing anywhere says so. The strip is the saying.
    ///
    /// Off is silence about the whole subject and nothing else: no pane is changed, no file is
    /// read, and a `$PROFILE` that already loads the script was never going to be asked about
    /// anyway. It is a switch on the asking, which is why it is one boolean and not a per-pane
    /// record — a reader who says "stop asking" has answered for every PowerShell they will ever
    /// open, and a table of dismissed panes would ask again tomorrow from a new one.
    #[serde(default = "default_powershell_integration_offer")]
    pub powershell_integration_offer: bool,
    /// **How tall a focus card's body stands**, in logical pixels — the Appearance page's
    /// `Focus card height` row (`docs/DESIGN.md` §7.1.6b′, user ruling 2026-08-21).
    ///
    /// A quantity and not a name, so it is written as the number: 160, 240 or 320 are the rungs
    /// this build's picker offers, and 160 is what every card was before the row existed.
    ///
    /// **Carried through unclamped**, on [`SettingsV1::block_max_height`]'s footing and for its
    /// reason: every positive value is a real height, this crate has no picker to check against,
    /// and a reader that snapped a file's number to the nearest rung would be reporting a value
    /// the file does not contain. A height this build's list does not offer simply shows no tick
    /// — see `settings::SettingsRow::selected_index`.
    #[serde(default = "default_focus_card_height")]
    pub focus_card_height: u32,
    /// **Whether entering Cards still owes the reader the `Alt`+wheel bubble** — spent by the
    /// bubble itself the first time it is shown, and given back by `Reset to defaults` on the
    /// Appearance page, which is the page Cards lives on (`docs/DESIGN.md` §7.21, user ruling
    /// 2026-08-27).
    ///
    /// **It has no row of its own, and that is the ruling rather than an omission.** A row would
    /// be a switch for turning off something that has already happened once and will not happen
    /// again, which is a control with no reader — and putting a new surface on the glass for one
    /// gesture is the very trade this hint exists to stop being made.
    ///
    /// `true` is the default because the gesture it names has no other door. `Alt`+wheel over a
    /// card scrolls that card's own window, and the 2026-08-26 audit filed it 丙1 — invented
    /// here, and with not one clue on the glass: the column refuses tooltips, a card carries no
    /// chevron, the wheel is not in the shortcut table, and the only sentence anywhere about it
    /// is on the tail of a settings row about *height*. A feature that can only be found by
    /// holding down a key nobody was told about does not exist.
    ///
    /// **A receipt and not a taste, and it lives here anyway.** A reader never sets this to
    /// `false`; the window does, on the frame the bubble first appears. It is in this file rather
    /// than in a record of its own because [`SettingsV1::powershell_integration_offer`] is the
    /// same bit — *does this window still owe this reader this sentence* — and a second file for
    /// the second one of them would be a second answer to one question.
    #[serde(default = "default_cards_gesture_hint_offer")]
    pub cards_gesture_hint_offer: bool,
    /// **Whether a line too long for the pane wraps onto the next row** — the Terminal page's
    /// `Line wrapping` row (`docs/plans/horizontal-scroll/plan.md`, ladder one).
    ///
    /// `true` is the wrap every terminal in this product's history has done, and it is the
    /// default for exactly that reason. `false` flattens each logical line onto one presentation
    /// row and reads it through a horizontal window, which is a different document on screen —
    /// different row counts, a different scroll extent, a different answer to "which row is this
    /// anchor on" — and therefore a member of `bt_doc::LayoutKey` as well as a key here.
    ///
    /// **It says nothing to the program.** The grid stays exactly as wide as it was, `DECAWM` is
    /// untouched, and a child that asks its size gets the same answer either way; what moves is
    /// only how this window presents what it already stored. A setting that changed the child's
    /// world would be ladder two, which is spiked and not built.
    #[serde(default = "default_line_wrapping")]
    pub line_wrapping: bool,
    /// **Whether a modifier held on its own raises the card that lists what it starts** — the
    /// General page's `Shortcut hints` row (`docs/DESIGN.md` §7.1.5e′).
    ///
    /// `true`, because the card is an offer rather than a change: it appears only after a hand has
    /// held modifiers for the better part of a second without pressing anything, it never takes a
    /// key, and it leaves the instant one is pressed. A reader who does not want it switches it off
    /// once and this key remembers.
    ///
    /// **It says nothing to the shortcut table.** Every chord is bound, in force and dispatched
    /// exactly as it was with this off; what the key governs is whether the window offers to *say
    /// so*. That is why it is a settings key and not a member of any layout's identity.
    #[serde(default = "default_key_hints")]
    pub key_hints: bool,
    /// **Whether the end of a turn is allowed to reach the desktop** — the **Agents** page's
    /// `Turn finished` row (`docs/plans/attention/plan.md` §11.7, user ruling 2026-08-25; the row
    /// moved off the Terminal page by the categorisation ruling of the same day, and this sentence
    /// said `Terminal` until 2026-08-26 — `bt_app::settings::SettingsRow::category` is the one
    /// that decides).
    ///
    /// One key and not two, though the ruling names two arms — a taskbar flash on a window that
    /// has not got the keyboard, and a toast on one that is minimised. "May the end of a turn
    /// reach the desktop" is one sentence, and two switches would make legal two combinations
    /// nobody asked for: flashing without toasting, and toasting without flashing. If the two are
    /// ever wanted apart that is a second row, not a changed judgement.
    ///
    /// **What it does *not* cover is a message a program wrote itself** (DESIGN §7.1.5o ③′, user
    /// ruling 2026-08-26). This row's sentence is about an agent going quiet, so it governs the
    /// arrivals that say only that — a hook `Stop`, a bare bell, an `OSC 1337;…=once` arm, a codex
    /// `notify`, a pi `agent_settled`. An `OSC 9;<text>` or `OSC 777;notify` carries the program's
    /// own words and answers to `terminal_notifications` instead. Which door an arrival knocks on
    /// is decided by *who is speaking*, in one exhaustive match
    /// (`bt_app::attention::NotificationSwitches::admits`), so a new way of hearing about a turn
    /// ending has to choose a row at compile time.
    ///
    /// **It governs how far, never what the pane says.** Off leaves the bell dot, the unread dot
    /// and the attention queue's own badge exactly as they were: this key is about the desktop
    /// outside the window, which is the division `terminal_notifications` is filed under and the
    /// one the ledger's own predicate is kept clear of.
    ///
    /// `true` is the default and it is the ruling's own. The arm that runs most of the time costs
    /// the reader a flash on a taskbar button they are not currently looking at, and a feature
    /// that has to be found in Settings before it works once is a feature most of its users never
    /// learn they have.
    #[serde(default = "default_turn_end_notification")]
    pub turn_end_notification: bool,
    /// **Whether dragging a selection in the terminal writes it to the clipboard the moment the
    /// drag lets go** — no row of its own until the 2026-08-26 gesture audit gave it one
    /// (`docs/plans/ui-style/invisible-gestures-2026-08-26.md`, 丙4).
    ///
    /// The audit's whole complaint is that the write is **invisible**: a hand drags across three
    /// lines, lets go, and the clipboard now holds those three lines with nothing on screen having
    /// said so — no flash, no toast, no mark on the selection. A person who then presses `Ctrl+C`
    /// expecting the interrupt they always get, or pastes somewhere else expecting whatever they
    /// copied last, finds out the clipboard moved only by what comes out of the paste. Windows
    /// Terminal's own `copyOnSelect` ships `false` for the same reason, so a reader arriving from
    /// that product carries no muscle memory that would make the write expected either.
    ///
    /// `true` is the default anyway, and it is not this row choosing the behaviour — see
    /// [`SETTINGS_SCHEMA_VERSION`]'s v24 note. Every build before this key existed already copied a
    /// drag's selection the instant it was let go; the key exists so that behaviour has a name and
    /// a door, and a reader who wants the interrupt back on a plain drag now has one press that
    /// closes it.
    #[serde(default = "default_copy_on_select")]
    pub copy_on_select: bool,
    /// **Whether this build asks the releases page whether a newer one exists** (v26,
    /// `docs/DESIGN.md` §7.51).
    ///
    /// The only key in this file that governs a network request, and the request it governs is
    /// one `GET` of one fixed address, at most once a day, carrying nothing about the machine or
    /// the person at it. What comes back is a version number; what is done with it is a mark on
    /// the gear and a line in this dialog. Nothing is downloaded and nothing is replaced, whatever
    /// this key says.
    ///
    /// Off, the thread is never started and `update-check.json` is never written. It is not a
    /// quieter check — there is no check.
    #[serde(default = "default_update_check")]
    pub update_check: bool,
    /// **How much of the monitor's work area the summoned terminal covers**, as a whole
    /// percentage of its height (v27, `docs/DESIGN.md` §7.54).
    ///
    /// A percentage and not a pixel count, because the window this describes is not placed by a
    /// hand: it is computed afresh at every summon against whichever monitor the pointer is over,
    /// and a height in pixels would mean one thing on the laptop panel and another on the desk
    /// screen. The width is not a setting at all — the window spans the work area, which is what
    /// makes it the shape it is.
    ///
    /// Out-of-range values are the reader's problem and not this crate's, on
    /// [`Self::background_image_opacity`]'s own terms: a file written by hand may say 400, and the
    /// answer is to clamp at the surface that has to place the window.
    #[serde(default = "default_quake_height")]
    pub quake_height: u8,
    /// **How wide the summoned terminal opens**, as a whole percentage of that same work area
    /// (v28, `docs/DESIGN.md` §7.54).
    ///
    /// [`Self::quake_height`]'s twin on the other axis, and it exists because the sentence that
    /// used to stand in that field's note — "the width is not a setting at all" — turned out to be
    /// a claim about a 16:9 laptop panel. What the window is *centred* in is not stored anywhere
    /// and cannot be: a percentage and a work area are the whole of the rectangle, so there is no
    /// second number here that could ever disagree with them.
    ///
    /// Out-of-range values are the reader's problem and not this crate's, on
    /// [`Self::quake_height`]'s own terms: the clamp belongs at the surface that places the window.
    #[serde(default = "default_quake_width")]
    pub quake_width: u8,
    /// **Whether the summoned terminal goes away when the keyboard leaves it** (v27).
    ///
    /// On, because the window is above every other window by construction: a reader who clicks
    /// back into the editor they summoned the terminal over has said what they want, and a strip
    /// that stays in front of it afterwards is covering the thing they just asked to look at. Off
    /// is for the reader who summons a shell to watch it while they work — a real use, which is
    /// why the row exists, and not the one the key is usually pressed for.
    #[serde(default = "default_quake_dismiss_on_blur")]
    pub quake_dismiss_on_blur: bool,
}

/// `serde`'s door for a v14 key that is missing from a file this build is reading.
///
/// A function rather than `#[serde(default)]`'s `Default::default()`, because a `u32`'s own
/// default is `0` and zero lines is not a capacity — a file that had lost this key would
/// come back as a pane that keeps nothing.
fn default_scrollback_lines() -> u32 {
    DEFAULT_SCROLLBACK_LINES
}

/// `serde`'s door for the v27 height key, missing from every file written before it.
///
/// A function and not `#[serde(default)]`, for [`default_scrollback_lines`]'s reason: a `u8`'s own
/// default is `0`, and a window nought percent of the screen tall is not a smaller window, it is
/// no window at all.
fn default_quake_height() -> u8 {
    DEFAULT_QUAKE_HEIGHT
}

/// `serde`'s door for the v28 width key, missing from every file written before it.
///
/// [`default_quake_height`]'s twin and for its reason: a `u8`'s own default is `0`, and a window
/// nought percent of the screen wide is not a narrower window, it is no window at all.
fn default_quake_width() -> u8 {
    DEFAULT_QUAKE_WIDTH
}

/// And the v27 dismissal key. A function because the answer is `true` and `bool::default()` is
/// `false` — a file that had lost this key would come back with a terminal standing in front of
/// whatever the reader turned to next.
fn default_quake_dismiss_on_blur() -> bool {
    true
}

/// `serde`'s door for a v15 key missing from a file this build is reading.
///
/// A function rather than `#[serde(default)]` for [`default_scrollback_lines`]'s reason turned
/// round: a `bool`'s own default is `false`, and this key's default is `true`. A file that lost
/// the key would otherwise come back silent, which is the one answer its owner cannot be assumed
/// to have given.
fn default_terminal_notifications() -> bool {
    true
}

/// `serde`'s door for a v18 key missing from a file this build is reading.
///
/// [`default_terminal_notifications`]'s twin and for its reason: the default is `true` and a
/// `bool`'s own default is `false`, so a file that lost the key would come back as a reader who
/// had pressed `Don't show again` — the one answer this product must not put in somebody's mouth.
fn default_powershell_integration_offer() -> bool {
    true
}

/// `true`, and the word in the field's name is what makes that readable: an *offer* defaults to
/// being owed. See [`SettingsV1::cards_gesture_hint_offer`].
fn default_cards_gesture_hint_offer() -> bool {
    true
}

/// [`default_scrollback_lines`]'s reason exactly: a `u32`'s own default is `0`, and a card with
/// no body at all is not a height anybody chose — it is the field having been dropped.
fn default_focus_card_height() -> u32 {
    DEFAULT_FOCUS_CARD_HEIGHT
}

/// `serde`'s door for a v21 key missing from a file this build is reading.
///
/// [`default_terminal_notifications`]'s reason exactly: a `bool`'s own default is `false`, and a
/// file that had lost this key would come back as a pane that stopped wrapping — a document nobody
/// asked for, on the strength of an absent line.
fn default_line_wrapping() -> bool {
    true
}

/// `serde`'s door for a v22 key missing from a file this build is reading.
///
/// [`default_powershell_integration_offer`]'s reason exactly, and it is the same *kind* of key: a
/// `bool`'s own default is `false`, so a file that had lost this one would come back as a reader
/// who had switched the offer off — the one answer this product must not put in somebody's mouth.
fn default_key_hints() -> bool {
    true
}

/// `serde`'s door for a v23 key missing from a file this build is reading.
///
/// [`default_terminal_notifications`]'s reason exactly: a `bool`'s own default is `false`, and a
/// file that had lost this key would come back as a reader who had switched the turn-end lane off
/// — an answer they never gave, arrived at from an absent line.
fn default_turn_end_notification() -> bool {
    true
}

/// `serde`'s door for a v24 key missing from a file this build is reading.
///
/// [`default_line_wrapping`]'s reason exactly, and it is the same *kind* of key: a `bool`'s own
/// default is `false`, and a file that had lost this one would come back as a pane that had
/// stopped copying a drag's selection — a habit every build before this key existed actually had,
/// silently broken on the strength of an absent line.
fn default_copy_on_select() -> bool {
    true
}

/// `serde`'s door for a v26 key missing from a file this build is reading.
///
/// A function and not `#[serde(default)]` for the reason every `bool` above it has one: `false` is
/// what a `bool` answers on its own, and `false` here is a reader who switched the check off — an
/// answer they never gave, arrived at from an absent line.
fn default_update_check() -> bool {
    true
}

impl Default for SettingsV1 {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            theme_mode: ThemeModeV1::default(),
            display_formulas: true,
            inline_formulas: true,
            tables: true,
            block_max_height: DEFAULT_BLOCK_MAX_HEIGHT,
            default_profile: DEFAULT_PROFILE_UNSET.to_owned(),
            git_panel: true,
            split_direction: SplitDirectionV1::default(),
            search_engine: SearchEngineV1::default(),
            language: LanguageV1::default(),
            terminal_font_family: DEFAULT_TERMINAL_FONT_FAMILY.to_owned(),
            terminal_font_size: DEFAULT_TERMINAL_FONT_SIZE,
            psreadline_invite: PsReadLineInviteV1::default(),
            light_scheme: DEFAULT_LIGHT_SCHEME.to_owned(),
            dark_scheme: DEFAULT_DARK_SCHEME.to_owned(),
            background_image: DEFAULT_BACKGROUND_IMAGE.to_owned(),
            background_fit: BackgroundFitV1::default(),
            background_image_opacity: DEFAULT_BACKGROUND_IMAGE_OPACITY,
            background_opacity: DEFAULT_BACKGROUND_OPACITY,
            acrylic: false,
            always_on_top: false,
            // Every group shut, which is the ruling's own default: progressive
            // disclosure that arrived already disclosed would be a longer page
            // with a triangle on it.
            advanced_open: Vec::new(),
            scrollback_lines: DEFAULT_SCROLLBACK_LINES,
            // The shape every window this product has ever opened in.
            focus_mode: false,
            // Every colour a program asks for, drawn as it was asked for.
            minimum_contrast: MinimumContrastV1::Off,

            terminal_notifications: true,
            // A PowerShell with no integration is told so, once, in its own pane.
            powershell_integration_offer: true,
            // The body every card has stood on since F2 — see
            // `DEFAULT_FOCUS_CARD_HEIGHT`.
            focus_card_height: DEFAULT_FOCUS_CARD_HEIGHT,
            // What every terminal this product has ever drawn did with a line too long for its
            // pane.
            line_wrapping: true,
            // A hand that has stopped on its modifiers is offered the list, once it has really
            // stopped.
            key_hints: true,
            turn_end_notification: true,
            // A reader who has never been in Cards is owed the one sentence that makes the
            // gesture in it findable.
            cards_gesture_hint_offer: true,
            // A drag that lets go of a selection has always written it to the clipboard; the row
            // gives that habit a name rather than choosing it.
            copy_on_select: true,
            // A preview has no other way to say that it has been superseded.
            update_check: true,
            // Tall enough to read a command's output, short enough that the window it came down
            // over is still there under it.
            quake_height: DEFAULT_QUAKE_HEIGHT,
            // Wide enough that nothing an ordinary terminal draws has to wrap, narrow enough that
            // one line of it is read without crossing a 4K desk.
            quake_width: DEFAULT_QUAKE_WIDTH,
            // A window that is above every other window goes away when the reader turns to one of
            // them.
            quake_dismiss_on_blur: true,
        }
    }
}

/// How a background picture meets a window that is not its shape —
/// `docs/DESIGN.md` §7.1.6c-4b.
///
/// Three values and not a scale factor, because each one is a different
/// sentence about what may be lost, and there is no number that interpolates
/// between "the aspect ratio" and "the edges".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BackgroundFitV1 {
    /// The picture takes the window's shape. Nothing is cropped and the aspect
    /// ratio is whatever the window's is.
    Stretch,
    /// The picture keeps its own shape and covers the window, the overflowing
    /// edge cropped evenly on both sides. The default: it is the only one of the
    /// three that both fills the window and leaves the picture looking like
    /// itself, and it is what every desktop in this product's world calls
    /// "fill".
    #[default]
    Fill,
    /// The picture repeats at its own pixel size from the window's top-left.
    /// Nothing is scaled and nothing is cropped except by the window's edge —
    /// the answer for a texture rather than a photograph.
    Tile,
}

/// How far the PSReadLine invitation has got with this user — `docs/DESIGN.md`
/// §7.1.6c-3b.
///
/// Four values and not a `bool`, because the invitation has to distinguish three
/// kinds of "do not show this again" that behave differently the next time the
/// window opens, and a two-state field would have to guess between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PsReadLineInviteV1 {
    /// Never offered. The only state from which the dialog appears unprompted.
    #[default]
    NotAsked,
    /// Offered once and refused. The dialog is owed exactly one more appearance,
    /// and only immediately after the user changes the font size — the one
    /// action whose visible symptom on an unpatched 5.1 is the bug the patch
    /// fixes. After that it goes to [`Self::Dismissed`] whatever the answer.
    Declined,
    /// Folio wrote the module. The Terminal page offers to remove it; nothing
    /// asks again.
    Installed,
    /// Refused twice, or refused after the second showing. Nothing asks again,
    /// ever, on this machine — the Terminal page's row remains the only way in.
    Dismissed,
}

/// Which language the interface is written in — `docs/DESIGN.md` §7.1.6c-3.
///
/// Three values, of which one is not a language: `System` is the answer "ask the
/// operating system", stored as itself so that it goes on meaning that. The two
/// named ones are the two columns `bt_app::i18n`'s table has; a third language
/// is a fourth variant here and a third literal per arm there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LanguageV1 {
    #[default]
    System,
    English,
    Chinese,
}

/// Which way a direction-less split cuts — `docs/DESIGN.md` §7.1.6.
///
/// Three values and not two plus a boolean, because `Auto` is not "no choice":
/// it is a rule (cut across the pane's longer side, so both halves come out as
/// square as the pane allows) and it is the one Windows Terminal's
/// `duplicatePane` takes by default. A user who picks `Right` is turning that
/// rule off, not declining to answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SplitDirectionV1 {
    /// Across the pane's longer side.
    #[default]
    Auto,
    /// Always side by side, the new pane on the right.
    Right,
    /// Always stacked, the new pane below.
    Down,
}

/// Which engine a web preview's address field hands a non-address to —
/// `docs/DESIGN.md` §7.7 ②.
///
/// **Three names rather than a template.** A URL template in a settings file is
/// a string a person can put anything into, and the thing on the other end of it
/// is a browser engine — which is precisely the shape §3's URL policy spends
/// three pages refusing. So the file carries a *name*, this build carries the
/// three addresses, and the composed URL goes out through `webnav::address_bar`
/// exactly as a typed one does.
///
/// **DuckDuckGo is the default**, and the argument is the address field's own
/// job: what a reader types is what they get. It is the one of the three that
/// needs no account and no cookie to answer, and that returns the same page in
/// every region — so an address somebody reads out of this window is an address
/// somebody else can retype. The other two are here because a person who has an
/// account with one of them is not being asked to give it up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SearchEngineV1 {
    #[default]
    DuckDuckGo,
    Bing,
    Google,
}

/// The contrast floor a terminal cell's ink is held to against its own paper — `docs/DESIGN.md`
/// §2.6.
///
/// Four values, and they are Windows Terminal's four to the letter (`Off`, `2:1`, `3:1`,
/// `4.5:1`) because this is one of the few rows where a user arriving from that product carries
/// a number in their head: somebody who has typed `"experimental.minimumContrastRatio": 4.5`
/// means the WCAG AA text ratio, and a Folio that offered `4:1` beside it would be asking them
/// to re-derive an answer they already have. The three ratios are the accessibility bars
/// themselves — 3:1 is WCAG AA for large text and for non-text objects, 4.5:1 is AA for body
/// text, and 2:1 is below both on purpose: it is the "just make it visible" rung, for a reader
/// who wants their scheme back the moment it stops being invisible rather than made compliant.
///
/// Named `Ratio45` rather than `Ratio4_5` because these are wire values (§2's PascalCase
/// pin), and a variant with an underscore in it would be a key with an underscore in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MinimumContrastV1 {
    /// Every colour drawn as the program asked for it.
    #[default]
    Off,
    /// 2:1 — visible, and no claim beyond that.
    Ratio2,
    /// 3:1 — WCAG AA for large text and non-text objects.
    Ratio3,
    /// 4.5:1 — WCAG AA for body text.
    Ratio45,
}

/// `docs/DESIGN.md` §7.1.6: "主题 System/Light/Dark 跟随系统".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemeModeV1 {
    #[default]
    System,
    Light,
    Dark,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_system_theme_at_current_version() {
        let defaults = SettingsV1::default();
        assert_eq!(defaults.schema_version, SETTINGS_SCHEMA_VERSION);
        assert_eq!(defaults.theme_mode, ThemeModeV1::System);
    }

    /// PIN — a settings file that has never been asked about profiles names no
    /// profile, and says so with an id rather than with a number.
    ///
    /// The number is the trap this pins shut: the mock-up stores an *index*, and
    /// an index is the one spelling that survives a round trip while quietly
    /// changing meaning the day the profile table gains a row above it.
    #[test]
    fn the_default_profile_is_unchosen_and_spelled_as_an_id() {
        let defaults = SettingsV1::default();
        assert_eq!(defaults.default_profile, DEFAULT_PROFILE_UNSET);
        let wire = serde_json::to_value(&defaults).unwrap();
        assert_eq!(wire["default_profile"], serde_json::Value::from(""));
        assert!(
            wire["default_profile"].is_string(),
            "a profile is named, never numbered"
        );
    }

    /// PIN — a settings file that has never been asked which way a split goes
    /// answers `Auto`, and says so with a word rather than with a number.
    ///
    /// Both halves matter. `Auto` because it is what every direction-less split
    /// did before the setting existed, and a default that changed behaviour on
    /// upgrade would be the setting announcing itself by breaking a habit; a word
    /// because an ordinal would go on meaning `Right` the day a fourth direction
    /// is inserted above it — the same trap `default_profile` is pinned against
    /// one test up.
    #[test]
    fn a_split_with_no_direction_of_its_own_defaults_to_the_longer_edge() {
        let defaults = SettingsV1::default();
        assert_eq!(defaults.split_direction, SplitDirectionV1::Auto);
        assert_eq!(defaults.search_engine, SearchEngineV1::DuckDuckGo);
        let wire = serde_json::to_value(&defaults).unwrap();
        assert_eq!(wire["split_direction"], serde_json::Value::from("Auto"));
    }

    /// PIN — a settings file that has never been asked which language to speak
    /// answers `System`, and says so with a word.
    ///
    /// `System` because it is the answer every user got before the question
    /// existed — a machine's Windows is the only thing that has ever decided
    /// this — and a word for `default_profile`'s reason one test up: an ordinal
    /// would go on meaning `English` the day a language is inserted above it.
    #[test]
    fn a_settings_file_that_was_never_asked_follows_the_operating_system() {
        let defaults = SettingsV1::default();
        assert_eq!(defaults.language, LanguageV1::System);
        let wire = serde_json::to_value(&defaults).unwrap();
        assert_eq!(wire["language"], serde_json::Value::from("System"));
    }

    /// PIN — every language survives a round trip, which is the whole of what
    /// this field owes a reader.
    #[test]
    fn every_language_survives_a_round_trip_through_the_file() {
        for language in [LanguageV1::System, LanguageV1::English, LanguageV1::Chinese] {
            let settings = SettingsV1 {
                language,
                ..SettingsV1::default()
            };
            let text = serde_json::to_string(&settings).unwrap();
            let read: SettingsV1 = serde_json::from_str(&text).unwrap();
            assert_eq!(read.language, language);
            assert_eq!(read, settings);
        }
    }

    /// PIN — the round trip, which is the whole of what this field owes a reader:
    /// what was chosen is what comes back.
    #[test]
    fn every_split_direction_survives_a_round_trip_through_the_file() {
        for direction in [
            SplitDirectionV1::Auto,
            SplitDirectionV1::Right,
            SplitDirectionV1::Down,
        ] {
            let settings = SettingsV1 {
                split_direction: direction,
                ..SettingsV1::default()
            };
            let text = serde_json::to_string(&settings).unwrap();
            let read: SettingsV1 = serde_json::from_str(&text).unwrap();
            assert_eq!(read.split_direction, direction);
            assert_eq!(read, settings);
        }
    }

    /// PIN — a settings file that has never been asked which face to draw the
    /// grid in names no family, and says so with the empty string rather than
    /// with `"Consolas"`.
    ///
    /// The named default is the trap. `"Consolas"` on disk is indistinguishable
    /// from a user who opened the list and picked Consolas out of it, so the day
    /// the build's default face moves, every file ever written pins the old one
    /// — and this crate would be asserting that a particular family exists on a
    /// machine it knows nothing about. It is `default_profile`'s ruling, and the
    /// same empty string carries it.
    #[test]
    fn the_default_terminal_font_is_unnamed_and_sixteen_logical_pixels() {
        let defaults = SettingsV1::default();
        assert_eq!(defaults.terminal_font_family, DEFAULT_TERMINAL_FONT_FAMILY);
        assert_eq!(defaults.terminal_font_size, DEFAULT_TERMINAL_FONT_SIZE);
        assert_eq!(
            DEFAULT_TERMINAL_FONT_SIZE, 16,
            "16 logical pixels is what `bt_render::BASE_FONT_SIZE_LOGICAL_PX` has \
             been since the first frame; the row writes that answer down rather \
             than changing it"
        );
        let wire = serde_json::to_value(&defaults).unwrap();
        assert_eq!(wire["terminal_font_family"], serde_json::Value::from(""));
        assert!(
            wire["terminal_font_family"].is_string(),
            "a family is named, never numbered — an index into a machine's font \
             list means a different face on the next machine"
        );
        assert_eq!(wire["terminal_font_size"], serde_json::Value::from(16));
    }

    /// PIN — a settings file that has never been shown the PSReadLine
    /// invitation says `NotAsked`, and says it with a word.
    #[test]
    fn a_settings_file_that_was_never_shown_the_invitation_says_so() {
        let defaults = SettingsV1::default();
        assert_eq!(defaults.psreadline_invite, PsReadLineInviteV1::NotAsked);
        let wire = serde_json::to_value(&defaults).unwrap();
        assert_eq!(
            wire["psreadline_invite"],
            serde_json::Value::from("NotAsked")
        );
    }

    /// PIN — every invitation state survives a round trip, and the four are
    /// four rather than a `bool`, because each answers the next launch
    /// differently.
    #[test]
    fn every_invitation_state_survives_a_round_trip_through_the_file() {
        for state in [
            PsReadLineInviteV1::NotAsked,
            PsReadLineInviteV1::Declined,
            PsReadLineInviteV1::Installed,
            PsReadLineInviteV1::Dismissed,
        ] {
            let settings = SettingsV1 {
                psreadline_invite: state,
                ..SettingsV1::default()
            };
            let text = serde_json::to_string(&settings).unwrap();
            let read: SettingsV1 = serde_json::from_str(&text).unwrap();
            assert_eq!(read.psreadline_invite, state);
            assert_eq!(read, settings);
        }
    }

    /// PIN — a named family and a chosen size survive a round trip, including a
    /// size this build's own list does not offer.
    ///
    /// The odd size is the half worth pinning: a file written by a newer build
    /// may name 17, and this crate's job is to hand it back unchanged rather
    /// than to snap it onto a list it does not own. Clamping here would be the
    /// persistence layer holding an opinion about the row's options.
    #[test]
    fn a_chosen_family_and_size_survive_a_round_trip_including_an_unlisted_size() {
        for (family, size) in [
            ("Cascadia Mono", 14u8),
            ("MS Gothic", 24),
            ("Consolas", 17),
            ("", 10),
        ] {
            let settings = SettingsV1 {
                terminal_font_family: family.to_owned(),
                terminal_font_size: size,
                ..SettingsV1::default()
            };
            let text = serde_json::to_string(&settings).unwrap();
            let read: SettingsV1 = serde_json::from_str(&text).unwrap();
            assert_eq!(read.terminal_font_family, family);
            assert_eq!(read.terminal_font_size, size);
            assert_eq!(read, settings);
        }
    }

    /// PIN — a settings file that has never been asked which palette to paint
    /// the grid in names neither scheme, and says so with two empty strings
    /// rather than with this build's two default names.
    ///
    /// It is the font row's ruling one field over, and the trap is the same one
    /// wearing a different hat. `"Folio Dark"` on disk is indistinguishable from
    /// a user who opened the list and picked Folio Dark, so the day the built-in
    /// default palette is renamed, retired or improved, every file ever written
    /// pins the old name and the user never sees the new one. The pair is also
    /// pinned as a *pair*: a default that filled in only one side would leave
    /// the other to be guessed, which is the thing having two fields exists to
    /// prevent.
    #[test]
    fn the_default_schemes_are_unnamed_on_both_sides_of_the_theme() {
        let defaults = SettingsV1::default();
        assert_eq!(defaults.light_scheme, DEFAULT_LIGHT_SCHEME);
        assert_eq!(defaults.dark_scheme, DEFAULT_DARK_SCHEME);
        assert_eq!(
            DEFAULT_LIGHT_SCHEME, "",
            "an unnamed scheme means `this build's default palette`, which every \
             reader already handles because a named scheme may equally have been \
             deleted since"
        );
        assert_eq!(DEFAULT_DARK_SCHEME, "");
        let wire = serde_json::to_value(&defaults).unwrap();
        assert_eq!(wire["light_scheme"], serde_json::Value::from(""));
        assert_eq!(wire["dark_scheme"], serde_json::Value::from(""));
        assert!(
            wire["light_scheme"].is_string() && wire["dark_scheme"].is_string(),
            "a scheme is named, never numbered — an index into a list that is part \
             built-in and part user-supplied means a different palette the moment \
             either half changes"
        );
    }

    /// PIN — a chosen pair survives a round trip, including a name this build
    /// has never heard of and a pair that names one side only.
    ///
    /// Both of the odd cases are the point. A scheme this build cannot resolve
    /// is the ordinary case rather than corruption — deleted, renamed, or
    /// written by a newer build — and this crate's job is to hand the name back
    /// unchanged rather than to correct it against a table it does not own. And
    /// a file that names a dark scheme while leaving Light unset is a user who
    /// has only ever run dark; the empty side must stay empty rather than being
    /// helpfully filled in with the other one.
    #[test]
    fn a_chosen_pair_of_schemes_survives_a_round_trip_including_unknown_names() {
        for (light, dark) in [
            ("Solarized Light", "Solarized Dark"),
            ("", "Nord"),
            ("Folio Light", ""),
            ("a-scheme-this-build-never-heard-of", "Gruvbox Dark"),
            ("", ""),
        ] {
            let settings = SettingsV1 {
                light_scheme: light.to_owned(),
                dark_scheme: dark.to_owned(),
                ..SettingsV1::default()
            };
            let text = serde_json::to_string(&settings).unwrap();
            let read: SettingsV1 = serde_json::from_str(&text).unwrap();
            assert_eq!(read.light_scheme, light);
            assert_eq!(read.dark_scheme, dark);
            assert_eq!(read, settings);
        }
    }

    #[test]
    fn wire_values_match_spec_pascal_case() {
        assert_eq!(
            serde_json::to_string(&ThemeModeV1::System).unwrap(),
            "\"System\""
        );
        assert_eq!(
            serde_json::to_string(&ThemeModeV1::Light).unwrap(),
            "\"Light\""
        );
        assert_eq!(
            serde_json::to_string(&ThemeModeV1::Dark).unwrap(),
            "\"Dark\""
        );
        assert_eq!(
            serde_json::to_string(&BackgroundFitV1::Stretch).unwrap(),
            "\"Stretch\""
        );
        assert_eq!(
            serde_json::to_string(&BackgroundFitV1::Fill).unwrap(),
            "\"Fill\""
        );
        assert_eq!(
            serde_json::to_string(&BackgroundFitV1::Tile).unwrap(),
            "\"Tile\""
        );
    }

    /// PIN — a file that has never been asked about its ground reads as the
    /// ground every build before v10 drew.
    ///
    /// Five of the six are that behaviour recorded. The sixth, `background_fit`,
    /// is the one genuine choice, and it is unreachable until a picture is
    /// named — which is why it is allowed to be a real answer rather than an
    /// empty one, unlike the scheme and family fields above.
    #[test]
    fn a_settings_file_that_was_never_asked_draws_no_picture_on_an_opaque_ground() {
        let defaults = SettingsV1::default();
        assert_eq!(defaults.background_image, DEFAULT_BACKGROUND_IMAGE);
        assert_eq!(
            defaults.background_image, "",
            "an unnamed picture, for the reason a wallpaper differs from a \
             palette: there is no built-in one to fall back to"
        );
        assert_eq!(defaults.background_fit, BackgroundFitV1::Fill);
        assert_eq!(defaults.background_image_opacity, 100);
        assert_eq!(defaults.background_opacity, 100);
        assert!(!defaults.acrylic);
        assert!(!defaults.always_on_top);
        const {
            assert!(
                MINIMUM_BACKGROUND_OPACITY < DEFAULT_BACKGROUND_OPACITY,
                "the floor has to be under the default or the default is the floor"
            );
        }
    }

    /// PIN — every fit survives a round trip, and so does a ground whose four
    /// numbers are all off their defaults.
    ///
    /// The percentages are `u8` rather than `f32` and this is where that is
    /// worth something: `65` written is `65` read, on every machine, with no
    /// question about which of two neighbouring floats the file happened to
    /// carry. It is also what keeps [`SettingsV1`]'s `Eq`, which is how a
    /// settings write that moved nothing costs nothing.
    #[test]
    fn every_background_fit_survives_a_round_trip_with_its_ground() {
        for fit in [
            BackgroundFitV1::Stretch,
            BackgroundFitV1::Fill,
            BackgroundFitV1::Tile,
        ] {
            let settings = SettingsV1 {
                background_image: r"C:\Users\me\Pictures\ridge line.jpg".to_owned(),
                background_fit: fit,
                background_image_opacity: 45,
                background_opacity: 65,
                acrylic: true,
                always_on_top: true,
                ..SettingsV1::default()
            };
            let text = serde_json::to_string(&settings).unwrap();
            let read: SettingsV1 = serde_json::from_str(&text).unwrap();
            assert_eq!(read.background_fit, fit);
            assert_eq!(
                read.background_image,
                r"C:\Users\me\Pictures\ridge line.jpg"
            );
            assert_eq!(read.background_image_opacity, 45);
            assert_eq!(read.background_opacity, 65);
            assert!(read.acrylic);
            assert!(read.always_on_top);
            assert_eq!(read, settings);
        }
    }

    /// PIN — a percentage this crate cannot vouch for still round-trips.
    ///
    /// A hand-edited file may say 7 where the floor is 30, and this crate is not
    /// the place that argues with it: the same ruling `light_scheme` gets for a
    /// palette name nothing answers to (§5.4 逐叶降级). Clamping here would mean
    /// a file quietly rewritten on the first launch that read it, and the person
    /// who typed 7 would never learn that anything had happened.
    #[test]
    fn a_ground_opacity_under_the_floor_is_stored_as_written_and_argued_with_elsewhere() {
        let settings = SettingsV1 {
            background_opacity: 7,
            background_image_opacity: 200,
            ..SettingsV1::default()
        };
        let text = serde_json::to_string(&settings).unwrap();
        let read: SettingsV1 = serde_json::from_str(&text).unwrap();
        assert_eq!(read.background_opacity, 7);
        assert_eq!(read.background_image_opacity, 200);
    }
}
