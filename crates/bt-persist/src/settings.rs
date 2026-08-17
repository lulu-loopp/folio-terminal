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
/// `psreadline_invite`, v9 adds `light_scheme` and `dark_scheme`. §2's
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
pub const SETTINGS_SCHEMA_VERSION: u32 = 9;

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

/// `settings.json` v9 — docs/M2-persistence-schema-v1.md §2:
/// ```json
/// {
///   "schema_version": 9,
///   "theme_mode": "System" | "Light" | "Dark",
///   "display_formulas": true | false,
///   "inline_formulas": true | false,
///   "default_profile": "pwsh" | "wsl" | "gitbash" | "cmd" | "",
///   "git_panel": true | false,
///   "split_direction": "Auto" | "Right" | "Down",
///   "language": "System" | "English" | "Chinese",
///   "terminal_font_family": "Consolas" | "Cascadia Mono" | … | "",
///   "terminal_font_size": 10..=24,
///   "psreadline_invite": "NotAsked" | "Declined" | "Installed" | "Dismissed",
///   "light_scheme": "Solarized Light" | … | "",
///   "dark_scheme": "Nord" | … | ""
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
}

impl Default for SettingsV1 {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            theme_mode: ThemeModeV1::default(),
            display_formulas: true,
            inline_formulas: true,
            default_profile: DEFAULT_PROFILE_UNSET.to_owned(),
            git_panel: true,
            split_direction: SplitDirectionV1::default(),
            language: LanguageV1::default(),
            terminal_font_family: DEFAULT_TERMINAL_FONT_FAMILY.to_owned(),
            terminal_font_size: DEFAULT_TERMINAL_FONT_SIZE,
            psreadline_invite: PsReadLineInviteV1::default(),
            light_scheme: DEFAULT_LIGHT_SCHEME.to_owned(),
            dark_scheme: DEFAULT_DARK_SCHEME.to_owned(),
        }
    }
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
    }
}
