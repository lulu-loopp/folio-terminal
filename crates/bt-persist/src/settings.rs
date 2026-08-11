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
/// `default_profile`. §2's "只收录已经在 DESIGN/M2 文档里落定的用户可见项" is
/// satisfied the way §1.3 intends it to be: each field arrives in the same change
/// that gives it a reader, not ahead of one.
pub const SETTINGS_SCHEMA_VERSION: u32 = 4;

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

/// `settings.json` v4 — docs/M2-persistence-schema-v1.md §2:
/// ```json
/// {
///   "schema_version": 4,
///   "theme_mode": "System" | "Light" | "Dark",
///   "display_formulas": true | false,
///   "inline_formulas": true | false,
///   "default_profile": "pwsh" | "wsl" | "gitbash" | "cmd" | ""
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
}

impl Default for SettingsV1 {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            theme_mode: ThemeModeV1::default(),
            display_formulas: true,
            inline_formulas: true,
            default_profile: DEFAULT_PROFILE_UNSET.to_owned(),
        }
    }
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
