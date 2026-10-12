//! **`settings.json`, merged three ways** (`docs/plans/design/settings-reread-2026-10-11.md`
//! §3.3 and §3.4 as revised by (b)3, (b)6, (b)8, (c)1 and (d)1; ticket
//! T-STORE-MERGE-SETTINGS).
//!
//! A re-read whose bytes are neither the revision's nor the write on its way,
//! a write that answered `Moved`, and a write a hand edit crossed (`Displaced`)
//! each leave the file's bytes with the store for the merge
//! (`document_request::PendingMerge`). The merge takes three documents:
//!
//! - the **base** `B`: the revision the file was compared with — for a
//!   displaced file, the revision before the exchange;
//! - **ours** `O`: the document Folio wants on disk (`desired`) — for a
//!   displaced file, the document that was written;
//! - **theirs** `T`: the file's document; no file is the default document.
//!
//! **The unit is the field**, and each is merged by §3.3's table ([`unit`]): the
//! same on both sides is `B`; changed here only is `O`, which stays owed;
//! changed in the file only is `T`; changed on both to one value is `T`; changed
//! on both to two values is a **conflict, and the file wins** (owner ruling Q1,
//! 2026-10-11), named by the row it is chosen on. The document is taken apart
//! with no `..`, as `settings_bundle::plan_settings` takes it apart, so a field
//! added to `SettingsV1` does not compile here until somebody has said how it
//! merges.
//!
//! **The three machine receipts** (`first_run_card`, `cards_gesture_hint_offer`,
//! `web_pages_used`) merge like any other field: a hand edit on this machine is
//! a statement about this machine. They have no door, so storing them is
//! putting them in force ([`take_receipts`]).
//!
//! **The doors are derived from the merged document** ((b)8): the merge comes
//! first, and then [`doors`] turns the difference between what is in force and
//! the merged document into the changes a press on each row makes. Folio's font
//! family and the file's font size are one door, called once, with both.

use bt_persist::{SETTINGS_SCHEMA_VERSION, SettingsV1};

use crate::document_request::ReplacedInFile;
use crate::settings_bundle::{SettingChange, SettingsPlan, plan_settings};

/// **What one unit came to** (§3.3's table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unit {
    /// Neither side changed it.
    Base,
    /// Only Folio changed it: Folio's value, which stays owed.
    Ours,
    /// The file changed it, alone or to the value Folio chose too.
    Theirs,
    /// Both changed it, to two values: the file's value, named.
    Conflict,
}

/// **§3.3's table for one unit**: the value the merge keeps, and which row of
/// the table chose it.
pub(crate) fn unit<V: PartialEq>(base: V, ours: V, theirs: V) -> (V, Unit) {
    match (ours == base, theirs == base) {
        (true, true) => (base, Unit::Base),
        (false, true) => (ours, Unit::Ours),
        (true, false) => (theirs, Unit::Theirs),
        (false, false) if ours == theirs => (theirs, Unit::Theirs),
        (false, false) => (theirs, Unit::Conflict),
    }
}

/// **What one merge says about itself**: how many units each side gave, and
/// the names of the units the file gave.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Account {
    /// How many units took the file's value without a conflict.
    pub(crate) took: usize,
    /// Those units, named — a field that shares a row with another (the
    /// terminal font's three) is named once.
    pub(crate) taken: Vec<String>,
    /// How many units kept Folio's value against an unchanged file.
    pub(crate) kept: usize,
    /// How many units both sides changed to two values; the file won each.
    pub(crate) conflicted: usize,
    /// Those units, named.
    pub(crate) conflicts: Vec<String>,
}

/// **One merge of `settings.json`.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Merged {
    /// The merged document `M`: what Folio now wants on disk.
    pub(crate) document: SettingsV1,
    pub(crate) account: Account,
}

/// The merge's running account, field by field.
struct Merging {
    base: SettingsV1,
    ours: SettingsV1,
    platform: bt_platform::HostPlatform,
    account: Account,
}

