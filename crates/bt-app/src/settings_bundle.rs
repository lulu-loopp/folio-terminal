//! **Settings, carried by hand** — the export file and the import of one
//! (0.4.4 ticket 05; owner rulings 2026-09-22 and 2026-09-23).
//!
//! The file's shape and the reading of each part are `bt_persist::export`'s.
//! What lives here is the half that needs this application: which of the
//! reader's scheme files go into an export, and what an import *does* — which
//! setting changed, which door each part enters by, and what is named back to
//! the reader because it could not be put in force.
//!
//! # The three doors a part enters by
//!
//! **An import is not a fourth configuration entrance; it is the three files
//! arriving by hand** (`docs/RULES.md` row 33). Each part goes through the door
//! the same document goes through when somebody edits it:
//!
//! * `keybindings` — [`import_shortcuts`]: this build's defaults, then
//!   `Shortcuts::apply_overrides`, exactly as at launch. A refused line is an
//!   `OverrideFault` and is named; the rest land.
//! * `schemes` — [`plan_schemes`]: each document through `bt_persist::parse_scheme`,
//!   the reader a scheme file in the folder meets. One that will not parse is
//!   named by its file and not written; the rest are written into the folder,
//!   and a name already there is replaced.
//! * `profiles` — the parse-and-compare `Runtime::reread_profiles` runs on a
//!   live edit: an identical table is no news, a different one is put in force.
//! * `settings` — the settings file's own chain (migrated forward from an older
//!   build, refused whole from a newer one), then [`plan_settings`]: every value
//!   that differs becomes one [`SettingChange`], and `Runtime` puts each through
//!   the function a Settings press on that row calls. **Never by writing
//!   `settings.json` and waiting** — nothing re-reads that file during a run.
//!
//! # What an import leaves alone
//!
//! Four keys of `settings.json` are receipts about *this machine* and not
//! preferences: whether the first-run card has been put up here
//! (`first_run_card`), whether this machine owes its `$PROFILE` a line
//! (`powershell_install_pending`), whether the cards' gesture hint has been
//! shown here (`cards_gesture_hint_offer`, which has one spender and one
//! restorer and no third opinion — `the_cards_offer_is_spent_in_one_place_and_given_back_in_one`),
//! and whether a web page has ever committed here (`web_pages_used`, 0.4.5
//! ticket 60, which decides whether this profile is given a spare controller).
//! Another machine's answer to any of them is not a fact about this one, so they
//! are kept as they stand. A row this platform
//! does not have — `Acrylic` on a Mac, `Option key sends Alt` on Windows — is
//! stored as it came, so the file still says it for the machine it belongs to,
//! and named, because nothing on this screen will show it.

use std::collections::BTreeMap;

use bt_persist::{
    BackgroundFitV1, KeybindingsV1, LanguageV1, LaunchOpensV1, MinimumContrastV1,
    PsReadLineInviteV1, QuakeRestoreV1, SearchEngineV1, SettingsV1, SplitDirectionV1, ThemeModeV1,
    WebColorSchemeV1,
};
use serde_json::Value;

use crate::settings::SettingsRow;
use crate::shortcuts::{Override, Shortcuts};

/// Something an import could not put in force, named the way the reader would
/// look for it — a shortcut's id, a scheme's file, a settings row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImportFault {
    pub(crate) what: String,
    pub(crate) reason: String,
}

/// **One setting an import changes**, carrying the value it changes to.
///
/// One variant per door and not one per field: the three terminal-font keys
/// are one decision on one door (`apply_terminal_font`), and the two scheme
/// names are one each. A variant with no row in [`Self::row`] is a value that
/// is read where it is used, so storing it *is* putting it in force.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SettingChange {
    Theme(ThemeModeV1),
    DisplayFormulas(bool),
    InlineFormulas(bool),
    RepairRowBreaks(bool),
    Tables(bool),
    BlockMaxHeight(u32),
    DefaultProfile(String),
    GitPanel(bool),
    SplitDirection(SplitDirectionV1),
    SearchEngine(SearchEngineV1),
    Language(LanguageV1),
    TerminalFont {
        family: String,
        cjk_family: String,
        size: u8,
    },
    PsReadLineInvite(PsReadLineInviteV1),
    LightScheme(String),
    DarkScheme(String),
    BackgroundImage(String),
    BackgroundFit(BackgroundFitV1),
    ImageOpacity(u8),
    BackgroundOpacity(u8),
    Acrylic(bool),
    AlwaysOnTop(bool),
    AdvancedOpen(Vec<String>),
    Scrollback(u32),
    FocusMode(bool),
    MinimumContrast(MinimumContrastV1),
    TerminalNotifications(bool),
    PowerShellOffer(bool),
    FocusCardHeight(u32),
    LineWrapping(bool),
    KeyHints(bool),
    TurnEndNotification(bool),
    CopyOnSelect(bool),
    UpdateCheck(bool),
    QuakeHeight(u8),
    QuakeWidth(u8),
    QuakeDismiss(bool),
    QuakeProfile(String),
    QuakeCommand(String),
    QuakeTopGap(u32),
    QuakeRestore(QuakeRestoreV1),
    LaunchOpens(LaunchOpensV1),
    OptionSendsAlt(bool),
    MultilinePaste(bool),
    /// Which colour scheme a web pane asks its page for (0.4.4 ticket 09).
    WebPages(WebColorSchemeV1),
}

