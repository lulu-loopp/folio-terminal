//! **The three configuration doors on the About page** (0.4.4 ticket 05):
//! `Export…`, `Import…` and `Open` on the settings folder.
//!
//! What each part of an import *means* is `settings_bundle`'s; this is where it
//! is done to a running window. Everything here runs on the window thread and
//! touches no more disk than a Settings press does: the two dialogs are posted
//! and collected on a later turn (`bt_platform::SaveFilePicker`, and the file
//! chooser `ImagePicker` already is), the import file is read once through
//! `bt_persist::read_export` — the bounded read on `file_reads`' settings lane —
//! and the writes are the stores' own, the settings batch landing as one write.

use std::path::Path;

use anyhow::Result;
use bt_persist::{ExportParts, FallbackReason, KEYBINDINGS_SCHEMA_VERSION, KeybindingsV1};

use crate::settings_bundle::{self, ImportFault, SettingChange};
use crate::{FilePick, Runtime, hang_watch, i18n, persist, profiles, schemes, settings, toast};

impl Runtime<'_> {
    /// **A press on one of the three doors.**
    ///
    /// Export and import only *post* a dialog: the system's own dialogs run a
    /// nested loop, and their answers are collected on a later turn by
    /// [`Self::apply_save_pick_result`] and `apply_image_pick_result`. The folder
    /// goes to the reveal door every `Show in Explorer` in this window goes
    /// through, on the OS hand-off lane — there is no second door to the system.
    pub(crate) fn open_configuration_door(&mut self, door: settings::ConfigurationDoor) {
        match door {
            settings::ConfigurationDoor::Export => {
                if let Err(error) = self
                    .window
                    .save_picker
                    .request(None, bt_persist::EXPORT_FILE_NAME)
                {
                    eprintln!("recoverable save dialog failure: {error}");
                }
            }
            settings::ConfigurationDoor::Import => match self
                .window
                .image_picker
                .request(bt_platform::FilePickKind::SettingsFile, None)
            {
                Ok(true) => self.window.image_pick_pending = Some(FilePick::SettingsImport),
                // Already queued or already open: whoever asked first keeps it.
                Ok(false) => {}
                Err(error) => eprintln!("recoverable settings file chooser failure: {error}"),
            },
            settings::ConfigurationDoor::Folder => {
                self.reveal_in_explorer(&persist::storage_dir());
            }
        }
    }

    /// Collect the save dialog's answer, once, after it has shut.
    pub(in crate::runtime) fn apply_save_pick_result(&mut self) -> Result<()> {
        let Some(result) = self.window.save_picker.take_result() else {
            return Ok(());
        };
        match result {
            // Cancelled: nothing was chosen, so nothing is written anywhere.
            Ok(None) => Ok(()),
            Ok(Some(path)) => self.export_settings_to(&path),
            Err(error) => {
                eprintln!("recoverable save dialog failure: {error}");
                Ok(())
            }
        }
    }

    /// **Write the export** — what is in force now, not what the files last
    /// said: the settings document the store holds, the profile table and the
    /// shortcut departures as this window has them, and every scheme file in
    /// the reader's folder.
    pub(crate) fn export_settings_to(&mut self, path: &Path) -> Result<()> {
        let (scheme_documents, unreadable) = schemes::user_documents();
        let keybindings = KeybindingsV1 {
            schema_version: KEYBINDINGS_SCHEMA_VERSION,
            bindings: self
                .app
                .shortcuts
                .overrides()
                .into_iter()
                .map(|entry| bt_persist::BindingOverrideV1 {
                    action: entry.id,
                    chord: entry.chord,
                })
                .collect(),
        };
        let banner = crate::version::banner();
        let profiles = profiles::to_file();
        let written = bt_persist::serialize_export(ExportParts {
            exported_by: &banner,
            settings: self.app.settings_store.loaded(),
            profiles: &profiles,
            keybindings: &keybindings,
            schemes: &scheme_documents,
        })
        .and_then(|bytes| {
            hang_watch::during(hang_watch::Station::SettingsWrite, || {
                bt_persist::atomic_write(path, &bytes)
            })
        });
        match written {
            Ok(()) => self.toast(
                toast::ToastKind::Info,
                toast::ToastAnchor::Window,
                Some(i18n::Text::SettingsExported.text().to_owned()),
                path.display().to_string(),
            )?,
            Err(error) => {
                eprintln!("BT_PERSIST export to {} failed: {error}", path.display());
                self.toast(
                    toast::ToastKind::Error,
                    toast::ToastAnchor::Window,
                    Some(i18n::Text::SettingsExportFailed.text().to_owned()),
                    error.to_string(),
                )?;
            }
        }
        // A scheme file that is not JSON cannot travel as a JSON value. It is
        // named rather than dropped without a word — the folder's own rule.
        let skipped: Vec<ImportFault> = unreadable
            .into_iter()
            .map(|reject| ImportFault {
                what: format!("{}/{}", schemes::USER_SCHEME_DIR, reject.file),
                reason: reject.reason,
            })
            .collect();
        self.say_skipped(&skipped)
    }

    /// **Import an export file** — each part through the door the same document
    /// takes when it is edited by hand, and in the order that lets the later
    /// parts name what the earlier ones brought: schemes, then profiles, then
    /// shortcuts, then the settings that name a scheme and a profile.
    ///
    /// No question first (ticket 05's scope): the import is the reader's
    /// deliberate gesture. What could not be put in force is named, by row, on
    /// one card; the full reasons go to `diagnostics.log`.
    pub(crate) fn import_settings_from(&mut self, path: &Path) -> Result<()> {
        let parts = match bt_persist::read_export(path) {
            Ok(parts) => parts,
            Err(refusal) => {
                eprintln!("BT_PERSIST import of {} refused: {refusal}", path.display());
                return self.toast(
                    toast::ToastKind::Error,
                    toast::ToastAnchor::Window,
                    Some(i18n::Text::SettingsImportFailed.text().to_owned()),
                    refusal.to_string(),
                );
            }
        };
        let mut faults: Vec<ImportFault> = Vec::new();
        let mut elsewhere: Vec<&'static str> = Vec::new();

        if let Some(part) = parts.schemes {
            match part {
                Ok(documents) => self.import_schemes(&documents, &mut faults)?,
                Err(reason) => faults.push(part_refused(schemes::USER_SCHEME_DIR, &reason)),
            }
        }
        if let Some(part) = parts.profiles {
            match part {
                // `reread_profiles`' comparison: a table identical to the one in
                // force is no news, and a different one is put in force by the
                // same function a hand edit's is.
                Ok(file) => {
                    if self.app.profiles_store.store(file) {
                        for fault in self.take_profile_table()? {
                            faults.push(ImportFault {
                                what: persist::PROFILES_FILE_NAME.to_owned(),
                                reason: i18n::profile_entry_fault(&fault),
                            });
                        }
                    }
                }
                Err(reason) => faults.push(part_refused(persist::PROFILES_FILE_NAME, &reason)),
            }
        }
        if let Some(part) = parts.keybindings {
            match part {
                Ok(file) => {
                    let (table, refused) = settings_bundle::import_shortcuts(&file);
                    faults.extend(refused);
                    if table != self.app.shortcuts {
                        self.app.shortcuts = table;
                        self.store_keybindings();
                    }
                }
                Err(reason) => {
                    faults.push(part_refused(persist::KEYBINDINGS_FILE_NAME, &reason));
                }
            }
        }
        if let Some(part) = parts.settings {
            match part {
                Ok(imported) => {
                    self.import_settings_part(&imported, &mut faults, &mut elsewhere)?
                }
                Err(reason) => faults.push(part_refused(persist::SETTINGS_FILE_NAME, &reason)),
            }
        }

        // The dialog may be standing on a row or a page the import has just
        // moved, and every value it draws has changed under it.
        let (rows, shortcuts, profile_lines, scheme_files, values) = self.settings_content();
        let content =
            self.settings_dialog(&rows, &shortcuts, &profile_lines, &scheme_files, &values);
        self.window.settings.keep_focus_reachable(content);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }

        self.toast(
            toast::ToastKind::Info,
            toast::ToastAnchor::Window,
            Some(i18n::Text::SettingsImported.text().to_owned()),
            path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            ),
        )?;
        self.say_skipped(&faults)?;
        if !elsewhere.is_empty() {
            self.toast(
                toast::ToastKind::Info,
                toast::ToastAnchor::Window,
                Some(i18n::Text::SettingsNotOnThisMachine.text().to_owned()),
                elsewhere.join(", "),
            )?;
        }
        Ok(())
    }

    /// The `schemes` part: judged by `plan_schemes`, the good ones written into
    /// the folder, and the folder re-read in process — the very function its
    /// watch calls — so the settings part after this can name what just arrived.
    fn import_schemes(
        &mut self,
        documents: &std::collections::BTreeMap<String, serde_json::Value>,
        faults: &mut Vec<ImportFault>,
    ) -> Result<()> {
        let (existing, _) = schemes::user_documents();
        let plan = settings_bundle::plan_schemes(&existing, documents);
        faults.extend(plan.faults);
        if plan.writes.is_empty() {
            return Ok(());
        }
        let directory = match schemes::user_dir() {
            Ok(directory) => directory,
            Err(error) => {
                faults.push(ImportFault {
                    what: schemes::USER_SCHEME_DIR.to_owned(),
                    reason: error.to_string(),
                });
                return Ok(());
            }
        };
        for (file, text) in plan.writes {
            if let Err(error) = hang_watch::during(hang_watch::Station::SettingsWrite, || {
                bt_persist::atomic_write(&directory.join(&file), text.as_bytes())
            }) {
                faults.push(ImportFault {
                    what: format!("{}/{file}", schemes::USER_SCHEME_DIR),
                    reason: error.to_string(),
                });
            }
        }
        self.reread_schemes()
    }

    /// The `settings` part: every value that differs put through its row's own
    /// door, with the store's writes held so the batch lands as one write.
    ///
    /// A value its door did not take — a backdrop this Windows cannot draw, a
    /// size outside a slider's range — is named rather than assumed: what is in
    /// the store afterwards is asked, not what was offered.
    fn import_settings_part(
        &mut self,
        imported: &bt_persist::SettingsV1,
        faults: &mut Vec<ImportFault>,
        elsewhere: &mut Vec<&'static str>,
    ) -> Result<()> {
        let plan = settings_bundle::plan_settings(
            self.app.settings_store.loaded(),
            imported,
            bt_platform::host_platform(),
        );
        self.app.settings_store.hold_writes();
        let mut outcome = Ok(());
        for change in &plan.apply {
            outcome = self.apply_imported_setting(change.clone());
            if outcome.is_err() {
                break;
            }
        }
        for change in &plan.elsewhere {
            self.store_imported_value(change);
            if let Some(row) = change.row() {
                elsewhere.push(row.title());
            }
        }
        self.app.settings_store.release_writes();
        outcome?;
        for change in &plan.apply {
            let mut probe = self.app.settings_store.loaded().clone();
            change.write_into(&mut probe);
            if &probe != self.app.settings_store.loaded() {
                faults.push(ImportFault {
                    what: change.row().map_or_else(
                        || persist::SETTINGS_FILE_NAME.to_owned(),
                        |row| row.title().to_owned(),
                    ),
                    reason: "this machine did not take the value".to_owned(),
                });
            }
        }
        Ok(())
    }

    /// **One imported value, through the function a press on its row calls.**
    ///
    /// The values no row shows, and the two rows whose press only records a
    /// value read where it is used, are stored as they are — for them storing is
    /// the whole of putting them in force.
    fn apply_imported_setting(&mut self, change: SettingChange) -> Result<()> {
        match change {
            SettingChange::Theme(mode) => {
                self.apply_theme_mode(mode)?;
            }
            SettingChange::DisplayFormulas(enabled) => {
                self.apply_display_formulas(enabled)?;
            }
            SettingChange::InlineFormulas(enabled) => {
                self.apply_inline_formulas(enabled)?;
            }
            SettingChange::RepairRowBreaks(enabled) => {
                self.apply_repair_row_breaks(enabled)?;
            }
            SettingChange::Tables(enabled) => {
                self.apply_tables(enabled)?;
            }
            SettingChange::BlockMaxHeight(height) => {
                self.apply_block_max_height(height)?;
            }
            SettingChange::DefaultProfile(id) => {
                self.apply_default_profile(&id)?;
            }
            SettingChange::GitPanel(enabled) => {
                self.apply_git_panel(enabled)?;
            }
            SettingChange::SplitDirection(direction) => {
                self.apply_split_direction(direction)?;
            }
            SettingChange::SearchEngine(engine) => {
                self.apply_search_engine(engine)?;
            }
            SettingChange::Language(language) => {
                self.apply_language(language)?;
            }
            SettingChange::TerminalFont {
                family,
                cjk_family,
                size,
            } => {
                self.apply_terminal_font(family, cjk_family, size)?;
            }
            SettingChange::LightScheme(name) => {
                self.apply_scheme(Some(name), None)?;
            }
            SettingChange::DarkScheme(name) => {
                self.apply_scheme(None, Some(name))?;
            }
            SettingChange::BackgroundImage(path) => {
                self.apply_background_image(path)?;
            }
            SettingChange::BackgroundFit(fit) => {
                self.apply_image_fit(fit)?;
            }
            SettingChange::ImageOpacity(value) => {
                self.apply_slider(settings::SettingsRow::ImageOpacity, value)?;
            }
            SettingChange::BackgroundOpacity(value) => {
                self.apply_slider(settings::SettingsRow::BackgroundOpacity, value)?;
            }
            SettingChange::Acrylic(enabled) => {
                self.apply_acrylic(enabled)?;
            }
            SettingChange::AlwaysOnTop(enabled) => {
                self.apply_always_on_top(enabled)?;
            }
            SettingChange::Scrollback(lines) => {
                self.apply_scrollback_lines(lines)?;
            }
            // The row's door turns this window's posture and records it — but
            // only when the posture moves. A window already standing in the
            // imported posture still owes the file the value.
            ref change @ SettingChange::FocusMode(on) => {
                self.set_focus_mode(on)?;
                self.store_imported_value(change);
            }
            SettingChange::MinimumContrast(floor) => {
                self.apply_minimum_contrast(floor)?;
            }
            SettingChange::TerminalNotifications(enabled) => {
                self.apply_terminal_notifications(enabled);
            }
            SettingChange::PowerShellOffer(enabled) => {
                self.press_powershell_integration_offer(enabled)?;
            }
            SettingChange::FocusCardHeight(height) => {
                self.apply_focus_card_height(height)?;
            }
            SettingChange::LineWrapping(wrapping) => {
                self.apply_line_wrapping(wrapping)?;
            }
            SettingChange::KeyHints(enabled) => {
                self.apply_key_hints(enabled)?;
            }
            SettingChange::TurnEndNotification(enabled) => {
                self.apply_turn_end_notification(enabled);
            }
            SettingChange::CopyOnSelect(enabled) => {
                self.apply_copy_on_select(enabled);
            }
            SettingChange::UpdateCheck(enabled) => {
                self.apply_update_check(enabled);
            }
            SettingChange::QuakeHeight(value) => {
                self.apply_slider(settings::SettingsRow::QuakeHeight, value)?;
            }
            SettingChange::QuakeWidth(value) => {
                self.apply_slider(settings::SettingsRow::QuakeWidth, value)?;
            }
            SettingChange::QuakeDismiss(enabled) => {
                self.apply_quake_dismiss(enabled)?;
            }
            SettingChange::QuakeCommand(command) => {
                self.apply_quake_command(command)?;
            }
            SettingChange::QuakeRestore(rung) => {
                self.apply_quake_restore(rung)?;
            }
            SettingChange::LaunchOpens(opens) => {
                self.apply_launch_opens(opens)?;
            }
            SettingChange::OptionSendsAlt(enabled) => {
                self.apply_option_sends_alt(enabled)?;
            }
            // Read where they are used, so the store is their door: the answer
            // the PSReadLine card was given, the open Advanced groups, the
            // gesture hint's receipt, the summoned terminal's profile — stored
            // by id, which is what its row's press stores too, rather than
            // through the row's index into this table — and its top gap.
            ref change @ (SettingChange::PsReadLineInvite(_)
            | SettingChange::AdvancedOpen(_)
            | SettingChange::CardsGestureHintOffer(_)
            | SettingChange::QuakeProfile(_)
            | SettingChange::QuakeTopGap(_)) => self.store_imported_value(change),
        }
        Ok(())
    }

    /// Write one imported value into the settings document as it stands.
    fn store_imported_value(&mut self, change: &SettingChange) {
        let mut settings = self.app.settings_store.loaded().clone();
        change.write_into(&mut settings);
        self.app.settings_store.store(settings);
    }

    /// **One card naming what an export or an import left out**, row by row;
    /// the reasons go to `diagnostics.log`, one line each.
    ///
    /// One card and not one per row, because the toast host holds three and an
    /// import from another platform can name a dozen: a card per row would
    /// drop the rows past the third without a word.
    fn say_skipped(&mut self, faults: &[ImportFault]) -> Result<()> {
        if faults.is_empty() {
            return Ok(());
        }
        for fault in faults {
            eprintln!("BT_PERSIST skipped {}: {}", fault.what, fault.reason);
        }
        let names: Vec<&str> = faults.iter().map(|fault| fault.what.as_str()).collect();
        self.toast(
            toast::ToastKind::Error,
            toast::ToastAnchor::Window,
            Some(i18n::Text::SettingsSkipped.text().to_owned()),
            names.join(", "),
        )
    }
}

/// A whole part the importing build could not read, named by its file.
fn part_refused(what: &str, reason: &FallbackReason) -> ImportFault {
    ImportFault {
        what: what.to_owned(),
        reason: format!("{reason:?}"),
    }
}