impl Merging {
    /// **One field through [`unit`]**: `key` is the field's name in the file,
    /// `at` reaches the field in a document, `theirs` is the file's value.
    fn field<V, F>(&mut self, key: &'static str, at: F, theirs: V) -> V
    where
        V: Clone + PartialEq,
        F: Fn(&mut SettingsV1) -> &mut V,
    {
        let base = at(&mut self.base).clone();
        let ours = at(&mut self.ours).clone();
        let (value, row) = unit(base, ours, theirs);
        match row {
            Unit::Base => {}
            Unit::Ours => self.account.kept += 1,
            Unit::Theirs => {
                self.account.took += 1;
                let name = self.name(key, &at, &value);
                push_once(&mut self.account.taken, name);
            }
            Unit::Conflict => {
                self.account.conflicted += 1;
                let name = self.name(key, &at, &value);
                push_once(&mut self.account.conflicts, name);
            }
        }
        value
    }

    /// **A field named the way the reader knows it** (§3.6): the title of the
    /// Settings row its value is chosen on, which is the change it would make
    /// to the base — or, for a value no row shows, its name in the file.
    fn name<V, F>(&self, key: &'static str, at: &F, value: &V) -> String
    where
        V: Clone,
        F: Fn(&mut SettingsV1) -> &mut V,
    {
        let mut probe = self.base.clone();
        *at(&mut probe) = value.clone();
        let plan = plan_settings(&self.base, &probe, self.platform);
        plan.apply
            .iter()
            .chain(&plan.elsewhere)
            .find_map(|change| change.row())
            .map_or_else(|| key.to_owned(), |row| row.title().to_owned())
    }
}

fn push_once(names: &mut Vec<String>, name: String) {
    if !names.contains(&name) {
        names.push(name);
    }
}

/// **Merge `ours` and `theirs` against `base`, field by field** (§3.3, §3.4).
pub(crate) fn merge(
    base: &SettingsV1,
    ours: &SettingsV1,
    theirs: &SettingsV1,
    platform: bt_platform::HostPlatform,
) -> Merged {
    let mut m = Merging {
        base: base.clone(),
        ours: ours.clone(),
        platform,
        account: Account::default(),
    };
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
        shell_integration,
        focus_card_height,
        cards_gesture_hint_offer,
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
        first_run_card,
        web_pages_used,
        launch_opens,
        option_sends_alt,
        multiline_paste_ask,
        web_color_scheme,
    } = theirs.clone();
    let document = SettingsV1 {
        // The envelope is this build's: a newer file is refused before any
        // merge, and an older one was migrated forward when it was read.
        schema_version: SETTINGS_SCHEMA_VERSION,
        theme_mode: m.field("theme_mode", |s| &mut s.theme_mode, theme_mode),
        display_formulas: m.field(
            "display_formulas",
            |s| &mut s.display_formulas,
            display_formulas,
        ),
        inline_formulas: m.field(
            "inline_formulas",
            |s| &mut s.inline_formulas,
            inline_formulas,
        ),
        repair_row_breaks: m.field(
            "repair_row_breaks",
            |s| &mut s.repair_row_breaks,
            repair_row_breaks,
        ),
        tables: m.field("tables", |s| &mut s.tables, tables),
        block_max_height: m.field(
            "block_max_height",
            |s| &mut s.block_max_height,
            block_max_height,
        ),
        default_profile: m.field(
            "default_profile",
            |s| &mut s.default_profile,
            default_profile,
        ),
        git_panel: m.field("git_panel", |s| &mut s.git_panel, git_panel),
        split_direction: m.field(
            "split_direction",
            |s| &mut s.split_direction,
            split_direction,
        ),
        search_engine: m.field("search_engine", |s| &mut s.search_engine, search_engine),
        language: m.field("language", |s| &mut s.language, language),
        terminal_font_family: m.field(
            "terminal_font_family",
            |s| &mut s.terminal_font_family,
            terminal_font_family,
        ),
        terminal_cjk_font_family: m.field(
            "terminal_cjk_font_family",
            |s| &mut s.terminal_cjk_font_family,
            terminal_cjk_font_family,
        ),
        terminal_font_size: m.field(
            "terminal_font_size",
            |s| &mut s.terminal_font_size,
            terminal_font_size,
        ),
        psreadline_invite: m.field(
            "psreadline_invite",
            |s| &mut s.psreadline_invite,
            psreadline_invite,
        ),
        light_scheme: m.field("light_scheme", |s| &mut s.light_scheme, light_scheme),
        dark_scheme: m.field("dark_scheme", |s| &mut s.dark_scheme, dark_scheme),
        background_image: m.field(
            "background_image",
            |s| &mut s.background_image,
            background_image,
        ),
        background_fit: m.field("background_fit", |s| &mut s.background_fit, background_fit),
        background_image_opacity: m.field(
            "background_image_opacity",
            |s| &mut s.background_image_opacity,
            background_image_opacity,
        ),
        background_opacity: m.field(
            "background_opacity",
            |s| &mut s.background_opacity,
            background_opacity,
        ),
        acrylic: m.field("acrylic", |s| &mut s.acrylic, acrylic),
        always_on_top: m.field("always_on_top", |s| &mut s.always_on_top, always_on_top),
        advanced_open: m.field("advanced_open", |s| &mut s.advanced_open, advanced_open),
        scrollback_lines: m.field(
            "scrollback_lines",
            |s| &mut s.scrollback_lines,
            scrollback_lines,
        ),
        focus_mode: m.field("focus_mode", |s| &mut s.focus_mode, focus_mode),
        minimum_contrast: m.field(
            "minimum_contrast",
            |s| &mut s.minimum_contrast,
            minimum_contrast,
        ),
        terminal_notifications: m.field(
            "terminal_notifications",
            |s| &mut s.terminal_notifications,
            terminal_notifications,
        ),
        shell_integration: m.field(
            "shell_integration",
            |s| &mut s.shell_integration,
            shell_integration,
        ),
        focus_card_height: m.field(
            "focus_card_height",
            |s| &mut s.focus_card_height,
            focus_card_height,
        ),
        cards_gesture_hint_offer: m.field(
            "cards_gesture_hint_offer",
            |s| &mut s.cards_gesture_hint_offer,
            cards_gesture_hint_offer,
        ),
        line_wrapping: m.field("line_wrapping", |s| &mut s.line_wrapping, line_wrapping),
        key_hints: m.field("key_hints", |s| &mut s.key_hints, key_hints),
        turn_end_notification: m.field(
            "turn_end_notification",
            |s| &mut s.turn_end_notification,
            turn_end_notification,
        ),
        copy_on_select: m.field("copy_on_select", |s| &mut s.copy_on_select, copy_on_select),
        update_check: m.field("update_check", |s| &mut s.update_check, update_check),
        quake_height: m.field("quake_height", |s| &mut s.quake_height, quake_height),
        quake_width: m.field("quake_width", |s| &mut s.quake_width, quake_width),
        quake_dismiss_on_blur: m.field(
            "quake_dismiss_on_blur",
            |s| &mut s.quake_dismiss_on_blur,
            quake_dismiss_on_blur,
        ),
        quake_profile_id: m.field(
            "quake_profile_id",
            |s| &mut s.quake_profile_id,
            quake_profile_id,
        ),
        quake_startup_command: m.field(
            "quake_startup_command",
            |s| &mut s.quake_startup_command,
            quake_startup_command,
        ),
        quake_top_gap: m.field("quake_top_gap", |s| &mut s.quake_top_gap, quake_top_gap),
        quake_restore: m.field("quake_restore", |s| &mut s.quake_restore, quake_restore),
        first_run_card: m.field("first_run_card", |s| &mut s.first_run_card, first_run_card),
        web_pages_used: m.field("web_pages_used", |s| &mut s.web_pages_used, web_pages_used),
        launch_opens: m.field("launch_opens", |s| &mut s.launch_opens, launch_opens),
        option_sends_alt: m.field(
            "option_sends_alt",
            |s| &mut s.option_sends_alt,
            option_sends_alt,
        ),
        multiline_paste_ask: m.field(
            "multiline_paste_ask",
            |s| &mut s.multiline_paste_ask,
            multiline_paste_ask,
        ),
        web_color_scheme: m.field(
            "web_color_scheme",
            |s| &mut s.web_color_scheme,
            web_color_scheme,
        ),
    };
    Merged {
        document,
        account: m.account,
    }
}

/// **One storage pass's merge of `settings.json`** — what
/// `SettingsStore::begin_merge` hands the window: the doors to walk, and what
/// the pass says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SettingsPass {
    /// The changes that put the merged document in force, derived from what
    /// is in force ([`doors`]).
    pub(crate) doors: SettingsPlan,
    /// Every merge of the pass, summed.
    pub(crate) account: Account,
    /// The units a door did not put in force, named by row, with why
    /// ([`run_pass`]).
    pub(crate) refused: Vec<(String, String)>,
    /// The units a door put in force and then answered an error about (a
    /// step after the store failed), named by row, with the error.
    pub(crate) noted: Vec<(String, String)>,
}