impl SettingChange {
    /// The Settings row this value is chosen on, for the platform question and
    /// for the name a report gives it. `None` for the two values no row shows.
    pub(crate) const fn row(&self) -> Option<SettingsRow> {
        Some(match self {
            Self::Theme(_) => SettingsRow::Theme,
            Self::DisplayFormulas(_) => SettingsRow::Formulas,
            Self::InlineFormulas(_) => SettingsRow::InlineFormulas,
            Self::RepairRowBreaks(_) => SettingsRow::RepairRowBreaks,
            Self::Tables(_) => SettingsRow::Tables,
            Self::BlockMaxHeight(_) => SettingsRow::BlockMaxHeight,
            Self::DefaultProfile(_) => SettingsRow::DefaultProfile,
            Self::GitPanel(_) => SettingsRow::GitPanel,
            Self::SplitDirection(_) => SettingsRow::SplitDirection,
            Self::SearchEngine(_) => SettingsRow::SearchEngine,
            Self::Language(_) => SettingsRow::Language,
            Self::TerminalFont { .. } => SettingsRow::TerminalFont,
            Self::LightScheme(_) => SettingsRow::LightScheme,
            Self::DarkScheme(_) => SettingsRow::DarkScheme,
            Self::BackgroundImage(_) => SettingsRow::BackgroundImage,
            Self::BackgroundFit(_) => SettingsRow::ImageFit,
            Self::ImageOpacity(_) => SettingsRow::ImageOpacity,
            Self::BackgroundOpacity(_) => SettingsRow::BackgroundOpacity,
            Self::Acrylic(_) => SettingsRow::Acrylic,
            Self::AlwaysOnTop(_) => SettingsRow::AlwaysOnTop,
            Self::Scrollback(_) => SettingsRow::Scrollback,
            Self::FocusMode(_) => SettingsRow::FocusMode,
            Self::MinimumContrast(_) => SettingsRow::MinimumContrast,
            Self::TerminalNotifications(_) => SettingsRow::Notifications,
            Self::PowerShellOffer(_) => SettingsRow::PowerShellOffer,
            Self::FocusCardHeight(_) => SettingsRow::FocusCardHeight,
            Self::LineWrapping(_) => SettingsRow::LineWrapping,
            Self::KeyHints(_) => SettingsRow::KeyHints,
            Self::TurnEndNotification(_) => SettingsRow::TurnEndNotifications,
            Self::CopyOnSelect(_) => SettingsRow::CopyOnSelect,
            Self::UpdateCheck(_) => SettingsRow::UpdateCheck,
            Self::QuakeHeight(_) => SettingsRow::QuakeHeight,
            Self::QuakeWidth(_) => SettingsRow::QuakeWidth,
            Self::QuakeDismiss(_) => SettingsRow::QuakeDismiss,
            Self::QuakeProfile(_) => SettingsRow::QuakeProfile,
            Self::QuakeCommand(_) => SettingsRow::QuakeCommand,
            Self::QuakeTopGap(_) => SettingsRow::QuakeTopGap,
            Self::QuakeRestore(_) => SettingsRow::QuakeRestore,
            Self::LaunchOpens(_) => SettingsRow::LaunchOpens,
            Self::OptionSendsAlt(_) => SettingsRow::OptionSendsAlt,
            Self::MultilinePaste(_) => SettingsRow::MultilinePaste,
            Self::WebPages(_) => SettingsRow::WebPages,
            // The two with no row: an answer the PSReadLine card was given, and
            // which pages' Advanced groups are open. Each is read where it is used.
            Self::PsReadLineInvite(_) | Self::AdvancedOpen(_) => {
                return None;
            }
        })
    }

