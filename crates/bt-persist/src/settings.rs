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
/// v2 adds `display_formulas`, v3 adds `inline_formulas`. §2's "只收录已经在
/// DESIGN/M2 文档里落定的用户可见项" is satisfied the way §1.3 intends it to be:
/// each field arrives in the same change that gives it a reader, not ahead of one.
pub const SETTINGS_SCHEMA_VERSION: u32 = 3;

/// `settings.json` v3 — docs/M2-persistence-schema-v1.md §2:
/// ```json
/// {
///   "schema_version": 3,
///   "theme_mode": "System" | "Light" | "Dark",
///   "display_formulas": true | false,
///   "inline_formulas": true | false
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
}

impl Default for SettingsV1 {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            theme_mode: ThemeModeV1::default(),
            display_formulas: true,
            inline_formulas: true,
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