impl SettingsPass {
    /// A pass over `doors`, with `account` to say; no door walked yet.
    pub(crate) fn new(doors: SettingsPlan, account: Account) -> Self {
        Self {
            doors,
            account,
            refused: Vec::new(),
            noted: Vec::new(),
        }
    }

    /// **The pass's one line for `diagnostics.log`** (§3.6): the counts, each
    /// unit a door did not put in force and why, and each error a door
    /// answered after putting its unit in force.
    pub(crate) fn diagnostics(&self) -> String {
        let mut line = format!(
            "BT_PERSIST {} merged: took {} from the file, kept {} from Folio, conflicts {}, \
             doors refused: {}",
            crate::persist::SETTINGS_FILE_NAME,
            self.account.took,
            self.account.kept,
            self.account.conflicted,
            self.refused.len()
        );
        for (name, why) in &self.refused {
            line.push_str(&format!("; {name} not applied: {why}"));
        }
        for (name, error) in &self.noted {
            line.push_str(&format!("; {name} applied, then: {error}"));
        }
        line
    }

    /// **What the pass replaced and could not apply, for its card** (§3.6):
    /// nothing for a clean hand edit; otherwise the file, each value it won
    /// over Folio's, and each value whose door refused it, each named once.
    pub(crate) fn replaced(&self) -> Option<ReplacedInFile> {
        if self.account.conflicts.is_empty() && self.refused.is_empty() {
            return None;
        }
        let mut not_applied = Vec::new();
        for (name, _) in &self.refused {
            push_once(&mut not_applied, name.clone());
        }
        Some(ReplacedInFile {
            file: crate::persist::SETTINGS_FILE_NAME,
            values: self.account.conflicts.clone(),
            not_applied,
        })
    }
}