    /// Write this value into a settings document — the whole of what the change
    /// *is*, which `Runtime` then puts through the row's own door and which a
    /// row this platform does not have is stored as.
    pub(crate) fn write_into(&self, settings: &mut SettingsV1) {
        match self.clone() {
            Self::Theme(value) => settings.theme_mode = value,
            Self::DisplayFormulas(value) => settings.display_formulas = value,
            Self::InlineFormulas(value) => settings.inline_formulas = value,
            Self::RepairRowBreaks(value) => settings.repair_row_breaks = value,
            Self::Tables(value) => settings.tables = value,
            Self::BlockMaxHeight(value) => settings.block_max_height = value,
            Self::DefaultProfile(value) => settings.default_profile = value,
            Self::GitPanel(value) => settings.git_panel = value,
            Self::SplitDirection(value) => settings.split_direction = value,
            Self::SearchEngine(value) => settings.search_engine = value,
            Self::Language(value) => settings.language = value,
            Self::TerminalFont {
                family,
                cjk_family,
                size,
            } => {
                settings.terminal_font_family = family;
                settings.terminal_cjk_font_family = cjk_family;
                settings.terminal_font_size = size;
            }
            Self::PsReadLineInvite(value) => settings.psreadline_invite = value,
            Self::LightScheme(value) => settings.light_scheme = value,
            Self::DarkScheme(value) => settings.dark_scheme = value,
            Self::BackgroundImage(value) => settings.background_image = value,
            Self::BackgroundFit(value) => settings.background_fit = value,
            Self::ImageOpacity(value) => settings.background_image_opacity = value,
            Self::BackgroundOpacity(value) => settings.background_opacity = value,
            Self::Acrylic(value) => settings.acrylic = value,
            Self::AlwaysOnTop(value) => settings.always_on_top = value,
            Self::AdvancedOpen(value) => settings.advanced_open = value,
            Self::Scrollback(value) => settings.scrollback_lines = value,
            Self::FocusMode(value) => settings.focus_mode = value,
            Self::MinimumContrast(value) => settings.minimum_contrast = value,
            Self::TerminalNotifications(value) => settings.terminal_notifications = value,
            Self::PowerShellOffer(value) => settings.powershell_integration_offer = value,
            Self::FocusCardHeight(value) => settings.focus_card_height = value,
            Self::LineWrapping(value) => settings.line_wrapping = value,
            Self::KeyHints(value) => settings.key_hints = value,
            Self::TurnEndNotification(value) => settings.turn_end_notification = value,
            Self::CopyOnSelect(value) => settings.copy_on_select = value,
            Self::UpdateCheck(value) => settings.update_check = value,
            Self::QuakeHeight(value) => settings.quake_height = value,
            Self::QuakeWidth(value) => settings.quake_width = value,
            Self::QuakeDismiss(value) => settings.quake_dismiss_on_blur = value,
            Self::QuakeProfile(value) => settings.quake_profile_id = value,
            Self::QuakeCommand(value) => settings.quake_startup_command = value,
            Self::QuakeTopGap(value) => settings.quake_top_gap = value,
            Self::QuakeRestore(value) => settings.quake_restore = value,
            Self::LaunchOpens(value) => settings.launch_opens = value,
            Self::OptionSendsAlt(value) => settings.option_sends_alt = value,
            Self::MultilinePaste(value) => settings.multiline_paste_ask = value,
            Self::WebPages(value) => settings.web_color_scheme = value,
        }
    }
}

/// What an imported settings part does, sorted by the platform it lands on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SettingsPlan {
    /// Every change a row on this platform answers for, in the order the
    /// fields are declared.
    pub(crate) apply: Vec<SettingChange>,
    /// Changes to rows this platform does not have: stored as they came and
    /// named, never put through a door that is not here.
    pub(crate) elsewhere: Vec<SettingChange>,
}