/// **What a settings merge needs from the window it runs in** — the product
/// seam `Runtime` implements (`runtime/configuration.rs`), so the pass's order
/// and its judgment of each door are [`run_pass`]'s and nobody else's.
pub(crate) trait MergeWindow {
    /// `SettingsStore::begin_merge`: the merges folded, the writes held, the
    /// doors derived.
    fn merge_begin(&mut self) -> Option<SettingsPass>;
    /// What is in force: the settings store's projection.
    fn merge_in_force(&self) -> &SettingsV1;
    /// One change through its row's own door (`apply_imported_setting`).
    fn merge_door(&mut self, change: &SettingChange) -> anyhow::Result<()>;
    /// A change no row on this platform shows, stored as it came.
    fn merge_store_as_it_came(&mut self, change: &SettingChange);
    /// `SettingsStore::end_merge`: the writes released, the merged document
    /// sent conditionally.
    fn merge_end(&mut self);
    /// The pass's one line, written to `diagnostics.log`.
    fn merge_say(&mut self, line: &str);
    /// The open dialog re-derived and the chrome presented.
    fn merge_present(&mut self) -> anyhow::Result<()>;
}

/// **What one pass came to**: its card's data, and whether the presentation
/// after it went through.
pub(crate) struct PassOutcome {
    pub(crate) replaced: Option<ReplacedInFile>,
    pub(crate) presented: anyhow::Result<()>,
}

/// **Whether `change` is in force in `in_force`**: writing it in changes
/// nothing. The import's own post-probe (`import_settings_part`).
fn holds(in_force: &SettingsV1, change: &SettingChange) -> bool {
    let mut probe = in_force.clone();
    change.write_into(&mut probe);
    &probe == in_force
}