/// **Every value of `imported` that differs from `current`, as the change that
/// puts it in force** — and nothing for a value that is already so, which is
/// what makes an import of an export of this very state change nothing.
///
/// The document is taken apart field by field with no `..`, so a key added to
/// `SettingsV1` does not compile here until somebody has said which door it
/// enters by. The three machine receipts are named and left behind; see the
/// module header.
pub(crate) fn plan_settings(
    current: &SettingsV1,
    imported: &SettingsV1,
    platform: bt_platform::HostPlatform,
) -> SettingsPlan {
    let SettingsV1 {
        schema_version: _,
        theme_mode,
        display_formulas,
        inline_formulas,
        repair_row_breaks,
        tables,
        block_max_height,
        default_profile,
        git_panel,
        split_direction,
        search_engine,
        language,
        terminal_font_family,
        terminal_cjk_font_family,
        terminal_font_size,
        psreadline_invite,
        light_scheme,
        dark_scheme,
        background_image,
        background_fit,
        background_image_opacity,
        background_opacity,
        acrylic,
        always_on_top,
        advanced_open,
        scrollback_lines,
        focus_mode,
        minimum_contrast,
        terminal_notifications,
        powershell_integration_offer,
        focus_card_height,
        cards_gesture_hint_offer: _,
        line_wrapping,
        key_hints,
        turn_end_notification,
        copy_on_select,
        update_check,
        quake_height,
        quake_width,
        quake_dismiss_on_blur,
        quake_profile_id,
        quake_startup_command,
        quake_top_gap,
        quake_restore,
        // This machine's own receipts — see the module header, and the third
        // one above.
        first_run_card: _,
        powershell_install_pending: _,
        web_pages_used: _,
        launch_opens,
        option_sends_alt,
        multiline_paste_ask,
        web_color_scheme,
    } = imported.clone();
    let mut changes = Vec::new();
    let mut offer = |differs: bool, change: SettingChange| {
        if differs {
            changes.push(change);
        }
    };
    offer(
        current.theme_mode != theme_mode,
        SettingChange::Theme(theme_mode),
    );
    offer(
        current.display_formulas != display_formulas,
        SettingChange::DisplayFormulas(display_formulas),
    );
    offer(
        current.inline_formulas != inline_formulas,
        SettingChange::InlineFormulas(inline_formulas),
    );
    offer(
        current.repair_row_breaks != repair_row_breaks,
        SettingChange::RepairRowBreaks(repair_row_breaks),
    );
    offer(current.tables != tables, SettingChange::Tables(tables));
    offer(
        current.block_max_height != block_max_height,
        SettingChange::BlockMaxHeight(block_max_height),
    );
    offer(
        current.default_profile != default_profile,
        SettingChange::DefaultProfile(default_profile),
    );
    offer(
        current.git_panel != git_panel,
        SettingChange::GitPanel(git_panel),
    );
    offer(
        current.split_direction != split_direction,
        SettingChange::SplitDirection(split_direction),
    );
    offer(
        current.search_engine != search_engine,
        SettingChange::SearchEngine(search_engine),
    );
    offer(
        current.language != language,
        SettingChange::Language(language),
    );
    offer(
        current.terminal_font_family != terminal_font_family
            || current.terminal_cjk_font_family != terminal_cjk_font_family
            || current.terminal_font_size != terminal_font_size,
        SettingChange::TerminalFont {
            family: terminal_font_family,
            cjk_family: terminal_cjk_font_family,
            size: terminal_font_size,
        },
    );
    offer(
        current.psreadline_invite != psreadline_invite,
        SettingChange::PsReadLineInvite(psreadline_invite),
    );
    offer(
        current.light_scheme != light_scheme,
        SettingChange::LightScheme(light_scheme),
    );
    offer(
        current.dark_scheme != dark_scheme,
        SettingChange::DarkScheme(dark_scheme),
    );
    offer(
        current.background_image != background_image,
        SettingChange::BackgroundImage(background_image),
    );
    offer(
        current.background_fit != background_fit,
        SettingChange::BackgroundFit(background_fit),
    );
    offer(
        current.background_image_opacity != background_image_opacity,
        SettingChange::ImageOpacity(background_image_opacity),
    );
    offer(
        current.background_opacity != background_opacity,
        SettingChange::BackgroundOpacity(background_opacity),
    );
    offer(current.acrylic != acrylic, SettingChange::Acrylic(acrylic));
    offer(
        current.always_on_top != always_on_top,
        SettingChange::AlwaysOnTop(always_on_top),
    );
    offer(
        current.advanced_open != advanced_open,
        SettingChange::AdvancedOpen(advanced_open),
    );
    offer(
        current.scrollback_lines != scrollback_lines,
        SettingChange::Scrollback(scrollback_lines),
    );
    offer(
        current.focus_mode != focus_mode,
        SettingChange::FocusMode(focus_mode),
    );
    offer(
        current.minimum_contrast != minimum_contrast,
        SettingChange::MinimumContrast(minimum_contrast),
    );
    offer(
        current.terminal_notifications != terminal_notifications,
        SettingChange::TerminalNotifications(terminal_notifications),
    );
    offer(
        current.powershell_integration_offer != powershell_integration_offer,
        SettingChange::PowerShellOffer(powershell_integration_offer),
    );
    offer(
        current.focus_card_height != focus_card_height,
        SettingChange::FocusCardHeight(focus_card_height),
    );
    offer(
        current.line_wrapping != line_wrapping,
        SettingChange::LineWrapping(line_wrapping),
    );
    offer(
        current.key_hints != key_hints,
        SettingChange::KeyHints(key_hints),
    );
    offer(
        current.turn_end_notification != turn_end_notification,
        SettingChange::TurnEndNotification(turn_end_notification),
    );
    offer(
        current.copy_on_select != copy_on_select,
        SettingChange::CopyOnSelect(copy_on_select),
    );
    offer(
        current.update_check != update_check,
        SettingChange::UpdateCheck(update_check),
    );
    offer(
        current.quake_height != quake_height,
        SettingChange::QuakeHeight(quake_height),
    );
    offer(
        current.quake_width != quake_width,
        SettingChange::QuakeWidth(quake_width),
    );
    offer(
        current.quake_dismiss_on_blur != quake_dismiss_on_blur,
        SettingChange::QuakeDismiss(quake_dismiss_on_blur),
    );
    offer(
        current.quake_profile_id != quake_profile_id,
        SettingChange::QuakeProfile(quake_profile_id),
    );
    offer(
        current.quake_startup_command != quake_startup_command,
        SettingChange::QuakeCommand(quake_startup_command),
    );
    offer(
        current.quake_top_gap != quake_top_gap,
        SettingChange::QuakeTopGap(quake_top_gap),
    );
    offer(
        current.quake_restore != quake_restore,
        SettingChange::QuakeRestore(quake_restore),
    );
    offer(
        current.launch_opens != launch_opens,
        SettingChange::LaunchOpens(launch_opens),
    );
    offer(
        current.option_sends_alt != option_sends_alt,
        SettingChange::OptionSendsAlt(option_sends_alt),
    );
    offer(
        current.multiline_paste_ask != multiline_paste_ask,
        SettingChange::MultilinePaste(multiline_paste_ask),
    );
    offer(
        current.web_color_scheme != web_color_scheme,
        SettingChange::WebPages(web_color_scheme),
    );
    let (apply, elsewhere) = changes.into_iter().partition(|change| {
        change
            .row()
            .and_then(SettingsRow::needs)
            .is_none_or(|needed| needed.on(platform))
    });
    SettingsPlan { apply, elsewhere }
}

/// **A shortcut part, read the way `keybindings.json` is read at launch**:
/// this build's table, then the file's lines laid over it by
/// `Shortcuts::apply_overrides`, every refused line named by its id.
pub(crate) fn import_shortcuts(file: &KeybindingsV1) -> (Shortcuts, Vec<ImportFault>) {
    let mut table = Shortcuts::defaults();
    let overrides: Vec<Override> = file
        .bindings
        .iter()
        .map(|entry| Override {
            id: entry.action.clone(),
            chord: entry.chord.clone(),
        })
        .collect();
    let faults = table
        .apply_overrides(&overrides)
        .into_iter()
        .map(|fault| ImportFault {
            what: format!("{}: {}", crate::persist::KEYBINDINGS_FILE_NAME, fault.id),
            reason: fault.reason,
        })
        .collect();
    (table, faults)
}

/// What an imported `schemes` part writes into the folder, and what it names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SchemePlan {
    /// `(file name, text)` for every scheme that parses and is not already the
    /// same document in the folder.
    pub(crate) writes: Vec<(String, String)>,
    pub(crate) faults: Vec<ImportFault>,
}

/// **Judge each imported scheme the way a file dropped into the folder is
/// judged** — `bt_persist::parse_scheme` on the text that would be written —
/// and plan the writes.
///
/// A name that already holds the same document is left alone, so an import of
/// an export of this machine writes nothing. A name that holds something else
/// is replaced: the import is the reader's deliberate gesture. A key that is
/// not a plain `*.json` file name is refused, because the folder is the only
/// place this part may write.
pub(crate) fn plan_schemes(
    existing: &BTreeMap<String, Value>,
    imported: &BTreeMap<String, Value>,
) -> SchemePlan {
    let mut plan = SchemePlan::default();
    for (file, document) in imported {
        let fault = |reason: String| ImportFault {
            what: format!("{}/{file}", crate::schemes::USER_SCHEME_DIR),
            reason,
        };
        if !is_plain_scheme_file_name(file) {
            plan.faults.push(fault(
                "not a scheme file name this folder can hold".to_owned(),
            ));
            continue;
        }
        let Ok(mut text) = serde_json::to_string_pretty(document) else {
            plan.faults
                .push(fault("the scheme could not be written as JSON".to_owned()));
            continue;
        };
        text.push('\n');
        if let Err(reason) = bt_persist::parse_scheme(&text) {
            plan.faults.push(fault(reason.to_string()));
            continue;
        }
        if existing.get(file) == Some(document) {
            continue;
        }
        plan.writes.push((file.clone(), text));
    }
    plan
}