/// **One storage pass's merge of `settings.json`, in §3.3's order**
/// (T-STORE-MERGE-SETTINGS, review rounds 2 and 3).
///
/// The store merges and folds and holds the writes; then **every door is
/// walked**, and whether a unit was applied is read from what is in force,
/// before and after its door — never from the door's answer: a door that
/// answers `Ok` and leaves the unit as it was refused it (`apply_acrylic` on a
/// machine with no backdrop); a door that put the unit in force and then
/// failed (`apply_block_max_height`'s frame) applied it, and its error is said
/// beside it. A refused unit stays in `desired` as the file said it, and what
/// is in force keeps its own value ((b)6). Then a value no row on this
/// platform shows is stored as it came, the writes are released, the pass's
/// line is written and its card's data is made — both before the
/// presentation, which cannot take them away.
pub(crate) fn run_pass(window: &mut impl MergeWindow) -> PassOutcome {
    let Some(mut pass) = window.merge_begin() else {
        return PassOutcome {
            replaced: None,
            presented: Ok(()),
        };
    };
    for change in pass.doors.apply.clone() {
        let before = holds(window.merge_in_force(), &change);
        let answer = window.merge_door(&change);
        let after = holds(window.merge_in_force(), &change);
        let name = change.row().map_or_else(
            || crate::persist::SETTINGS_FILE_NAME.to_owned(),
            |row| row.title().to_owned(),
        );
        match (before || after, answer) {
            (true, Ok(())) => {}
            (true, Err(error)) => pass.noted.push((name, error.to_string())),
            (false, Ok(())) => {
                pass.refused
                    .push((name, "this machine did not take the value".to_owned()));
            }
            (false, Err(error)) => pass.refused.push((name, error.to_string())),
        }
    }
    for change in &pass.doors.elsewhere {
        window.merge_store_as_it_came(change);
    }
    window.merge_end();
    window.merge_say(&pass.diagnostics());
    let replaced = pass.replaced();
    let presented = if pass.doors.apply.is_empty() && pass.doors.elsewhere.is_empty() {
        Ok(())
    } else {
        window.merge_present()
    };
    PassOutcome {
        replaced,
        presented,
    }
}

impl Account {
    /// Add one merge's account to the pass's.
    pub(crate) fn add(&mut self, other: Account) {
        self.took += other.took;
        self.kept += other.kept;
        self.conflicted += other.conflicted;
        for name in other.taken {
            push_once(&mut self.taken, name);
        }
        for name in other.conflicts {
            push_once(&mut self.conflicts, name);
        }
    }
}

/// **The doors a merge walks** ((b)8): every value of `merged` that differs
/// from what is in force, as the change a press on its row makes. Derived from
/// the merged document — never from the file's — so a door that puts several
/// fields in force at once (the terminal font) is called with Folio's half and
/// the file's half together.
pub(crate) fn doors(
    in_force: &SettingsV1,
    merged: &SettingsV1,
    platform: bt_platform::HostPlatform,
) -> SettingsPlan {
    plan_settings(in_force, merged, platform)
}

/// **The three machine receipts, put in force by storing them** (§3.4): they
/// have no door, and the doors' plan leaves them out.
pub(crate) fn take_receipts(settings: &mut SettingsV1, merged: &SettingsV1) {
    settings.first_run_card = merged.first_run_card;
    // The merge's writer of the cards' offer: a hand edit of this machine's
    // file (`the_cards_offer_is_spent_in_one_place_and_given_back_in_one`).
    settings.cards_gesture_hint_offer = merged.cards_gesture_hint_offer;
    settings.web_pages_used = merged.web_pages_used;
}

#[cfg(test)]
mod tests {
    use bt_persist::{ThemeModeV1, WebPagesUsedV1};

    use super::*;
    use crate::document_request::StoredDocument;
    use crate::persist::{SETTINGS_FILE_NAME, SettingsStore, StorageLane};

    const PLATFORM: bt_platform::HostPlatform = bt_platform::HostPlatform::Windows;