/// A bare `*.json` name: no directory in it, and not a name that means one.
fn is_plain_scheme_file_name(file: &str) -> bool {
    let path = std::path::Path::new(file);
    !file.contains(['/', '\\'])
        && path.file_name().and_then(std::ffi::OsStr::to_str) == Some(file)
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_persist::{
        BindingOverrideV1, ExportParts, KEYBINDINGS_SCHEMA_VERSION, ProfilesV1, SchemeFileV1,
    };

    fn scheme_document(name: &str) -> Value {
        let text = bt_persist::write_scheme(&SchemeFileV1 {
            name: name.to_owned(),
            background: [0x10, 0x20, 0x30],
            foreground: [0xe0, 0xe0, 0xe0],
            cursor: [0xe0, 0xe0, 0xe0],
            selection: [0x30, 0x40, 0x50],
            ansi: [[0x80, 0x80, 0x80]; 16],
            accent: [0x40, 0x80, 0xc0],
        });
        serde_json::from_str(&text).expect("a written scheme is JSON")
    }

    /// Every value of the document departing from `SettingsV1::default()`.
    fn departed() -> SettingsV1 {
        let base = SettingsV1::default();
        SettingsV1 {
            theme_mode: ThemeModeV1::Dark,
            display_formulas: !base.display_formulas,
            inline_formulas: !base.inline_formulas,
            repair_row_breaks: !base.repair_row_breaks,
            tables: !base.tables,
            block_max_height: base.block_max_height + 40,
            default_profile: "cmd".to_owned(),
            git_panel: !base.git_panel,
            split_direction: SplitDirectionV1::Down,
            search_engine: SearchEngineV1::Google,
            language: LanguageV1::Chinese,
            terminal_font_family: "Test Mono".to_owned(),
            terminal_cjk_font_family: "Test Han".to_owned(),
            terminal_font_size: base.terminal_font_size + 3,
            psreadline_invite: PsReadLineInviteV1::Declined,
            light_scheme: "Pale (custom)".to_owned(),
            dark_scheme: "Deep (custom)".to_owned(),
            background_image: "ground.png".to_owned(),
            background_fit: BackgroundFitV1::Tile,
            background_image_opacity: base.background_image_opacity.wrapping_sub(7),
            background_opacity: base.background_opacity.wrapping_sub(9),
            acrylic: !base.acrylic,
            always_on_top: !base.always_on_top,
            advanced_open: vec!["appearance".to_owned()],
            scrollback_lines: base.scrollback_lines * 2,
            focus_mode: !base.focus_mode,
            minimum_contrast: MinimumContrastV1::Ratio45,
            terminal_notifications: !base.terminal_notifications,
            powershell_integration_offer: !base.powershell_integration_offer,
            focus_card_height: base.focus_card_height + 10,
            cards_gesture_hint_offer: !base.cards_gesture_hint_offer,
            line_wrapping: !base.line_wrapping,
            key_hints: !base.key_hints,
            turn_end_notification: !base.turn_end_notification,
            copy_on_select: !base.copy_on_select,
            update_check: !base.update_check,
            quake_height: base.quake_height.wrapping_sub(5),
            quake_width: base.quake_width.wrapping_sub(5),
            quake_dismiss_on_blur: !base.quake_dismiss_on_blur,
            quake_profile_id: "pwsh".to_owned(),
            quake_startup_command: "git status".to_owned(),
            quake_top_gap: base.quake_top_gap + 12,
            quake_restore: QuakeRestoreV1::Nothing,
            launch_opens: LaunchOpensV1::TabInLastWindow,
            option_sends_alt: !base.option_sends_alt,
            multiline_paste_ask: !base.multiline_paste_ask,
            web_color_scheme: WebColorSchemeV1::Dark,
            web_pages_used: bt_persist::WebPagesUsedV1::Used,
            ..base
        }
    }

    /// RED (0.4.4 ticket 05) — **an import puts every value of the settings part
    /// in force through a change this process makes, and none of it waits for a
    /// re-read of `settings.json`.**
    ///
    /// `settings.json` is applied in process at its own door and is never re-read
    /// during a run (RULES row 33), so an import that wrote the file and hoped
    /// would leave every value it carried out of force until the next launch.
    /// Here the plan for a document departing from the current one in every key
    /// is folded back over the current document: what comes out must be the
    /// imported document itself, less this machine's two records — which means
    /// every key reached a change, and every change is one `Runtime` puts
    /// through the row's own door. No key is left for a re-read to carry.
    ///
    /// MUTATION: drop the `quake_top_gap` offer from `plan_settings` and the fold
    /// comes out one key short.
    #[test]
    fn an_import_applies_settings_in_process_and_does_not_wait_for_a_reread() {
        let current = SettingsV1::default();
        let imported = departed();
        for platform in [
            bt_platform::HostPlatform::Windows,
            bt_platform::HostPlatform::MacOs,
        ] {
            let plan = plan_settings(&current, &imported, platform);
            let mut folded = current.clone();
            for change in plan.apply.iter().chain(&plan.elsewhere) {
                change.write_into(&mut folded);
            }
            let expected = SettingsV1 {
                first_run_card: current.first_run_card,
                powershell_install_pending: current.powershell_install_pending,
                cards_gesture_hint_offer: current.cards_gesture_hint_offer,
                web_pages_used: current.web_pages_used,
                ..imported.clone()
            };
            assert_eq!(folded, expected, "{platform:?}");
            for change in &plan.elsewhere {
                let row = change.row().expect("a row this machine lacks is a row");
                assert!(
                    row.needs().is_some_and(|needed| !needed.on(platform)),
                    "{row:?} is set aside on {platform:?} only because it is not here"
                );
            }
        }
        let windows = plan_settings(&current, &imported, bt_platform::HostPlatform::Windows);
        assert!(
            windows
                .elsewhere
                .contains(&SettingChange::OptionSendsAlt(imported.option_sends_alt)),
            "a Mac's Option key is named on Windows, not put through a door that is not there"
        );
        assert!(
            windows
                .apply
                .contains(&SettingChange::Acrylic(imported.acrylic)),
            "and Windows' own backdrop is applied there"
        );
        let mac = plan_settings(&current, &imported, bt_platform::HostPlatform::MacOs);
        assert!(
            mac.elsewhere
                .contains(&SettingChange::Acrylic(imported.acrylic)),
            "the backdrop is named on a Mac"
        );
    }

    /// RED (60) — **the web-pages receipt is no row, and no import moves it, in either
    /// direction.**
    ///
    /// Whether a page has ever committed here is a fact about this profile: another machine's
    /// `Used` would hand this one a 91 MB spare it never asked for, and another machine's `Never`
    /// would take away the first page's speed from a reader who opens pages every day.
    ///
    /// MUTATION: bind `web_pages_used` in `plan_settings` and offer it as a change, and the fold
    /// carries the imported value.
    #[test]
    fn the_web_pages_receipt_is_no_row_and_no_import_moves_it() {
        use bt_persist::WebPagesUsedV1::{Never, Used};
        for (here, there) in [(Never, Used), (Used, Never)] {
            let current = SettingsV1 {
                web_pages_used: here,
                ..SettingsV1::default()
            };
            let imported = SettingsV1 {
                web_pages_used: there,
                ..SettingsV1::default()
            };
            let plan = plan_settings(&current, &imported, bt_platform::HostPlatform::Windows);
            assert!(
                plan.apply.is_empty() && plan.elsewhere.is_empty(),
                "nothing to put in force: {:?} {:?}",
                plan.apply,
                plan.elsewhere
            );
            let mut folded = current.clone();
            for change in plan.apply.iter().chain(&plan.elsewhere) {
                change.write_into(&mut folded);
            }
            assert_eq!(folded.web_pages_used, here, "this machine's receipt stands");
        }
    }

    /// RED (0.4.4 ticket 05) — **an import of an export of this very state
    /// changes nothing and names nothing.**
    ///
    /// Runs the real producer end to end: the export writer, the export reader,
    /// and each part's own plan. A key order that differed, a scheme re-written
    /// because its bytes moved, or a shortcut line refused on the way back would
    /// each make importing one's own export a change — which is the one import a
    /// reader should be able to make without consequence.
    ///
    /// MUTATION: drop the `existing.get(file) == Some(document)` check from
    /// `plan_schemes` and the scheme is written again.
    #[test]
    fn an_import_of_an_export_is_the_identity() {
        let settings = SettingsV1 {
            git_panel: false,
            dark_scheme: "Deep (custom)".to_owned(),
            multiline_paste_ask: false,
            ..SettingsV1::default()
        };
        let profiles = ProfilesV1::default();
        let keybindings = KeybindingsV1 {
            schema_version: KEYBINDINGS_SCHEMA_VERSION,
            bindings: vec![BindingOverrideV1 {
                action: "new-tab".to_owned(),
                chord: None,
            }],
        };
        let mut schemes = BTreeMap::new();
        schemes.insert(
            "Deep (custom).json".to_owned(),
            scheme_document("Deep (custom)"),
        );
        let bytes = bt_persist::serialize_export(ExportParts {
            exported_by: "Folio test",
            settings: &settings,
            profiles: &profiles,
            keybindings: &keybindings,
            schemes: &schemes,
        })
        .unwrap();
        let parts = bt_persist::parse_export(&bytes).unwrap();

        let imported = parts.settings.unwrap().unwrap();
        assert_eq!(
            plan_settings(&settings, &imported, bt_platform::host_platform()),
            SettingsPlan::default()
        );
        assert_eq!(parts.profiles.unwrap().unwrap(), profiles);
        let (table, faults) = import_shortcuts(&parts.keybindings.unwrap().unwrap());
        assert!(faults.is_empty(), "{faults:?}");
        let back: Vec<BindingOverrideV1> = table
            .overrides()
            .into_iter()
            .map(|entry| BindingOverrideV1 {
                action: entry.id,
                chord: entry.chord,
            })
            .collect();
        assert_eq!(back, keybindings.bindings);
        assert_eq!(
            plan_schemes(&schemes, &parts.schemes.unwrap().unwrap()),
            SchemePlan::default()
        );
    }

    /// RED (0.4.4 ticket 05) — **a scheme that will not parse is named by its
    /// file, and the others are still imported.**
    ///
    /// The folder's own rule for a bad file (`Text::SchemeFileSkipped`), met in
    /// a bundle: one broken scheme is not a reason to lose the good one beside
    /// it, and "a scheme was skipped" without its file name tells the reader to
    /// open all of them.
    ///
    /// MUTATION: return the plan at the first fault and the good scheme is lost.
    #[test]
    fn a_scheme_that_fails_to_parse_is_reported_by_name_and_the_rest_are_imported() {
        let mut imported = BTreeMap::new();
        imported.insert(
            "Broken.json".to_owned(),
            serde_json::json!({ "name": "Broken" }),
        );
        imported.insert(
            "Deep (custom).json".to_owned(),
            scheme_document("Deep (custom)"),
        );
        imported.insert("../escape.json".to_owned(), scheme_document("Escape"));
        let plan = plan_schemes(&BTreeMap::new(), &imported);
        let written: Vec<&str> = plan.writes.iter().map(|(file, _)| file.as_str()).collect();
        assert_eq!(written, ["Deep (custom).json"]);
        let named: Vec<&str> = plan
            .faults
            .iter()
            .map(|fault| fault.what.as_str())
            .collect();
        assert_eq!(named, ["schemes/../escape.json", "schemes/Broken.json"]);
        assert!(
            plan.faults[1].reason.contains("background"),
            "the reason names the key: {}",
            plan.faults[1].reason
        );
        let (_, text) = &plan.writes[0];
        assert_eq!(
            bt_persist::parse_scheme(text).unwrap().name,
            "Deep (custom)",
            "what is written is the scheme that was judged"
        );
    }

    /// RED (0.4.4 ticket 05) — **one bad shortcut line is named by its id, and
    /// every other line still applies.**
    ///
    /// `apply_overrides` at launch, met at the import's door: the refused line
    /// leaves its row at the default and says which line it was.
    ///
    /// MUTATION: discard the table when any fault comes back and the good line
    /// does not land.
    #[test]
    fn one_bad_keybinding_line_is_reported_by_name_and_the_rest_apply() {
        let file = KeybindingsV1 {
            schema_version: KEYBINDINGS_SCHEMA_VERSION,
            bindings: vec![
                BindingOverrideV1 {
                    action: "new-tab".to_owned(),
                    chord: None,
                },
                BindingOverrideV1 {
                    action: "no-such-verb".to_owned(),
                    chord: Some("Ctrl+Shift+U".to_owned()),
                },
            ],
        };
        let (table, faults) = import_shortcuts(&file);
        assert_eq!(faults.len(), 1, "{faults:?}");
        assert_eq!(faults[0].what, "keybindings.json: no-such-verb");
        assert!(table.is_overridden("new-tab"), "the good line landed");
    }
}