    fn scratch(case: &str) -> std::path::PathBuf {
        let root = bt_testpath::temp_path(&format!("bt-app-settings-merge-{case}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private storage folder for this test");
        root
    }

    fn bytes(document: &SettingsV1) -> std::sync::Arc<[u8]> {
        document
            .to_bytes()
            .expect("a settings document serializes")
            .into()
    }

    /// RED (§3.3) — **the merge's table, unit by unit**, over a synthetic set
    /// of fields in mixed script: the same on both sides, changed here only,
    /// changed in the file only, changed on both to one value, and changed on
    /// both to two values — a conflict the file wins.
    ///
    /// MUTATION: in [`unit`], let the conflict take `ours` — the last row's value
    /// is Folio's and the assertion names it.
    #[test]
    fn the_table_keeps_each_side_and_the_file_wins_a_conflict() {
        let rows = [
            // (base, ours, theirs) → (value, row)
            (("宋体 A", "宋体 A", "宋体 A"), ("宋体 A", Unit::Base)),
            (("宋体 A", "黑体 B", "宋体 A"), ("黑体 B", Unit::Ours)),
            (("宋体 A", "宋体 A", "楷体 C"), ("楷体 C", Unit::Theirs)),
            (("宋体 A", "楷体 C", "楷体 C"), ("楷体 C", Unit::Theirs)),
            (("宋体 A", "黑体 B", "楷体 C"), ("楷体 C", Unit::Conflict)),
        ];
        for ((base, ours, theirs), expected) in rows {
            assert_eq!(
                unit(base, ours, theirs),
                expected,
                "base {base}, ours {ours}, theirs {theirs}"
            );
        }
    }

    /// The table over whole documents: one field per row, counted and named.
    #[test]
    fn a_document_merges_field_by_field_and_names_what_it_took() {
        let base = SettingsV1::default();
        let ours = SettingsV1 {
            terminal_font_family: "我们的 Ours Mono".to_owned(),
            theme_mode: ThemeModeV1::Dark,
            ..base.clone()
        };
        let theirs = SettingsV1 {
            terminal_font_size: base.terminal_font_size + 3,
            theme_mode: ThemeModeV1::Light,
            web_pages_used: WebPagesUsedV1::Used,
            ..base.clone()
        };
        let merged = merge(&base, &ours, &theirs, PLATFORM);
        assert_eq!(
            merged.document,
            SettingsV1 {
                terminal_font_family: "我们的 Ours Mono".to_owned(),
                terminal_font_size: base.terminal_font_size + 3,
                theme_mode: ThemeModeV1::Light,
                web_pages_used: WebPagesUsedV1::Used,
                ..base.clone()
            }
        );
        assert_eq!(merged.account.kept, 1, "the family is Folio's");
        assert_eq!(merged.account.took, 2, "the size and the receipt");
        assert_eq!(merged.account.conflicted, 1);
        assert_eq!(
            merged.account.conflicts,
            vec![crate::settings::SettingsRow::Theme.title().to_owned()]
        );
        assert_eq!(
            merged.account.taken,
            vec![
                crate::settings::SettingsRow::TerminalFont
                    .title()
                    .to_owned(),
                "web_pages_used".to_owned(),
            ],
            "a row by its title, a receipt by its name in the file"
        );
    }

    /// A settings store over a scratch file, on a lane nobody serves: what it
    /// would write stays queued, and a test hands it the reads.
    fn store(
        case: &str,
    ) -> (
        std::path::PathBuf,
        SettingsStore,
        crate::persist::UnservedJobs,
    ) {
        let root = scratch(case);
        let (lane, jobs) = StorageLane::unserved();
        let store = SettingsStore::at_on(root.join(SETTINGS_FILE_NAME), lane);
        (root, store, jobs)
    }

    /// RED ((b)14-9, (b)8) — **Folio's font family and the file's font size both
    /// survive**, and the one terminal-font door is called with both.
    ///
    /// Folio changed the family, which is not on disk; the file changed only
    /// the size. The merged document holds Folio's family and the file's size,
    /// and the door the pass walks is derived from it.
    ///
    /// MUTATION: in `SettingsStore::begin_merge`, derive the doors from the
    /// file's document instead of the merged one — the door carries the old
    /// family and the assertion names it.
    #[test]
    fn folios_font_family_and_the_files_font_size_both_survive() {
        let (root, mut store, _jobs) = store("font");
        let base = SettingsV1::default();
        let ours = SettingsV1 {
            terminal_font_family: "等距更纱黑体 Sarasa Mono".to_owned(),
            ..base.clone()
        };
        assert!(store.store(ours.clone()));
        let theirs = SettingsV1 {
            terminal_font_size: 19,
            ..base.clone()
        };
        let _ = store
            .document()
            .hear_read(bt_persist::StableRead::Bytes(bytes(&theirs)));

        let pass = store
            .begin_merge(PLATFORM)
            .expect("the file's change is merged");
        store.end_merge();
        assert_eq!(
            pass.doors.apply,
            vec![SettingChange::TerminalFont {
                family: "等距更纱黑体 Sarasa Mono".to_owned(),
                cjk_family: base.terminal_cjk_font_family.clone(),
                size: 19,
            }],
            "one door, with Folio's family and the file's size"
        );
        assert_eq!(
            store.document().desired(),
            &SettingsV1 {
                terminal_font_family: "等距更纱黑体 Sarasa Mono".to_owned(),
                terminal_font_size: 19,
                ..base
            }
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **A window for the pass, with two doors shaped as the real ones are.** A
    /// real `Runtime` needs a window, which a test may not open, so the seam is
    /// driven here with doors that do what the two real doors do:
    /// - `Acrylic`: `apply_acrylic` on a machine whose backdrop is unavailable
    ///   answers `Ok(false)` before it stores anything, which
    ///   `apply_imported_setting` answers as `Ok(())`;
    /// - `BlockMaxHeight`: `apply_block_max_height` stores the value, then
    ///   publishes a frame, which fails here.
    ///
    /// Every other change is stored, as a door that took it.
    struct Window {
        store: SettingsStore,
        _jobs: crate::persist::UnservedJobs,
        presentation_fails: bool,
        /// What the pass said and presented, in the order it happened.
        log: Vec<String>,
    }

    impl MergeWindow for Window {
        fn merge_begin(&mut self) -> Option<SettingsPass> {
            self.store.begin_merge(PLATFORM)
        }

        fn merge_in_force(&self) -> &SettingsV1 {
            self.store.loaded()
        }

        fn merge_door(&mut self, change: &SettingChange) -> anyhow::Result<()> {
            match change {
                SettingChange::Acrylic(_) => Ok(()),
                SettingChange::BlockMaxHeight(height) => {
                    let mut settings = self.store.loaded().clone();
                    settings.block_max_height = *height;
                    self.store.store(settings);
                    Err(anyhow::anyhow!("画面没有发布 the frame was not published"))
                }
                other => {
                    self.merge_store_as_it_came(other);
                    Ok(())
                }
            }
        }

        fn merge_store_as_it_came(&mut self, change: &SettingChange) {
            let mut settings = self.store.loaded().clone();
            change.write_into(&mut settings);
            self.store.store(settings);
        }

        fn merge_end(&mut self) {
            self.store.end_merge();
        }

        fn merge_say(&mut self, line: &str) {
            self.log.push(line.to_owned());
        }

        fn merge_present(&mut self) -> anyhow::Result<()> {
            self.log.push("presented".to_owned());
            if self.presentation_fails {
                anyhow::bail!("呈现失败 the chrome could not be presented");
            }
            Ok(())
        }
    }

    /// A window over a scratch store whose file has changed to `theirs`.
    fn window(case: &str, theirs: &SettingsV1) -> (std::path::PathBuf, Window) {
        let (root, mut store, jobs) = store(case);
        let _ = store
            .document()
            .hear_read(bt_persist::StableRead::Bytes(bytes(theirs)));
        (
            root,
            Window {
                store,
                _jobs: jobs,
                presentation_fails: false,
                log: Vec::new(),
            },
        )
    }

    /// RED (round 3 (a)) — **a door that answers `Ok` and leaves the unit as it
    /// was refused it**, and the doors after it are walked. The file turns the
    /// backdrop on, on a machine that has none, and turns always-on-top on.
    ///
    /// MUTATION: in `run_pass`, judge a door by its answer alone (count only
    /// `Err` as refused) — the line says `doors refused: 0`.
    #[test]
    fn a_door_the_machine_refuses_is_named_refused_and_the_rest_are_walked() {
        let theirs = SettingsV1 {
            acrylic: true,
            always_on_top: true,
            ..SettingsV1::default()
        };
        let (root, mut window) = window("acrylic-refused", &theirs);
        let outcome = run_pass(&mut window);
        let acrylic = crate::settings::SettingsRow::Acrylic.title();
        assert_eq!(
            window.log.first().map(String::as_str),
            Some(
                format!(
                    "BT_PERSIST settings.json merged: took 2 from the file, kept 0 from Folio, \
                     conflicts 0, doors refused: 1; {acrylic} not applied: this machine did not \
                     take the value"
                )
                .as_str()
            )
        );
        let replaced = outcome.replaced.expect("the refusal is the card's");
        assert_eq!(replaced.not_applied, vec![acrylic.to_owned()]);
        assert!(
            window.store.loaded().always_on_top,
            "the next door is walked"
        );
        assert!(!window.store.loaded().acrylic, "in force: its own");
        assert!(
            window.store.document().desired().acrylic,
            "desired: the file's"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (round 3 (b)) — **a door that put its unit in force and then failed
    /// applied it**: the value is in force, and the line says the error beside
    /// it, not as a refusal; the card has nothing to say.
    ///
    /// MUTATION: in `run_pass`, trust the answer's shape (an `Err` is a refusal
    /// whatever is in force) — the line counts one refusal.
    #[test]
    fn a_door_that_stores_then_fails_applied_its_unit_and_says_the_error() {
        let theirs = SettingsV1 {
            block_max_height: 480,
            ..SettingsV1::default()
        };
        let (root, mut window) = window("stored-then-failed", &theirs);
        let outcome = run_pass(&mut window);
        let row = crate::settings::SettingsRow::BlockMaxHeight.title();
        assert_eq!(
            window.log.first().map(String::as_str),
            Some(
                format!(
                    "BT_PERSIST settings.json merged: took 1 from the file, kept 0 from Folio, \
                     conflicts 0, doors refused: 0; {row} applied, then: 画面没有发布 the frame \
                     was not published"
                )
                .as_str()
            )
        );
        assert_eq!(outcome.replaced, None, "nothing for the card");
        assert_eq!(window.store.loaded().block_max_height, 480, "in force");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (round 3 (c), round 4) — **a presentation that fails after the walk
    /// keeps the card's data, and the line is already in the log**: the line is
    /// written and the data made before the chrome is presented, and its
    /// failure is answered beside them.
    ///
    /// MUTATIONS: in `run_pass`, present first and return at its failure — the
    /// card's data is gone; write the line after the presentation — the log's
    /// first entry is the presentation.
    #[test]
    fn a_presentation_failure_after_the_walk_keeps_the_card_data() {
        let theirs = SettingsV1 {
            acrylic: true,
            ..SettingsV1::default()
        };
        let (root, mut window) = window("presentation-failed", &theirs);
        window.presentation_fails = true;
        let outcome = run_pass(&mut window);
        assert!(outcome.presented.is_err(), "the presentation failed");
        let replaced = outcome.replaced.expect("and the card's data is kept");
        assert_eq!(
            replaced.not_applied,
            vec![crate::settings::SettingsRow::Acrylic.title().to_owned()]
        );
        assert_eq!(window.log.len(), 2, "one line, one presentation");
        assert!(
            window.log[0].starts_with("BT_PERSIST settings.json merged:"),
            "the line was written before the presentation: {:?}",
            window.log
        );
        assert_eq!(window.log[1], "presented");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (§3.6) — **a clean hand edit says nothing, and a conflict is named
    /// once per pass**: the card's data is no entry for a pass the file won no
    /// conflict in, and one entry, naming each row once, for a pass it did —
    /// however many of the file's changes that pass merged.
    ///
    /// MUTATION: in `SettingsPass::replaced`, answer an entry whatever the
    /// conflicts are — the clean pass names `settings.json` with no values.
    #[test]
    fn a_clean_hand_edit_says_nothing_and_a_conflict_is_named_once_per_pass() {
        let (root, mut store, _jobs) = store("notice");
        let base = SettingsV1::default();

        // A clean hand edit: the file changed the theme, nothing here did.
        let clean = SettingsV1 {
            theme_mode: ThemeModeV1::Light,
            ..base.clone()
        };
        let _ = store
            .document()
            .hear_read(bt_persist::StableRead::Bytes(bytes(&clean)));
        let pass = store.begin_merge(PLATFORM).expect("a merge ran");
        store.end_merge();
        assert_eq!(pass.account.took, 1);
        assert_eq!(pass.replaced(), None, "nothing to say");
        assert_eq!(
            pass.diagnostics(),
            "BT_PERSIST settings.json merged: took 1 from the file, kept 0 from Folio, conflicts 0, \
             doors refused: 0"
        );

        // A conflict: Folio picked dark, the file says system, twice over.
        assert!(store.store(SettingsV1 {
            theme_mode: ThemeModeV1::Dark,
            ..clean.clone()
        }));
        for size in [17, 18] {
            let theirs = SettingsV1 {
                theme_mode: ThemeModeV1::System,
                terminal_font_size: size,
                ..clean.clone()
            };
            let _ = store
                .document()
                .hear_read(bt_persist::StableRead::Bytes(bytes(&theirs)));
        }
        let pass = store.begin_merge(PLATFORM).expect("a merge ran");
        store.end_merge();
        let replaced = pass.replaced().expect("the conflict is named");
        assert_eq!(replaced.file, SETTINGS_FILE_NAME);
        assert_eq!(
            replaced.values,
            vec![crate::settings::SettingsRow::Theme.title().to_owned()],
            "once, by its row's title"
        );
        assert_eq!(store.document().desired().theme_mode, ThemeModeV1::System);
        let _ = std::fs::remove_dir_all(&root);
    }
}
