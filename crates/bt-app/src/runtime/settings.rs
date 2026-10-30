//! **The settings page, on the window thread** (re-sort D5, ruled 2026-10-26): `Runtime`'s
//! methods that open, close and scroll the sheet, lay it out and read its content and values,
//! route the pointer into it (the four `settings::geometry::PointerHost` methods), apply what a row
//! or a slider answers, and work the Advanced groups and rows, the profile editor's fields, scheme
//! edit and delete, the cards' gesture hint and focus mode's on/off.
//!
//! `crate::settings` is the sheet's model (rows, layout, geometry, values, and the trait the window
//! drives it through); this file reads it, and it never reads this one. Moved out of `main.rs` by
//! K2 (T-SPLIT-2B) batches 23 and 24. Bodies unchanged.

use crate::window_seam::prelude::*;
use crate::{
    Announce, ApplicationChange, FilePick, RAIL_TRANSITION, RevealTween, Runtime,
    attention_copilot, attention_wire, block_max_height_px, cardhint, context_menu, explorer_menu,
    handoff_lane, hang_watch, i18n, install_minimum_contrast, marks, preview, profiles,
    programs_lane, psreadline, rail_state_for, schemes, seats, settings, shell_integration,
    shortcuts, text_field, toast, update, update_card,
};
use anyhow::Result;
use bt_persist::ThemeModeV1;
use bt_render::{FrameSource, FrameTrigger, Theme, ThemeChange, current_cursor_style, set_theme};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, MouseScrollDelta};

impl<B: Backend> Runtime<'_, B> {
    /// **Note whether this window is in Cards, and raise the bubble on the frame
    /// it first is** (§7.21).
    ///
    /// Asked once a turn against `rail_posture()` — the very posture the column
    /// is solved and painted from — rather than hooked onto the doors that turn
    /// the mode on. There are three of those (a chord, a settings row, and a
    /// window that opens straight into Cards because the file said so), the
    /// third does not go through `set_focus_mode` at all, and a hook on each
    /// would be three chances for a fourth to be added without one.
    ///
    /// **The offer is spent on the frame the bubble is raised, and written
    /// through at once.** Not when it comes down: a window that is closed, or
    /// that crashes, four seconds into a reader's first visit has still shown
    /// them the sentence, and a debt that is only discharged on a clean exit is
    /// a debt that greets them again tomorrow. The write is `settings.json`'s
    /// ordinary undebounced one, which is what every other row in that file
    /// takes.
    pub(crate) fn note_card_hint(&mut self, now: Instant) -> Result<()> {
        let in_cards = self.rail_posture().draws_focus_rail();
        let offered = self.app.settings_store.loaded().cards_gesture_hint_offer;
        let moved = match self.window.card_hint.observe(in_cards, offered, now) {
            cardhint::CardHint::Unchanged => false,
            cardhint::CardHint::Raised => {
                let mut settings = self.app.settings_store.loaded().clone();
                settings.cards_gesture_hint_offer = false;
                let _ = self.app.settings_store.store(settings);
                true
            }
            cardhint::CardHint::Lowered => true,
        };
        if moved {
            self.repaint_card_hint()?;
        }
        Ok(())
    }

    /// Take the bubble down when its four seconds are up, and pay the nudge's
    /// frames until the card it points at is home.
    ///
    /// The two are one function because they are one surface's clock, and they
    /// are asked in this order because a bubble whose dwell has just ended has
    /// nothing left to nudge.
    pub(crate) fn advance_card_hint(&mut self, now: Instant) -> Result<()> {
        // The bubble's four seconds running out is state, not a frame of
        // animation, and is never paced (closure review O4, 2026-09-18): behind
        // the gate a printing pane would leave the sentence on the glass for as
        // long as it printed.
        let ended = self.window.card_hint.expire(now) != cardhint::CardHint::Unchanged;
        // The nudge's own frames are the paced half.
        if !ended && !self.animation_frame_is_due() {
            return Ok(());
        }
        let nudging = self.window.card_hint.nudge_moving(now, self.app.motion);
        if ended || nudging {
            self.repaint_card_hint()?;
        }
        Ok(())
    }

    /// **Both surfaces, because this hint is two things on two levels**: the
    /// bubble is an overlay layer and the nudge is a card in the chrome, and a
    /// frame that rebuilt one of them would move the caption without the
    /// picture or the picture without the caption.
    ///
    /// Written once rather than at each of the three call sites, so the pairing
    /// cannot come apart. The two `refresh_` calls are both made before either
    /// answer is read — `||` would skip the second whenever the first said yes,
    /// and the one it skipped is the one that would then be a frame behind.
    fn repaint_card_hint(&mut self) -> Result<()> {
        let chrome = self.refresh_chrome();
        let overlay = self.refresh_overlay();
        if chrome || overlay {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The gear's verb: open the dialog, or shut the one that is open.
    ///
    /// Opening drops the chrome's hover state, because the scrim now stands
    /// between the pointer and the gear it is over — the highlight would be a
    /// button claiming to be reachable through a modal.
    pub(crate) fn toggle_settings_panel(&mut self) -> Result<()> {
        // **The font list rescans when the dialog opens** (user ruling
        // 2026-08-19), which is what `Install fonts…` at the foot of that picker
        // owes the reader: the gesture it starts — leave, drop a file on a
        // Windows page, come back — crosses exactly this call.
        //
        // **Asked for, not paid for** (GitHub issue #3). Until 2026-09-15 this
        // was a mark, and the first reader after it walked the machine's whole
        // font collection on this thread — and the first reader is the very next
        // statement, because `settings_values` asks which family is ticked. That
        // is the several seconds an outside user reported as the window freezing
        // when the gear is clicked. The walk is now a worker's, the seed this
        // hands it is the family in force so no picker is ever drawn blank, and
        // the answer arrives as `AppEvent::FontsScanned`.
        //
        // Started whether the press opens or closes, because starting costs
        // nothing that is not already coalesced and asking which way this press
        // went would be a second reading of `is_open` between two frames that
        // disagree about it.
        settings::begin_monospace_scan(
            &self.app.settings_store.loaded().terminal_font_family,
            &self.app.settings_store.loaded().terminal_cjk_font_family,
        );
        let (rows, shortcuts, profile_lines, scheme_files, values) = self.settings_content();
        if self.window.settings.is_open() {
            self.settings_closed();
        }
        self.window.settings.toggle(self.settings_dialog(
            &rows,
            &shortcuts,
            &profile_lines,
            &scheme_files,
            &values,
        ));
        // A dialog opens at its top. The distance belongs to the sitting the
        // wheel moved, not to the preference the dialog edits, so it does not
        // outlive the dialog being shut.
        self.window.settings_scroll = 0.0;
        if self.window.settings.is_open() {
            // **And the one row that holds text rather than a choice is filled in
            // from the file** (§7.54e ⑤). Here and not every frame, for
            // `SettingsPanel::seed_quake_command`'s own reason: a field is what
            // somebody is halfway through typing, and re-reading the file under a
            // caret would drop it on the frame after every keystroke.
            let command = self
                .app
                .settings_store
                .loaded()
                .quake_startup_command
                .clone();
            self.window.settings.seed_quake_command(&command);
            self.window.seat_pointer.hover = None;
            self.apply_pointer_cursor();
        } else if let Some(position) = self.window.pointer_position {
            self.update_chrome_hover(position)?;
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// **Take a picker item** — everything choosing one does to the process.
    ///
    /// One function and not a block inside the press router, because there are
    /// now two roads to the same choice: a click on the item, and Enter on the
    /// item the ring is on. A verb reachable two ways whose body lives on one of
    /// them is a verb that half works, and the half that would have been missed
    /// here is the half the keyboard was built for.
    pub(crate) fn apply_settings_choice(&mut self, target: settings::SettingsTarget) -> Result<()> {
        self.apply_settings_choice_announcing(target, Announce::Everything)
    }

    /// The same dispatcher, told **who asked**.
    ///
    /// One parameter and not a second dispatcher, because there is only one
    /// question the two callers answer differently and it is not "what does this
    /// press do": a switch flipped on the Settings page has no nearby request to
    /// confirm it, so a success is worth a card; a row on the first-run card was
    /// asked for half a second ago along with three others, so four cards is
    /// noise. Every failure still speaks, on both surfaces, in the same words.
    pub(crate) fn apply_settings_choice_announcing(
        &mut self,
        target: settings::SettingsTarget,
        announce: Announce,
    ) -> Result<()> {
        if let Some(action) = settings::profile_action_requested(target) {
            self.apply_profile_action(action)?;
        }
        if let settings::SettingsTarget::ProfileEnable(index) = target {
            self.enable_via_profile(index);
        }
        if let settings::SettingsTarget::ProfileCopyPolicyCommand(_) = target {
            self.copy_policy_command()?;
        }
        self.apply_editor_choice(target)?;
        if let Some(mode) = settings::theme_requested(target) {
            self.apply_theme_mode(mode)?;
        }
        if let Some(style) = settings::cursor_style_requested(target) {
            self.apply_cursor_style(style)?;
        }
        if let Some(enabled) = settings::display_formulas_requested(target) {
            self.apply_display_formulas(enabled)?;
        }
        if let Some(enabled) = settings::tables_requested(target) {
            self.apply_tables(enabled)?;
        }
        if let Some(height) = settings::block_max_height_requested(target) {
            self.apply_block_max_height(height)?;
        }
        if let Some(lines) = settings::scrollback_lines_requested(target) {
            self.apply_scrollback_lines(lines)?;
        }
        if let Some(wrapping) = settings::line_wrapping_requested(target) {
            self.apply_line_wrapping(wrapping)?;
        }
        if let Some(height) = settings::focus_card_height_requested(target) {
            self.apply_focus_card_height(height)?;
        }
        if let Some(enabled) = settings::inline_formulas_requested(target) {
            self.apply_inline_formulas(enabled)?;
        }
        if let Some(enabled) = settings::repair_row_breaks_requested(target) {
            self.apply_repair_row_breaks(enabled)?;
        }
        if let Some(enabled) = settings::git_panel_requested(target) {
            self.apply_git_panel(enabled)?;
        }
        // The uninstall's switch is the panel's, for this visit, and never the
        // settings file's (T-UNINSTALL-UX).
        if let Some(on) = settings::uninstall_remove_data_requested(target) {
            self.window.settings.set_uninstall_remove_data(on);
        }
        if let Some(enabled) = settings::update_check_requested(target) {
            self.apply_update_check(enabled);
        }
        if target == settings::SettingsTarget::MenuAction(settings::SettingsRow::AboutVersion) {
            match self.settings_values().version_update.link {
                Some(update_card::VersionLink::WhatsNew { tag }) => {
                    self.hand_url_to_the_browser(&format!("{}/tag/{tag}", update::RELEASES_PAGE))?;
                }
                Some(update_card::VersionLink::Details) => {
                    let window = self.window.window.id();
                    self.app.update_job.show_failure(window);
                }
                None => {}
            }
        }
        // **The one press in this dialog that leaves the window.** It is a foot
        // verb and not a choice, so it is answered here beside the choices for
        // `apply_settings_choice`'s founding reason rather than in a second
        // dispatcher of its own.
        if settings::update_row_foot_requested(target) {
            self.press_update_row_foot()?;
        }
        if let Some(enabled) = settings::key_hints_requested(target) {
            self.apply_key_hints(enabled)?;
        }
        if let Some(enabled) = settings::option_sends_alt_requested(target) {
            self.apply_option_sends_alt(enabled)?;
        }
        if let Some(enabled) = settings::copy_on_select_requested(target) {
            self.apply_copy_on_select(enabled);
        }
        if let Some(enabled) = settings::multiline_paste_ask_requested(target) {
            self.apply_multiline_paste_ask(enabled);
        }
        if let Some(enabled) = settings::terminal_notifications_requested(target) {
            self.apply_terminal_notifications(enabled);
        }
        if let Some(enabled) = settings::turn_end_notification_requested(target) {
            self.apply_turn_end_notification(enabled);
        }
        if let Some(enabled) = settings::shell_integration_requested(target) {
            self.apply_shell_integration(enabled)?;
        }
        // The machine fact travels with the press: what the switch's `On` reaches
        // is `explorer_menu::place_when_on`'s answer about this Windows and this
        // folder, and the door that reads the press is not the place to ask.
        if let Some(place) = settings::explorer_place_requested(
            target,
            explorer_menu::first_page_offered(
                explorer_menu::supported(),
                explorer_menu::package_file().is_some(),
            ),
        ) {
            self.apply_explorer_place(place, announce)?;
        }
        if let Some(install) = settings::claude_hooks_requested(target) {
            self.apply_claude_hooks(install, announce)?;
        }
        if let Some(install) = settings::codex_notify_requested(target) {
            self.apply_codex_notify(install, announce)?;
        }
        if let Some(install) = settings::copilot_hooks_requested(target) {
            self.apply_copilot_hooks(install, announce)?;
        }
        if let Some(engine) = settings::search_engine_requested(target) {
            self.apply_search_engine(engine)?;
        }
        if let Some(opens) = settings::launch_opens_requested(target) {
            self.apply_launch_opens(opens)?;
        }
        if let Some(direction) = settings::split_direction_requested(target) {
            self.apply_split_direction(direction)?;
        }
        if let Some(floor) = settings::minimum_contrast_requested(target) {
            self.apply_minimum_contrast(floor)?;
        }
        if let Some(scheme) = settings::web_color_scheme_requested(target) {
            self.apply_web_color_scheme(scheme)?;
        }
        if let Some(language) = settings::language_requested(target) {
            self.apply_language(language)?;
        }
        if let Some(family) = settings::terminal_font_requested(target) {
            let size = self.app.settings_store.loaded().terminal_font_size;
            let cjk_family = self
                .app
                .settings_store
                .loaded()
                .terminal_cjk_font_family
                .clone();
            self.apply_terminal_font(family.to_owned(), cjk_family, size)?;
        }
        if let Some(cjk_family) = settings::terminal_cjk_font_requested(target) {
            let (family, size) = {
                let current = self.app.settings_store.loaded();
                (
                    current.terminal_font_family.clone(),
                    current.terminal_font_size,
                )
            };
            self.apply_terminal_font(family, cjk_family.to_owned(), size)?;
        }
        if let Some(size) = settings::font_size_requested(target) {
            let family = self
                .app
                .settings_store
                .loaded()
                .terminal_font_family
                .clone();
            let cjk_family = self
                .app
                .settings_store
                .loaded()
                .terminal_cjk_font_family
                .clone();
            self.apply_terminal_font(family, cjk_family, size)?;
        }
        if let Some(name) = settings::light_scheme_requested(target) {
            self.apply_scheme(Some(name.to_owned()), None)?;
        }
        if let Some(name) = settings::dark_scheme_requested(target) {
            self.apply_scheme(None, Some(name.to_owned()))?;
        }
        // Both rail combos go through the one constructor: the layout choice
        // keeps the sidebar mode standing and the sidebar choice keeps the
        // layout standing, and Q190's combination rule inside `RailState`
        // decides what that pair actually puts on screen.
        if let Some(layout) = settings::tab_layout_requested(target) {
            self.set_rail_state(rail_state_for(layout, self.window.rail.mode))?;
        }
        // **Door 1 of two** (§7.1.6b′ ①). Deliberately *not* through
        // `rail_state_for`: focus mode is orthogonal to both rail rows, so a
        // press here must leave `Tab layout` and `Sidebar` exactly where they
        // stand — which is what makes leaving the mode need no restoring code.
        if let Some(on) = settings::focus_mode_requested(target) {
            self.set_focus_mode(on)?;
        }
        if let Some(mode) = settings::sidebar_mode_requested(target) {
            self.set_rail_state(rail_state_for(self.window.rail.layout, mode))?;
        }
        if let Some(id) = settings::default_profile_requested(target) {
            self.apply_default_profile(&id)?;
        }
        if let Some(source) = settings::background_image_requested(target) {
            match source {
                settings::ImageSource::None => {
                    self.apply_background_image(String::new())?;
                }
                // A verb, not a value: nothing is stored until the chooser comes
                // back, and it may come back with nothing.
                settings::ImageSource::Choose => self.browse_for_background_image(),
            }
        }
        if let Some(fit) = settings::image_fit_requested(target) {
            self.apply_image_fit(fit)?;
        }
        if let Some(enabled) = settings::acrylic_requested(target) {
            self.apply_acrylic(enabled)?;
        }
        if let Some(enabled) = settings::always_on_top_requested(target) {
            self.apply_always_on_top(enabled)?;
        }
        if let Some(enabled) = settings::quake_dismiss_requested(target) {
            self.apply_quake_dismiss(enabled)?;
        }
        if let Some(rung) = settings::quake_restore_requested(target) {
            self.apply_quake_restore(rung)?;
        }
        if let Some(profile) = settings::quake_profile_requested(target) {
            self.apply_quake_profile(profile)?;
        }
        if let settings::SettingsTarget::Advanced(category) = target {
            self.toggle_advanced_group(category)?;
        }
        if let settings::SettingsTarget::ResetAdvanced(category) = target {
            self.reset_advanced_group(category)?;
        }
        // **The picker's own three verbs** (user ruling 2026-08-19). Each one
        // names the row it acts on — the two marks by their item's index, the
        // action by the picker it ends — so none of them has to be told which
        // scheme, and none of them can act on a row the reader is not looking
        // at. That was the whole fault the foot's three buttons had.
        match target {
            settings::SettingsTarget::MenuAction(row) => match row {
                settings::SettingsRow::LightScheme => self.add_scheme(true)?,
                settings::SettingsRow::DarkScheme => self.add_scheme(false)?,
                settings::SettingsRow::TerminalFont => self.open_font_settings()?,
                settings::SettingsRow::TerminalCjkFont => self.open_font_settings()?,
                _ => {}
            },
            settings::SettingsTarget::MenuItemEdit(row, index) => {
                self.edit_scheme_at(row, index)?;
            }
            settings::SettingsTarget::MenuItemDelete(row, index) => {
                self.delete_scheme_at(row, index)?;
            }
            // **The About page's three doors** (GitHub issue #3). One arm and
            // not three, because the destination is the row's own answer — see
            // `settings::SettingsRow::link_destination`, where a row added
            // tomorrow says where it goes and this router does not have to be
            // told.
            //
            // **Two doors and not one** (owner ruling 2026-09-15). An address
            // goes through the hand-off every other address in this window
            // leaves through, which is what keeps one policy about what a
            // browser is handed. A file goes through `open_local_path`, which is
            // this window's one door out to the machine that takes a path and
            // the one that will not start a program: the licences row opens the
            // copy that shipped with this build where there is one, because
            // those are the notices this binary was actually linked against.
            settings::SettingsTarget::Link(row) => match row.link_destination() {
                Some(settings::LinkDestination::Address(url)) => {
                    self.hand_url_to_the_browser(url)?;
                }
                // `open_local_path` reports its own failure the way the files
                // column's rows do — a window that stopped working because a
                // text editor would not start is a worse answer than a row that
                // quietly did nothing.
                Some(settings::LinkDestination::File(path)) => {
                    self.open_local_path(path)?;
                }
                // The configuration doors (0.4.4 ticket 05): two dialogs, whose
                // answers are collected on a later turn, and the storage folder
                // handed to the reveal door.
                Some(settings::LinkDestination::Configuration(door)) => {
                    self.open_configuration_door(door)?;
                }
                None if row == settings::SettingsRow::PowerShellProfileLine => {
                    shell_integration::begin_removal();
                }
                None if row == settings::SettingsRow::LegacyPsReadLine => {
                    psreadline::begin_legacy_removal(self.window_id());
                }
                None => {}
            },
            _ => {}
        }
        // Tab layout is the one choice that changes which rows exist, and the
        // focus may be standing on the row it just deleted. So is the
        // disclosure, which is the same sentence with eight rows in it.
        let (rows, shortcuts, profile_lines, scheme_files, values) = self.settings_content();
        let content =
            self.settings_dialog(&rows, &shortcuts, &profile_lines, &scheme_files, &values);
        self.window.settings.keep_focus_reachable(content);
        Ok(())
    }

    /// Turn one page's Advanced triangle, and write it down (§7.1.6c-5).
    ///
    /// Persisted, because the ruling says the state is remembered: a reader who
    /// has decided they want the background-picture rows in front of them has
    /// decided it about the product, and a disclosure that shut itself at every
    /// launch would be asking them again every day.
    fn toggle_advanced_group(&mut self, category: settings::SettingsCategory) -> Result<()> {
        let mut open = self.advanced_open();
        open.toggle(category);
        // **The group's own two hundred milliseconds** ([`bt_render::DISCLOSURE_REVEAL`]),
        // aimed on the frame of the press. `GRAB_EASE` because a hand has just
        // let go of this: it leaves at full speed, so the page answers the press
        // on the first frame rather than a fortieth of a second later.
        //
        // A page other than the one this press was on loses its tween outright,
        // and that is the field's own rule read back — there is one, and the
        // dialog draws one page.
        let now = Instant::now();
        let motion = self.app.motion;
        let target = f32::from(u8::from(open.is_open(category)));
        let mut tween = match self.window.advanced_reveal {
            Some((held, tween)) if held == category => tween,
            _ => RevealTween::resting_on(
                1.0 - target,
                bt_render::DISCLOSURE_REVEAL,
                bt_render::GRAB_EASE,
            ),
        };
        tween.retarget(target, now, motion);
        self.window.advanced_reveal = Some((category, tween));
        self.window.advanced_reveal_sample = self.drawn_advanced_reveal(now);
        let mut settings = self.app.settings_store.loaded().clone();
        settings.advanced_open = open.keys();
        self.app.settings_store.store(settings);
        // The page got taller or shorter under a scroll that was measured against
        // the other length. Not reset to the top — the reader is looking at the
        // heading they just pressed, and sending them to the top would move it
        // out from under their eye — but clamped, which the layout does for us on
        // the next call and is stated here so the number the runtime holds is not
        // left describing a page that no longer exists.
        if let Some(layout) = self.settings_layout() {
            self.window.settings_scroll =
                self.window.settings_scroll.clamp(0.0, layout.max_scroll());
        }
        Ok(())
    }

    /// **Put one page's advanced rows back**, hot, and say which page
    /// (§7.1.6c-5).
    ///
    /// Derived from the row table and not from a list of fields: the rows are
    /// `SettingsContent::advanced_rows`, which is `SettingsRow::advanced`
    /// filtered by page, so a row that joins the group is reset by this verb
    /// without the verb being edited — and a row that leaves it stops being.
    ///
    /// **No confirmation**, which is the shortcut page's own ruling for its own
    /// `Restore all defaults`: this dialog has no dirty gate to route a question
    /// through, every choice in it is written the instant it is made, and what
    /// this discards is exactly the eight rows the reader is looking at while
    /// they press it.
    ///
    /// **And one thing that is not a row** (§7.21, user ruling 2026-08-27). The
    /// Appearance page's verb also gives back the Cards column's first-arrival
    /// bubble, which is a debt rather than a preference and therefore has no row
    /// to be derived from. It is named here, once, and the alternative was
    /// worse in both directions: a *row* for it would be a switch that turns off
    /// something that has already happened and cannot happen again — a control
    /// with no reader — while leaving it out of every reset at all would make
    /// "shown once" mean "shown once per installation, with no way back", which
    /// is the one thing a reader who missed a four-second card cannot fix.
    ///
    /// This page and not another, because this is the page Cards lives on
    /// (`Cards` and `Card height` are two of its rows), and "put this page back
    /// the way it shipped" is the only sentence in this dialog that covers it.
    fn reset_advanced_group(&mut self, category: settings::SettingsCategory) -> Result<()> {
        let (rows, shortcuts, profile_lines, scheme_files, values) = self.settings_content();
        let advanced = self
            .settings_dialog(&rows, &shortcuts, &profile_lines, &scheme_files, &values)
            .advanced_rows(category);
        for row in advanced {
            self.reset_advanced_row(row)?;
        }
        if category == settings::SettingsCategory::Appearance {
            let mut settings = self.app.settings_store.loaded().clone();
            settings.cards_gesture_hint_offer =
                bt_persist::SettingsV1::default().cards_gesture_hint_offer;
            let _ = self.app.settings_store.store(settings);
        }
        self.toast(
            toast::ToastKind::Ok,
            toast::ToastAnchor::Window,
            None,
            // The rail's word and not the page's heading: `label` is
            // upper-cased at the source because it is a heading, and a heading
            // dropped into a sentence shouts. `nav_label` is the same page
            // named in running text, which is what a card is.
            i18n::advanced_reset_toast(category.nav_label()),
        )?;
        Ok(())
    }

    /// One advanced row, back to the value a fresh install has.
    ///
    /// Every arm goes through the same applier the row's own picker goes
    /// through, which is `apply_settings_choice`'s founding rule read once more:
    /// a value reachable two ways whose *effect* lives on one of them is a value
    /// that half applies. The default itself comes from
    /// `bt_persist::SettingsV1::default()` and is never written out here — a
    /// second copy of a default is the copy that goes stale.
    ///
    /// The non-advanced rows are named rather than swept into a `_`, exactly as
    /// [`settings::SettingsRow::advanced`] names them: a row added to the group
    /// tomorrow must not silently reset to nothing.
    fn reset_advanced_row(&mut self, row: settings::SettingsRow) -> Result<()> {
        use settings::SettingsRow as Row;
        let defaults = bt_persist::SettingsV1::default();
        match row {
            Row::BackgroundImage => {
                self.apply_background_image(defaults.background_image)?;
            }
            Row::ImageFit => {
                self.apply_image_fit(defaults.background_fit)?;
            }
            Row::ImageOpacity => {
                self.apply_slider(row, defaults.background_image_opacity)?;
            }
            Row::BackgroundOpacity => {
                self.apply_slider(row, defaults.background_opacity)?;
            }
            Row::Acrylic => {
                self.apply_acrylic(defaults.acrylic)?;
            }
            Row::AlwaysOnTop => {
                self.apply_always_on_top(defaults.always_on_top)?;
            }
            // The one advanced row whose value is a window shape rather than a
            // preference: it lives in `session.json` with the tab layout it
            // depends on, so its default is `RailMode`'s and not the settings
            // file's.
            Row::Sidebar => {
                self.set_rail_state(rail_state_for(self.window.rail.layout, seats::RailMode::default()))?;
            }
            Row::SplitDirection => {
                self.apply_split_direction(defaults.split_direction)?;
            }
            Row::MinimumContrast => {
                self.apply_minimum_contrast(defaults.minimum_contrast)?;
            }
            Row::WebPages => {
                self.apply_web_color_scheme(defaults.web_color_scheme)?;
            }
            // Not in any group, and therefore never handed here — see
            // `SettingsContent::advanced_rows`, which is what this loop walks.
            Row::LaunchOpens
            | Row::Theme
            | Row::LightScheme
            | Row::DarkScheme
            | Row::TerminalFont
            | Row::TerminalCjkFont
            | Row::FontSize
            | Row::Cursor
            | Row::TabLayout
            | Row::FocusMode
            | Row::FocusCardHeight
            | Row::Formulas
            | Row::InlineFormulas
            | Row::RepairRowBreaks
            | Row::Tables
            | Row::BlockMaxHeight
            | Row::GitPanel
            | Row::KeyHints
            | Row::OptionSendsAlt
            | Row::DefaultProfile
            | Row::SearchEngine
            | Row::Language
            // And doubly never: what it would be putting back is not a value in
            // this file at all, it is two keys in the user's own registry, and
            // `Reset to defaults` on a page is not a licence to change another
            // program's menu.
            // And for a third store, on the same rule: what a reset would be
            // putting back is a package registered in this user's own deployment
            // database, which is not a line in this file and not this verb's to
            // undo. One row asks about both since 2026-09-07.
            | Row::ContextMenu
            | Row::QuakeHotkey
            | Row::QuakeProfile
            | Row::QuakeCommand
            | Row::QuakeHeight
            | Row::QuakeWidth
            | Row::QuakeTopGap
            | Row::QuakeDismiss
            | Row::QuakeRestore
            // Not advanced, and it is on a page whose Advanced group it is not
            // in — but it is named here for this arm's own rule: a row swept
            // into a `_` is a row that silently starts resetting the day
            // somebody moves it, and this is the one row in the dialog whose
            // reset would switch a network request back on.
            | Row::AutoCheck
            | Row::Scrollback
            | Row::LineWrapping
            | Row::CopyOnSelect
            | Row::MultilinePaste
            | Row::Notifications
            | Row::TurnEndNotifications
            | Row::ShellIntegration
            // And doubly never again, for `ContextMenu`'s reason over a different store: what a
            // reset would be putting back is not a value in this file, it is a block in the user's
            // own `~/.claude/settings.json`.
            | Row::ClaudeHooks
            | Row::CodexNotify
            | Row::CopilotHooks
            // The editor's own advanced rows are put back by the page's own foot
            // verb — `Restore all defaults` on a built-in — which restores the
            // whole profile rather than four of its fields. A second verb that
            // reset a subset would be two answers to "put this back".
            | Row::ProfileName
            | Row::ProfileProgram
            | Row::ProfileLogin
            | Row::ProfileStartAt
            | Row::ProfileColour
            | Row::ProfileArgs
            | Row::ProfileEnv
            | Row::ProfileHyperlink
            | Row::ProfileIntegration
            // And the five that hold no value at all (T-SETTINGS-ABOUT). The
            // About page has no Advanced group to be handed out of, and that is
            // the smaller half: what its rows say — which build this is, which
            // machine it was made for, and three addresses — is not a
            // preference, so there is no default for `Reset to defaults` to put
            // back. Named rather than swept into a `_`, on this arm's own rule.
            | Row::AboutVersion
            | Row::AboutPlatform
            | Row::AboutIssues
            | Row::AboutLicences
            // And the three configuration doors (0.4.4 ticket 05), which hold
            // no value either: they carry every other row's.
            | Row::ExportSettings
            | Row::ImportSettings
            | Row::SettingsFolder
            | Row::PowerShellProfileLine
            | Row::LegacyPsReadLine
            // And the uninstaller's rows (T-UNINSTALL-UX): a door, a switch that
            // lives for one visit, and a fact about who installed this copy.
            | Row::Uninstall
            | Row::UninstallData
            | Row::UninstallBy => {}
        }
        Ok(())
    }

    /// **One paste into the focused settings field**, written through to the
    /// table as a keystroke's edit is.
    ///
    /// A path is the one value in this dialog nobody types out, and the
    /// clipboard is where it comes from. Newlines are dropped rather than turned
    /// into a second line: the field holds one.
    pub(crate) fn paste_into_settings_field(&mut self, text: &str) -> Result<()> {
        let text = text.replace(['\r', '\n'], "");
        let Some(target) = self.window.settings.focus() else {
            return Ok(());
        };
        let Some(field) = self.window.settings.text_field_mut(target) else {
            return Ok(());
        };
        if !text.is_empty() {
            field.insert(&text);
            self.write_editor_field(target)?;
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// **Everything the Profiles page's editor does to the table**, whichever
    /// door it came through — a press or an `Enter` on the ring.
    ///
    /// One function for both, which is `apply_settings_choice`'s founding rule:
    /// a verb reachable two ways whose body lives on one of them is a verb that
    /// half works. Every arm that changes the table goes through
    /// [`Self::store_profiles`], the one door the file, the programs probe and
    /// the window's measurements move through together.
    fn apply_editor_choice(&mut self, target: settings::SettingsTarget) -> Result<()> {
        let editor_index = self.window.settings.editor().map(|editor| editor.index);
        match target {
            settings::SettingsTarget::ProfileEdit(index) => {
                return self.open_profile_editor(index);
            }
            settings::SettingsTarget::ProfileMoreItem(index, verb) => {
                return self.apply_row_verb(index, verb);
            }
            // **New copies the default rather than opening a blank one**, and
            // then opens on it with the name selected: a profile with no program
            // is a row that cannot start, and the first thing anybody does with a
            // new one is call it something.
            settings::SettingsTarget::ProfileNew => {
                // A copy of the default, and so not before the default is decided: the press
                // does nothing during the few milliseconds the machine has not answered.
                let Some(made) = self.default_profile().and_then(profiles::create) else {
                    return Ok(());
                };
                self.store_profiles()?;
                return self.open_profile_editor(made);
            }
            settings::SettingsTarget::EditorBack => {
                self.window.settings.close_editor();
                self.window.settings_scroll = 0.0;
                return Ok(());
            }
            settings::SettingsTarget::EditorBrowse => {
                self.browse_for_program();
                return Ok(());
            }
            settings::SettingsTarget::EnvAdd => {
                if let Some(editor) = self.window.settings.editor_mut() {
                    editor.env.push(Default::default());
                    // The caret goes into the row that just appeared, which is
                    // what somebody who pressed `Add` is about to type in.
                    let index = editor.env.len() - 1;
                    self.window
                        .settings
                        .press(settings::SettingsTarget::EnvName(index));
                }
                // Nothing reaches the table: an empty pair is not a variable, and
                // it becomes one the moment it is named.
                return Ok(());
            }
            // **Adopting a ghost** (§7.1.6c-6c): the line the terminal was going
            // to fill in becomes one of the reader's own, carrying the name and
            // the value it was showing, and the ghost stops being drawn because
            // the terminal no longer gets to say that name. The caret lands in
            // the value box, which is the box somebody who adopted a row came to
            // change — the name is already right, which is why they pressed
            // *that* row rather than `Add`.
            settings::SettingsTarget::EnvGhost(ordinal) => {
                let Some(index) = editor_index else {
                    return Ok(());
                };
                let Some((name, value)) = self
                    .editor_subject()
                    .and_then(|subject| subject.ghosts().nth(ordinal))
                    .and_then(|ghost| Some((ghost.name, ghost.adoptable()?)))
                else {
                    return Ok(());
                };
                let mut env = profiles::env(index);
                env.push((name.to_owned(), value.to_owned()));
                profiles::set_env(index, env);
                self.store_profiles()?;
                self.reseed_editor_env(index)?;
                let adopted = profiles::env(index).len().saturating_sub(1);
                self.window
                    .settings
                    .press(settings::SettingsTarget::EnvValue(adopted));
                return Ok(());
            }
            settings::SettingsTarget::EnvRemove(row) => {
                let Some(index) = editor_index else {
                    return Ok(());
                };
                let Some(editor) = self.window.settings.editor_mut() else {
                    return Ok(());
                };
                if row >= editor.env.len() {
                    return Ok(());
                }
                editor.env.remove(row);
                let env = editor.env_pairs();
                profiles::set_env(index, env);
                return self.store_profiles();
            }
            // **Put every field of this built-in back to the table this build
            // ships** — its position and its hidden flag excepted, because both
            // are decisions about the *list* rather than about this profile.
            settings::SettingsTarget::EditorRestore => {
                let Some(index) = editor_index else {
                    return Ok(());
                };
                if !profiles::restore_defaults(index) {
                    return Ok(());
                }
                self.store_profiles()?;
                return self.open_profile_editor(index);
            }
            settings::SettingsTarget::EditorDelete => {
                let Some(index) = editor_index else {
                    return Ok(());
                };
                return self.delete_profile(index);
            }
            _ => {}
        }
        let Some(index) = editor_index else {
            return Ok(());
        };
        if let settings::SettingsTarget::Choice(row, item) = target {
            match row {
                settings::SettingsRow::ProfileStartAt => {
                    match settings::START_AT_OPTIONS.get(item).copied() {
                        Some(settings::StartAtChoice::Inherit) => {
                            profiles::set_start_at(index, profiles::StartAt::Inherit);
                        }
                        Some(settings::StartAtChoice::Home) => {
                            profiles::set_start_at(index, profiles::StartAt::Home);
                        }
                        // A verb and not a value: nothing is stored until the
                        // chooser comes back, and it may come back with nothing.
                        Some(settings::StartAtChoice::Choose) => {
                            self.browse_for_start_folder();
                            return Ok(());
                        }
                        None => return Ok(()),
                    }
                }
                // **Whether the program starts as a login shell** (0.4.6 ticket 74).
                settings::SettingsRow::ProfileLogin => {
                    let Some(login) = settings::FORMULA_OPTIONS.get(item).copied() else {
                        return Ok(());
                    };
                    if !profiles::set_login(index, login) {
                        return Ok(());
                    }
                }
                settings::SettingsRow::ProfileColour => {
                    let Some(colour) = marks::MarkColour::ALL.get(item).copied() else {
                        return Ok(());
                    };
                    if !profiles::set_colour(index, colour) {
                        return Ok(());
                    }
                }
                // **The same storage read twice, not a second field**: `Auto` is
                // this profile saying nothing about `FORCE_HYPERLINK`, so the
                // terminal's own declaration stands; `On` and `Off` are a row in
                // this profile's environment with that name.
                settings::SettingsRow::ProfileHyperlink => {
                    let Some(answer) = settings::HYPERLINK_OPTIONS.get(item).copied() else {
                        return Ok(());
                    };
                    let mut env = profiles::env(index);
                    env.retain(|(name, _)| name != settings::FORCE_HYPERLINK_NAME);
                    match answer {
                        settings::ForceHyperlink::Auto => {}
                        settings::ForceHyperlink::On => {
                            env.push((settings::FORCE_HYPERLINK_NAME.to_owned(), "1".to_owned()))
                        }
                        settings::ForceHyperlink::Off => {
                            env.push((settings::FORCE_HYPERLINK_NAME.to_owned(), "0".to_owned()))
                        }
                    }
                    profiles::set_env(index, env);
                    // The table under the row changed too, so the editor's own
                    // rows are re-seeded from it rather than left describing what
                    // the environment used to hold.
                    self.store_profiles()?;
                    return self.reseed_editor_env(index);
                }
                // **Which door serves this profile** (§7.1.6c-6c). Changing it
                // re-derives three things at once and in one place: what the
                // spawn injects (`shell_integration::shell_command`), which
                // spelling of a path the profile speaks (`set_integration`
                // carries the namespace with it), and the capability sentence
                // under the picker — all read back off the table on the next
                // frame rather than cached here.
                settings::SettingsRow::ProfileIntegration => {
                    let Some(choice) = settings::INTEGRATION_OPTIONS.get(item).copied() else {
                        return Ok(());
                    };
                    if !profiles::set_integration(index, choice) {
                        return Ok(());
                    }
                    // The ghost list is derived from the door — a PowerShell is
                    // not told `FORCE_HYPERLINK` by this terminal — so the table
                    // under the row has changed shape even though its contents
                    // have not.
                    self.store_profiles()?;
                    return self.reseed_editor_env(index);
                }
                _ => return Ok(()),
            }
            return self.store_profiles();
        }
        Ok(())
    }

    /// Re-read the environment table's rows out of the profile.
    ///
    /// One place, because two rows write that table — the table itself and the
    /// `Force hyperlinks` picker above it — and a picker that changed the
    /// storage without the table noticing would leave two pictures of one fact.
    fn reseed_editor_env(&mut self, index: usize) -> Result<()> {
        let env = profiles::env(index);
        if let Some(editor) = self.window.settings.editor_mut() {
            editor.env = env
                .into_iter()
                .map(|(name, value)| {
                    (
                        text_field::TextField::holding(&name),
                        text_field::TextField::holding(&value),
                    )
                })
                .collect();
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// One verb from a row's `⋯`.
    ///
    /// Every one of them writes the table and then goes through
    /// [`Self::store_profiles`], which is the single door the file, the programs
    /// probe and the window's own measurements move through.
    fn apply_row_verb(&mut self, index: usize, verb: settings::RowVerb) -> Result<()> {
        self.window.settings.close_row_menu();
        match verb {
            settings::RowVerb::Duplicate => {
                if profiles::duplicate(index).is_none() {
                    return Ok(());
                }
            }
            settings::RowVerb::Hide => {
                let hidden = profiles::hidden(index);
                let defaults = profiles::possible_defaults(
                    &self.app.settings_store.loaded().default_profile,
                    &self.app.profile_programs,
                );
                if !profiles::set_hidden(index, !hidden, &defaults) {
                    return Ok(());
                }
            }
            settings::RowVerb::Delete => return self.delete_profile(index),
            // The default is still *chosen* in one place — the picker on the
            // General page — and this is that same choice made from the table it
            // is a fact about (user ruling 2026-08-17, Q4). It goes through that
            // row's own applier, so the two doors cannot mean two things.
            settings::RowVerb::SetDefault => {
                let id = profiles::id(index);
                self.apply_default_profile(&id)?;
                return Ok(());
            }
            // The row's button, behind the `⋯`: the same press, through the same door.
            settings::RowVerb::EnableViaProfile => {
                self.enable_via_profile(index);
                return Ok(());
            }
            settings::RowVerb::CopyPolicyCommand => return self.copy_policy_command(),
        }
        self.store_profiles()
    }

    /// `Browse…` beside the program field: pick an executable, and point the
    /// profile at it.
    ///
    /// The same door the background picture's `Choose…` goes through
    /// (the window's `image_picker`, asked here for `FilePickKind::Program`), and it may come back
    /// with nothing — in which case nothing is written, because a chooser that was cancelled has
    /// not said anything.
    fn browse_for_program(&mut self) {
        let start = self
            .window
            .settings
            .editor()
            .map(|editor| PathBuf::from(editor.program.text()))
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .filter(|parent| parent.as_os_str().len() > 1);
        match self.window.image_picker.request(
            bt_platform::FilePickKind::Program,
            start.as_deref(),
            crate::i18n::Text::ImageFileType.text(),
        ) {
            Ok(true) => self.window.image_pick_pending = Some(FilePick::ProfileProgram),
            // Already queued or already open: a second press while one is up is
            // one dialog, not two, and whoever asked first keeps the answer.
            Ok(false) => {}
            Err(error) => crate::diagnostics::stderr_line(format!(
                "recoverable program chooser failure: {error}"
            )),
        }
    }

    /// Push whichever of the editor's fields has just been typed into back to
    /// the table.
    ///
    /// **On every change, which is this dialog's own rule** (§7.1.6c-4a): there
    /// is no commit and nothing to save. The one write that can be refused is the
    /// name — an exact duplicate of another row's — and the refusal is a state
    /// the reader is standing in rather than an error they submitted, so the
    /// field keeps what they typed and the row says why it is not the table's
    /// answer yet.
    pub(crate) fn write_editor_field(&mut self, target: settings::SettingsTarget) -> Result<()> {
        // **The one field on this dialog that is not the editor's** (§7.54e ⑤), and
        // it is answered before the editor is asked for: there is no profile under
        // the summoned terminal's page, and a guard that demanded one would make
        // this row a field that quietly stores nothing.
        if target == settings::SettingsTarget::Field(settings::SettingsRow::QuakeCommand) {
            let command = self.window.settings.quake_command().to_owned();
            self.apply_quake_command(command)?;
            if self.refresh_chrome() {
                self.present_chrome_change()?;
            }
            return Ok(());
        }
        let Some(editor) = self.window.settings.editor() else {
            return Ok(());
        };
        let index = editor.index;
        match target {
            settings::SettingsTarget::Field(settings::SettingsRow::ProfileName) => {
                let verdict = profiles::rename(index, editor.name.text());
                if let Some(editor) = self.window.settings.editor_mut() {
                    editor.refusal = match verdict {
                        profiles::NameVerdict::Written => None,
                        profiles::NameVerdict::Blank => Some(i18n::Text::ProfilesNameBlank),
                        profiles::NameVerdict::Taken => Some(i18n::Text::ProfilesNameTaken),
                    };
                }
                if verdict != profiles::NameVerdict::Written {
                    // Nothing reached the table, so nothing is owed the file —
                    // but the row's own sentence changed, so the frame is.
                    if self.refresh_chrome() {
                        self.present_chrome_change()?;
                    }
                    return Ok(());
                }
            }
            // **A program and never a command line** (0.4.6 ticket 74, issue #12):
            // `/bin/zsh -l` typed here used to become a program of that name, on no
            // machine, and the row read `not installed`. Such a text is refused with a
            // sentence and the row keeps its program; the argument has a row of its
            // own and a login shell a switch of its own.
            settings::SettingsTarget::Field(settings::SettingsRow::ProfileProgram) => {
                let verdict = profiles::program_field_verdict(
                    editor.program.text(),
                    &bt_pty::SystemShellEnvironment,
                );
                let refusal = (verdict == profiles::ProgramVerdict::CarriesArguments)
                    .then_some(i18n::Text::ProfilesProgramHasArguments);
                if let Some(editor) = self.window.settings.editor_mut() {
                    editor.program_refusal = refusal;
                }
                let profiles::ProgramVerdict::Program(path) = verdict else {
                    // Nothing reached the table, but the row's sentence may have
                    // changed, so the frame is owed.
                    if self.refresh_chrome() {
                        self.present_chrome_change()?;
                    }
                    return Ok(());
                };
                profiles::set_program_path(index, &path);
            }
            settings::SettingsTarget::Field(settings::SettingsRow::ProfileArgs) => {
                let args = profiles::split_arguments(editor.args.text());
                profiles::set_args(index, args);
            }
            settings::SettingsTarget::EnvName(_) | settings::SettingsTarget::EnvValue(_) => {
                let env = editor.env_pairs();
                profiles::set_env(index, env);
            }
            _ => return Ok(()),
        }
        self.store_profiles()
    }

    /// Route a press that landed on the modal overlay. Every press is consumed:
    /// that is what the scrim is for.
    pub(crate) fn settings_mouse_input(
        &mut self,
        layout: &settings::SettingsLayout,
        state: ElementState,
        button: MouseButton,
        position: PhysicalPosition<f64>,
    ) -> Result<()> {
        if state != ElementState::Pressed || button != MouseButton::Left {
            return Ok(());
        }
        // **The open picker's bar is asked before anything else**, which is the
        // glance card's own precedence (`file_peek::press_at`): a thumb lies over
        // the last pixels of the item beside it, and asking the item first would
        // make the bar undraggable exactly where it is drawn. It is deliberately
        // not a `SettingsTarget` — the ring never goes to a scrollbar, and a
        // focus stop for one would be a Tab stop nobody wants.
        if let Some(bar) = layout.menu_bar()
            && contains_point(bar.grab, position)
        {
            let held = (position.y as f32 - bar.thumb[1]).clamp(0.0, bar.thumb[3] - bar.thumb[1]);
            self.latch_settings_menu_bar_drag(held);
            self.drag_settings_menu_bar(&bar, position)?;
            return Ok(());
        }
        let target = settings::hit(layout, &self.settings_values(), position.x, position.y);
        // **A press outside the popper that is up takes it down, and then goes
        // on being the press it was** (user report and ruling, 2026-08-30,
        // overturning 08-29's "and goes no further") — the window's own menu
        // rule, asked here because this dialog is a modal with a dispatch of its
        // own and nothing below `settings_mouse_input` will ever see this press.
        //
        // **Before everything**: before the focus moves, before the scrim
        // answers, before a `Nav` word turns the page. That order is what lets
        // the list be gone by the time the verb under the press runs — a press
        // on the `×` with `Language` open closes both, in one press, which is
        // the report this ruling came from.
        //
        // **Ground is the exception, and it is the whole exception**: the
        // dialog's own blank and the scrim take the list down and stop there, so
        // pressing the scrim while a list is up leaves the dialog standing —
        // one layer per gesture, exactly as the first `Esc` does.
        //
        // **The judgement is `settings::popup_press`'s and the geometry is
        // `settings::hit`'s**: the popup was hit-tested before the page one line
        // above, so which target came back already says whether the pointer was
        // inside it. Nothing here measures a rectangle, which is the whole reason
        // this is one question rather than a second copy of the popup's frame.
        match settings::popup_press(self.window.settings.popup_up(), target) {
            settings::PopupPress::Through => {}
            settings::PopupPress::DismissAndLand(popup) => {
                self.window.settings.close_popup(popup);
            }
            settings::PopupPress::DismissOnly(popup) => {
                self.window.settings.close_popup(popup);
                if let Some(position) = self.window.pointer_position {
                    let hover = self.settings_layout().map(|layout| {
                        settings::hit(&layout, &self.settings_values(), position.x, position.y)
                    });
                    self.window.settings.set_hover(hover);
                }
                if self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
                return Ok(());
            }
        }
        // The focus follows the finger, with the ring off — the pointer half of
        // `:focus-visible`. Stated before the verb below, because closing the
        // dialog drops the focus, and a press that set it afterwards would leave
        // a shut dialog remembering one.
        // **The dialog's half of the press is the panel's** (`SettingsPanel::press_verb`), in the
        // vocabulary `Enter` answers in, so the two roads are one model (F-SWEEP-048): the focus
        // follows the finger with the ring off — stated before the verb, because closing the
        // dialog drops the focus — and what comes back is what only the window can do.
        let (rows, shortcuts, profile_lines, scheme_files, values) = self.settings_content();
        let content =
            self.settings_dialog(&rows, &shortcuts, &profile_lines, &scheme_files, &values);
        let verdict = self.window.settings.press_verb(target, content);
        match (target, verdict) {
            // The list opens showing the answer it already has, whatever page of it that answer
            // is on. Asked after the open, because only then is there a menu whose body can say
            // where the item is.
            (settings::SettingsTarget::Combo(row), _) => self.show_open_settings_choice(row),
            // A press on a track is a jump to the pointer AND the first frame of a drag — one
            // gesture, so one door (`SettingsLayout::slider_at`).
            (settings::SettingsTarget::Slider(row), _) => {
                if let Some(value) = layout.slider_at(row, position.x) {
                    self.apply_slider(row, value)?;
                }
                self.latch_settings_slider_drag(row);
            }
            // Turning a page puts the reader at the top of it.
            (settings::SettingsTarget::Nav(_), settings::SettingsKeyVerdict::Moved) => {
                self.window.settings_scroll = 0.0;
            }
            (
                _,
                settings::SettingsKeyVerdict::Chose(
                    target @ (settings::SettingsTarget::RestoreRow(_)
                    | settings::SettingsTarget::RestoreAll),
                ),
            ) => self.apply_shortcut_edit(target)?,
            // Every verb that leaves the dialog goes through the door `Enter` on it goes through:
            // a verb reachable two ways whose body lives on one of them is a verb that half works.
            (_, settings::SettingsKeyVerdict::Chose(target)) => {
                self.apply_settings_choice(target)?
            }
            // The sheet's own close press shut it.
            (_, settings::SettingsKeyVerdict::Closed) => self.settings_closed(),
            _ => {}
        }
        if let Some(position) = self.window.pointer_position {
            let hover = self.settings_layout().map(|layout| {
                settings::hit(&layout, &self.settings_values(), position.x, position.y)
            });
            self.window.settings.set_hover(hover);
            if !self.window.settings.is_open() {
                self.update_chrome_hover(position)?;
            }
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Commit every theme-dependent surface at one event-loop safe point. Until the resulting frame
    /// presents, DWM retains the previous complete back buffer; the renderer never submits a frame
    /// with only one side of this transaction applied.
    pub(crate) fn apply_theme_mode(&mut self, mode: ThemeModeV1) -> Result<bool> {
        // **`settings.json` and nowhere else** (§7.46 ②). This used to mark the
        // *session* dirty, which is how the field the Settings page draws came to
        // be one nothing wrote and nothing read.
        let mut settings = self.app.settings_store.loaded().clone();
        settings.theme_mode = mode;
        let mode_changed = self.app.settings_store.store(settings);
        let theme_changed = self.apply_theme(super::theme_in_force(mode))?;
        if mode_changed && !theme_changed && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(mode_changed || theme_changed)
    }

    pub(crate) fn apply_theme(&mut self, theme: Theme) -> Result<bool> {
        match set_theme(theme) {
            ThemeChange::LockedByEnvironment => {
                crate::diagnostics::stderr_line(format!(
                    "BT_THEME switch_ignored={theme:?} reason=BT_BG runtime_theme_locked=true"
                ));
                Ok(false)
            }
            ThemeChange::Unchanged => Ok(false),
            ThemeChange::Changed => {
                self.adopt_new_palette()?;
                Ok(true)
            }
        }
    }

    /// Point the "Maximum height" row at `height` logical pixels, `0` being no
    /// cap at all (2026-08-18).
    ///
    /// Presentation, so it is [`Self::apply_tables`] exactly: every pane in every
    /// tab, an immediate write, and one frame's cost. The clamp is read where the
    /// frame is decorated rather than where a block is proven — `decorate_math_frame`
    /// asks `block_max_height_px` afresh every frame — so nothing is re-scanned,
    /// no raster is thrown away, and a block that was scrolled inside itself is
    /// still scrolled to the same place when the cap is lifted.
    pub(crate) fn apply_block_max_height(&mut self, height: u32) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.block_max_height = height;
        if !self.app.settings_store.store(settings) {
            return Ok(false);
        }
        for tab in &mut self.window.tabs {
            for (_, leaf) in tab.leaves_mut() {
                let mut options = leaf.session.math_layout_options();
                options.block_max_height_px = block_max_height_px(height);
                leaf.session.set_math_layout_options(options);
            }
        }
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        Ok(true)
    }

    /// Point the "Repair row breaks" switch at `enabled`.
    ///
    /// [`Self::apply_inline_formulas`]'s shape and its cost: this one gates detection too, and it
    /// re-scans for a stronger reason than that one has. A block proven while the repair was on
    /// holds a `render_source` carrying a separator the terminal never printed, so a session that
    /// kept it would be showing a reader the repair they have just asked Folio to stop making.
    /// The re-scan is asked for by installing the options — `set_math_layout_options` raises the
    /// detection revision exactly when the detector's own answers change — rather than by a second
    /// invalidation written here, which would be a second place to remember.
    ///
    /// Only this row's own field is replaced. The rest is read back off the pane so that
    /// this row cannot quietly reset a `Maximum height` somebody set, for the reason
    /// `math_layout_options` exists at all.
    pub(crate) fn apply_repair_row_breaks(&mut self, enabled: bool) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.repair_row_breaks = enabled;
        if !self.app.settings_store.store(settings) {
            return Ok(false);
        }
        for tab in &mut self.window.tabs {
            for (_, leaf) in tab.leaves_mut() {
                let mut options = leaf.session.math_layout_options();
                options.restore_stripped_environment_newlines = enabled;
                leaf.session.set_math_layout_options(options);
            }
        }
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        Ok(true)
    }

    /// **What a second start of Folio opens** (§7.59, user ruling 2026-09-11).
    ///
    /// One write and nothing else, which is [`Self::apply_search_engine`]'s shape and for its
    /// reason: the row is read at the moment a launch lands (`launch_wire::landing`), so there is
    /// no cached copy to push it into and nothing on the glass changes until somebody starts Folio
    /// again.
    pub(crate) fn apply_launch_opens(&mut self, opens: bt_persist::LaunchOpensV1) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.launch_opens = opens;
        Ok(self.app.settings_store.store(settings))
    }

    pub(crate) fn apply_minimum_contrast(
        &mut self,
        floor: bt_persist::MinimumContrastV1,
    ) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.minimum_contrast = floor;
        if !self.app.settings_store.store(settings) {
            return Ok(false);
        }
        install_minimum_contrast(floor);
        self.adopt_new_palette()?;
        Ok(true)
    }

    /// **Point the `Web pages` row at `scheme`** (0.4.4 ticket 09).
    ///
    /// [`Self::apply_minimum_contrast`]'s shape and its door: [`Self::adopt_new_palette`] is the
    /// one place every window already tells its pages their colour scheme, and it records the
    /// change for every other window too — so this row reaches every page in every window through
    /// the same call a theme flip does, rather than through a second walk of its own.
    pub(crate) fn apply_web_color_scheme(
        &mut self,
        scheme: bt_persist::WebColorSchemeV1,
    ) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.web_color_scheme = scheme;
        if !self.app.settings_store.store(settings) {
            return Ok(false);
        }
        self.adopt_new_palette()?;
        Ok(true)
    }

    /// Windows' own blur behind the ground.
    ///
    /// Applied to the live window before it is stored, and **not** stored when
    /// DWM refuses: a row that remembered On while the window wore nothing would
    /// be a switch whose position is a claim about a thing that did not happen.
    pub(crate) fn apply_acrylic(&mut self, enabled: bool) -> Result<bool> {
        if !self.app.acrylic_available {
            return Ok(false);
        }
        if let Err(error) = self.window.window.set_backdrop(enabled) {
            crate::diagnostics::stderr_line(format!(
                "recoverable system backdrop failure: {error}"
            ));
            return Ok(false);
        }
        let mut settings = self.app.settings_store.loaded().clone();
        settings.acrylic = enabled;
        if &settings == self.app.settings_store.loaded() {
            return Ok(false);
        }
        Ok(self.app.settings_store.store(settings))
    }

    /// Whether the window stays above other windows.
    ///
    /// Immediate, persisted, and re-applied at startup — the three halves of
    /// "this is a posture and not a gesture". `SetWindowPos` here never
    /// activates: switching the row on while another window has the keyboard
    /// must not steal it.
    pub(crate) fn apply_always_on_top(&mut self, enabled: bool) -> Result<bool> {
        // **The one window the row does not reach** (§7.54). The preference is
        // still written — it is the application's, and every other window of this
        // process wears it — but the summoned terminal's own posture is not a
        // preference: a reader who turned the row off from inside the window a key
        // calls up would have turned off the thing that makes it visible when it
        // arrives. Said here rather than by greying the row, because the row is
        // true of the program and this is one window declining to be its example.
        if !self.is_quake_window()
            && let Err(error) = self.window.window.set_topmost(enabled)
        {
            crate::diagnostics::stderr_line(format!("recoverable always-on-top failure: {error}"));
            return Ok(false);
        }
        let mut settings = self.app.settings_store.loaded().clone();
        settings.always_on_top = enabled;
        if &settings == self.app.settings_store.loaded() {
            return Ok(false);
        }
        Ok(self.app.settings_store.store(settings))
    }

    /// **`Install fonts…`: open the system's own Fonts page** (user ruling
    /// 2026-08-19).
    ///
    /// The list a font picker draws is every monospaced family DirectWrite
    /// reports, which makes the honest answer to "my font isn't here" *install
    /// it* — and this row is the product saying so instead of ending the list in
    /// silence.
    ///
    /// **The dialog stays open**, unlike the two scheme verbs beside it. What
    /// this opens is another program's window, not a document of ours, and a
    /// reader who installs a face is coming straight back to this picker to
    /// choose it. `settings::begin_monospace_scan` runs at the next open, so the
    /// round trip is: press, install, close, reopen, choose — and since the walk
    /// it starts is a worker's, the reopen costs nothing and the new family
    /// appears in the list a moment later, on the frame `AppEvent::FontsScanned`
    /// asks for.
    ///
    /// A refusal is a card and not a silence, because the reader is looking at
    /// this window and the answer arrived somewhere else: nothing appearing at
    /// all is indistinguishable from a page that opened behind us.
    ///
    /// **Handed to the OS hand-off lane** (2026-09-22), so the card arrives when the lane answers
    /// rather than while the press is still being handled — in the words of the refusal's kind
    /// (T-HANDOFF-DOOR-REASON: "no program to open it" only when there is none).
    fn open_font_settings(&mut self) -> Result<()> {
        self.hand_off(
            handoff_lane::Ask::new(
                bt_platform::Handoff::FontsPage,
                handoff_lane::Refusal {
                    // The card says the refusal's kind; its own words are this line's.
                    stderr: Some((
                        "recoverable font settings hand-off failure",
                        "open the system font settings",
                    )),
                    program_notice: false,
                },
            )
            .if_refused(handoff_lane::OnRefused::FontsToast),
        )
    }

    /// **The pencil on a picker row: open that scheme's file** (user ruling
    /// 2026-08-19).
    ///
    /// No copy, because the row it is drawn on already IS the reader's file —
    /// which is the whole content of the 2026-08-18 `Edit scheme…` report, now
    /// said by the mark being ABSENT from every bundled row instead of by a verb
    /// changing its word.
    ///
    /// **It does not change which scheme is in force.** Opening a file to read
    /// it is not choosing it, and a picker whose pencil silently selected the
    /// row it was pressed on would be a picker you cannot look inside.
    ///
    /// The name is read out of the catalogue at the press, on
    /// [`Self::delete_scheme_at`]'s own terms and for the same reason: the list
    /// was drawn a frame ago and the folder is somebody else's to change.
    fn edit_scheme_at(&mut self, row: settings::SettingsRow, index: usize) -> Result<()> {
        let Some(light) = row.scheme_canvas() else {
            return Ok(());
        };
        let Some(name) = settings::scheme_labels(light).get(index).copied() else {
            return Ok(());
        };
        let catalogue = schemes::catalogue();
        let file = catalogue.user_file_of(name, light).map(str::to_owned);
        drop(catalogue);
        // A row that is not a file has no pencil drawn on it, so this is a
        // folder that moved between the draw and the press rather than a state
        // the picture can be in. Nothing happened; nothing is said.
        let Some(file) = file else {
            return Ok(());
        };
        let path = match schemes::user_dir() {
            Ok(directory) => directory.join(&file),
            Err(error) => {
                return self.toast(
                    toast::ToastKind::Error,
                    toast::ToastAnchor::Window,
                    Some(name.to_owned()),
                    i18n::not_saved(&error.to_string()),
                );
            }
        };
        self.open_scheme_for_editing(path)
    }

    /// **The × on a picker row: send that scheme's file to the Recycle Bin**
    /// (user ruling 2026-08-19).
    ///
    /// The row names the file, so this resolves the item's name against the
    /// catalogue and hands the answer to [`Self::delete_scheme_file`], whose
    /// four steps and one card are unchanged — what moved is where the press
    /// comes from, never what the press does.
    fn delete_scheme_at(&mut self, row: settings::SettingsRow, index: usize) -> Result<()> {
        let Some(light) = row.scheme_canvas() else {
            return Ok(());
        };
        let Some(name) = settings::scheme_labels(light).get(index).copied() else {
            return Ok(());
        };
        let catalogue = schemes::catalogue();
        let file = catalogue.user_file_of(name, light).map(str::to_owned);
        drop(catalogue);
        let Some(file) = file else {
            return Ok(());
        };
        self.delete_scheme_file(&file)
    }

    /// **Send one scheme file to the Recycle Bin, and fall back to the
    /// default** (§7.1.6c-4d, user ruling 2026-08-18).
    ///
    /// `Customise scheme…` could only ever make files. A folder that grows a
    /// copy every time somebody presses a button and never loses one is a
    /// picker that only gets longer, and the way out of it was Explorer — which
    /// is to say, not this window.
    ///
    /// Four steps, and the order is the one that survives each step failing:
    ///
    /// 1. the file goes to the **Recycle Bin** and not to `remove_file`. It is a
    ///    file the user wrote by hand, and a delete button in a settings dialog
    ///    that destroys one is a button nobody can afford to press. The card
    ///    says where it went, which is a fact and also the undo;
    /// 2. the row falls back to the **default** — the stored name is cleared,
    ///    not left dangling, because unlike a file that has wandered off there
    ///    is nothing to come back;
    /// 3. the catalogue is re-read **here**, before the watcher gets to it, so
    ///    the picker has already lost the entry by the next frame;
    /// 4. one card, naming the file and the scheme now in force.
    ///
    /// **One card and not two**, which is the whole of why step 3 is where it
    /// is. The folder's watcher will see this deletion and run
    /// `schemes::rescan_verdict`, whose standing answer to "the selected
    /// scheme's file is gone" is a fall to the default *and an Info card*. That
    /// rule is right for a file that left the folder behind this window's back
    /// and wrong for one this window just deleted — the press already knows.
    /// It is silenced by arithmetic rather than by a flag: the verdict is
    /// derived from the two **stored names**, step 2 has already made this one
    /// the default, and a default resolves to a bundled scheme that cannot go
    /// missing. Pinned by `a_deletion_this_window_made_raises_one_card_not_two`.
    fn delete_scheme_file(&mut self, file: &str) -> Result<()> {
        let file = file.to_owned();
        // **Which rows this file was answering for**, asked before it goes. A
        // scheme that is not in force needs no fallback at all — deleting it
        // changes nothing on screen — and one that is needs the default put in
        // its row, which is the rule this verb has always had, now asked per row
        // instead of assumed about the canvas in force.
        let in_force = [
            self.app.settings_store.loaded().light_scheme.clone(),
            self.app.settings_store.loaded().dark_scheme.clone(),
        ];
        let catalogue = schemes::catalogue();
        let falls: [bool; 2] = [true, false].map(|light| {
            let index = usize::from(!light);
            catalogue.user_file_of(&in_force[index], light) == Some(file.as_str())
        });
        drop(catalogue);
        let path = match schemes::user_dir() {
            Ok(directory) => directory.join(&file),
            Err(error) => {
                return self.toast(
                    toast::ToastKind::Error,
                    toast::ToastAnchor::Window,
                    Some(i18n::Text::SchemeDeleted.text().to_owned()),
                    i18n::not_deleted(&error.to_string()),
                );
            }
        };
        match bt_platform::recycle(&path) {
            // The shell asked, and the answer was no. Nothing happened and
            // nothing is said: "cancelled" is a card about a decision the reader
            // made half a second ago and is still looking at.
            Ok(false) => return Ok(()),
            Ok(true) => {}
            Err(error) => {
                return self.toast(
                    toast::ToastKind::Error,
                    toast::ToastAnchor::Window,
                    Some(i18n::Text::SchemeDeleted.text().to_owned()),
                    i18n::not_deleted(&error),
                );
            }
        }
        // Only the rows this file was actually answering for fall back, and a
        // row that was on something else is not touched: `apply_scheme` takes
        // `None` for "leave this canvas alone", which is what makes deleting a
        // scheme nobody is wearing a pure file operation.
        if falls[0] || falls[1] {
            self.apply_scheme(falls[0].then(String::new), falls[1].then(String::new))?;
        }
        // Ahead of the watcher, so the picker has lost the entry by the next
        // frame rather than by the next quiet window — and so the verdict the
        // watcher does eventually reach is about a folder that already matches
        // the settings.
        let after = schemes::rescan();
        // The canvas whose row moved is the one whose default the card names,
        // and a file nobody was wearing moves no row at all — so there is
        // nothing to name and the sentence says only that the file has gone.
        let fallback = (falls[0] || falls[1]).then(|| after.default_name(falls[0]).to_owned());
        drop(after);
        self.refresh_scheme_sources();
        self.toast(
            toast::ToastKind::Ok,
            toast::ToastAnchor::Window,
            Some(i18n::Text::SchemeDeleted.text().to_owned()),
            // **The file and not the scheme's name**: what is in the Recycle Bin
            // is spelled the way the file was, and that is the string somebody
            // going to fetch it back has to recognise.
            i18n::scheme_deleted(&file, fallback.as_deref()),
        )
    }

    /// Move Folio's verb to the place the row was set to (§7.4, §7.4a, user
    /// ruling 2026-09-07).
    ///
    /// **One press, two stores, and the second one is only touched when it
    /// disagrees.** The classic trees answer inside this call; the package takes
    /// seconds and answers on `AppEvent::ExplorerPackageChanged`. A place that
    /// leaves the deployment database where it already is starts no job at all —
    /// otherwise choosing `Under Show more options` on a machine that never had
    /// a package would spend a service call and raise a card about a removal
    /// nobody asked for.
    ///
    /// **The row is redrawn from the machine either way**, not from what was
    /// asked for: the cached answers are re-read after the write, so a refusal
    /// leaves the picker standing where the machine actually is rather than
    /// where the press hoped it would be. That is the whole reason this row
    /// stores nothing in `settings.json` — there is no second copy of the truth
    /// to fall out of step.
    ///
    /// **One card, and it names the place.** Where a deployment was started the
    /// card is owed by it and this call stays silent: two cards for one press
    /// would report the halves of an answer the reader gave once. A failure
    /// carries the operating system's own sentence, because on the machine where
    /// it fires nobody else can see it.
    fn apply_explorer_place(
        &mut self,
        place: explorer_menu::ExplorerPlace,
        announce: Announce,
    ) -> Result<bool> {
        // **A job in flight makes the cached answer unusable** (review row
        // R4-12). During an install the cached state still says nothing is
        // registered, so an `Off` pressed two seconds in compared equal, started
        // no job at all, and the install landed against the reader's own last
        // choice. While something is running, what the machine will be is not
        // what the row was drawn from, so the press is handed on and the module
        // decides — it holds the only thing that knows.
        let package_job = explorer_menu::job_in_flight()
            || match explorer_menu::state() {
                // **`Unknown` is not "nothing registered"** (`explorer_menu`'s own
                // rule for this state): the first probe of a launch has not landed,
                // and skipping the job here would leave a package standing under a
                // row that says it is gone. So the job runs and answers the question
                // properly — `request` re-reads the deployment database on its own
                // thread before it does anything, which is the whole reason it is
                // allowed to ask.
                explorer_menu::PackageState::Unknown => true,
                state => place.package() != state.registered(),
            };
        if package_job {
            self.request_explorer_package(place, announce);
        }
        let outcome = context_menu::apply(place.classic());
        self.app.context_menu_installed = context_menu::installed(context_menu::state());
        match outcome {
            Ok(()) if package_job || announce == Announce::OnlyFailures => Ok(true),
            Ok(()) => {
                self.toast(
                    toast::ToastKind::Ok,
                    toast::ToastAnchor::Window,
                    None,
                    explorer_menu::place_toast(place, explorer_menu::shell_refresh_pending())
                        .text()
                        .to_owned(),
                )?;
                Ok(true)
            }
            Err(error) => {
                self.toast(
                    toast::ToastKind::Error,
                    toast::ToastAnchor::Window,
                    None,
                    i18n::context_menu_failed(&error),
                )?;
                Ok(false)
            }
        }
    }

    /// Store the reader's answer about the update check (§7.51).
    ///
    /// **Nothing is started or stopped directly here.** The application clock
    /// owns the schedule: `On` lets its next turn start an overdue check, while
    /// `Off` removes the deadline and leaves an answer already on the wire
    /// alone. Cached evidence remains visible and manual Check keeps using the
    /// same worker door.
    ///
    /// The state file is left where it is on `Off`. Its cached evidence still
    /// answers Version and its mark, and a reader who wants it gone has
    /// `docs/PRIVACY.md`'s one line.
    pub(crate) fn apply_update_check(&mut self, enabled: bool) {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.update_check = enabled;
        self.app.settings_store.store(settings);
        // This is the schedule only. Manual Check, cached offers and a job
        // already in progress remain owned by their existing roads.
        update::set_enabled(enabled);
    }

    /// Point the `Focus card height` row at `height` logical pixels of card body
    /// (user ruling 2026-08-21, §7.1.6b′).
    ///
    /// **One write and no fan-out**, which is the shape the setting was given
    /// rather than a shortcut taken here: nothing in this process holds a card
    /// height. `rail_posture` reads the file every time it is asked what shape
    /// this window is, and every box that follows the body — the card, the mini
    /// cell, the entrance's start and end, the hit boxes, the scroll range — is
    /// derived from the column `focus_rail_geometry` solves out of that posture.
    /// So the row takes effect on the next frame, in **every** window, and there
    /// is nothing to invalidate; compare [`Self::apply_block_max_height`], which
    /// has to walk every pane because `bt_term` really does keep a copy.
    ///
    /// The frame is asked for anyway, because a setting that took effect on the
    /// next thing that happened to repaint would look like it had not worked.
    pub(crate) fn apply_focus_card_height(&mut self, height: u32) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.focus_card_height = height;
        if !self.app.settings_store.store(settings) {
            return Ok(false);
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// **What the Option key is**, stored and told to every window this process
    /// has (M1-7, macOS plan §8 Q9).
    ///
    /// Two things have to hear it and they are not the same thing, which is why
    /// this is not one line. winit decides at the **window** whether Option
    /// composes a character or reports a raw letter with Alt held — so every
    /// open window is told, not only the one the dialog is standing in — and
    /// this window's own *modifier state* is stale the instant the answer
    /// changes, because it was last written by a `ModifiersChanged` that applied
    /// the old policy. A reader who turned the row on while holding Option and
    /// then pressed `a` would otherwise get one more keystroke of the answer
    /// they had just left.
    ///
    /// The other windows learn it the way they learn every application setting —
    /// see `settle_application_change` — and what they learn is read from the
    /// store, so there is one answer and no copy of it anywhere.
    pub(crate) fn apply_option_sends_alt(&mut self, enabled: bool) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.option_sends_alt = enabled;
        if !self.app.settings_store.store(settings) {
            return Ok(false);
        }
        self.note_application_change(ApplicationChange {
            font: false,
            look: false,
            caret: false,
            option: true,
            paid_by: Some(self.window.window.id()),
        });
        self.adopt_option_as_alt();
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// **The one door behind every door** (§7.1.6b′: "one bit, one face").
    ///
    /// The `Appearance` row, `Ctrl+Shift+Z`, the `Exit` button and the `Esc` rung
    /// all arrive here, for [`Self::set_rail_state`]'s reason one feature over: a
    /// mode cannot mean two things depending on which door it came through, and
    /// the row in Appearance has to show what is true however the bit was last
    /// turned.
    ///
    /// It writes **both halves** of §2.4 rule three: the window's own posture,
    /// and the file that says what the next window opens as. That second write is
    /// what the ruling asks the row to buy — a window closed in focus mode
    /// reopens in it — and it is here rather than in the row's own handler
    /// because every door is the same decision.
    ///
    /// **There is nothing to restore and nothing to save.** The stage is the
    /// active tab's own tree, untouched on the way in; what changes is the width
    /// of the panel beside it, which the next solve reads out of
    /// [`seats::RailState::terminal_inset_logical_px`].
    pub(crate) fn set_focus_mode(&mut self, on: bool) -> Result<()> {
        if self.window.focus_mode == on {
            return Ok(());
        }
        self.window.focus_mode = on;
        // **The entrance, aimed at the bit that has already turned** (F3). Only
        // the picture is behind: `rail_posture()` answers the new posture on this
        // very line, the solve below is made against it, and the hit test has
        // been answering the card column since the assignment above. What the
        // tween buys is the 180ms in which the list is still arriving — and on
        // the way *out* it buys nothing, by design: `draws_focus_rail()` is
        // already false, so nothing reads it and the column is simply gone. The
        // stage owns that space the moment the bit does, and a departing card
        // drawn across it would be the 2026-08-19 occlusion wearing a tween.
        if on {
            self.window
                .focus_reveal
                .retarget(1.0, Instant::now(), self.app.motion);
        } else {
            // **Put down, not eased down.** A tween left running on the way out
            // would keep the loop awake for 180ms drawing nothing — nothing
            // reads it once `draws_focus_rail()` is false — which is a schedule
            // for an animation that does not exist. Said here rather than left
            // to fall out of the paint, so "the departing list is not animated"
            // is true of the wake-ups too and not only of the pixels.
            self.window.focus_reveal = RevealTween::resting(0.0, RAIL_TRANSITION);
        }
        // The scroll offset is the panel's, and the panel now holds a list of a
        // different length. Left where it was, a window that had scrolled a long
        // rail would enter focus mode looking at nothing; both geometries clamp,
        // but neither can invent the row you were on.
        self.window.rail_scroll = 0.0;
        // The other half of the posture, and therefore the other route into the
        // band — see [`Self::set_rail_state`]'s own line. The band itself stopped
        // turning on the layout with §13.48, and the door is still said here for
        // the reason it was always a door: whoever decides what this window is
        // wearing says so again, and a placement that is already right writes
        // nothing.
        self.follow_the_window_band()?;
        let mut settings = self.app.settings_store.loaded().clone();
        settings.focus_mode = on;
        let _ = self.app.settings_store.store(settings);
        // **The panel's width changed, and that is the only thing this mode does
        // to the stage.** The very path `set_rail_state` takes when `Sidebar`
        // moves the terminal's left edge, and taken here for the same reason: the
        // tree is untouched, every pane's rectangle is not, because the viewport
        // they are solved into begins the column's own width further in (or stops
        // doing so). There is no branch anywhere below this for what is being
        // drawn — only a different number for where it starts.
        crate::diagnostics::stderr_line(format!(
            "BT_FOCUS on={on} inset={}px",
            self.rail_posture().terminal_inset_logical_px()
        ));
        self.commit_seat_geometry()?;
        self.apply_window_min_inner_size()?;
        if let Some(position) = self.window.pointer_position {
            self.window.seat_pointer.hover = self.chrome_target_at(position);
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// **Open the settings dialog on one row.**
    ///
    /// Three bricks that were all already here — open, choose the page, scroll
    /// — and the thin wrapper is the whole of what is new. The order matters
    /// and is the reason this is a function rather than three calls at the call
    /// site: `SettingsPanel::toggle` always lands on the first page, so the
    /// page has to be chosen *after* it, and the scroll has to be computed
    /// against a layout built after both.
    pub(crate) fn open_settings_on_row(&mut self, row: settings::SettingsRow) -> Result<()> {
        if !self.window.settings.is_open() {
            self.toggle_settings_panel()?;
        }
        if self.window.settings.select_category(row.category()) {
            self.window.settings_scroll = 0.0;
        }
        let target = settings::SettingsTarget::Field(row);
        self.window.settings.press(target);
        if let Some(layout) = self.settings_layout() {
            self.window.settings_scroll =
                layout.scroll_to_show(target, self.window.settings_scroll);
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// **Bring a picker's own answer into view the moment it opens or moves**
    /// (§7.1.6c-5).
    ///
    /// Asked after the state has changed, because only then is there a layout
    /// whose menu body can say where the item landed — the same order
    /// `scroll_to_show` is asked in for the page. `menu_scroll_to_show` moves by
    /// the minimum, so a list already showing the item does not twitch.
    fn show_open_settings_choice(&mut self, row: settings::SettingsRow) {
        let index = row
            .selected_index(&self.settings_values())
            .unwrap_or_default();
        self.show_settings_choice_at(index);
    }

    /// The same, for an index the keyboard has just walked onto.
    pub(crate) fn show_settings_choice_at(&mut self, index: usize) {
        let Some(layout) = self.settings_layout() else {
            return;
        };
        let scrolled = layout.menu_scroll_to_show(index, self.window.settings.menu_scroll());
        self.window.settings.scroll_menu(scrolled);
    }

    /// A wheel notch anywhere over the settings modal.
    ///
    /// Anywhere, and not only over the content box: the dialog is modal, so the
    /// scrim is not a place where a notch could mean something else, and a
    /// pointer resting on the header while the wheel turns is asking about the
    /// only scroller on screen. A dialog whose content fits has `max_scroll ==
    /// 0`, and the clamp then makes this the same no-op the old early return was.
    pub(crate) fn scroll_settings(
        &mut self,
        layout: &settings::SettingsLayout,
        delta: MouseScrollDelta,
    ) -> Result<()> {
        // **An open picker takes the wheel from under the pointer** (§7.1.6c-5),
        // and only from under it: a notch over the dialog while a capped menu
        // hangs somewhere else is still the page's, which is the same rule the
        // rail and the tab strip already answer to — a scroller owns the notches
        // over its own box. The menu is the innermost box, so it is asked first,
        // which is `hit`'s own smallest-target-first ruling read for the wheel.
        if let Some(body) = layout.menu_body()
            && let Some(position) = self.window.pointer_position
            && contains_point(body, position)
        {
            let travel = self.vertical_wheel_travel(delta, body[3] - body[1]);
            let scrolled =
                (self.window.settings.menu_scroll() - travel).clamp(0.0, layout.menu_max_scroll());
            if !self.window.settings.scroll_menu(scrolled) {
                return Ok(());
            }
            let moved = self.settings_layout();
            self.window.settings.set_hover(moved.map(|layout| {
                settings::hit(&layout, &self.settings_values(), position.x, position.y)
            }));
            if self.refresh_chrome() {
                self.present_chrome_change()?;
            }
            return Ok(());
        }
        let content = layout.content_box();
        let travel = self.vertical_wheel_travel(delta, content[3] - content[1]);
        let scrolled = (self.window.settings_scroll - travel).clamp(0.0, layout.max_scroll());
        if scrolled == self.window.settings_scroll {
            return Ok(());
        }
        self.window.settings_scroll = scrolled;
        // The stack moved under a stationary pointer, so what it is over changed
        // without the pointer having done anything.
        if let Some(position) = self.window.pointer_position {
            let moved = self.settings_layout();
            self.window.settings.set_hover(moved.map(|layout| {
                settings::hit(&layout, &self.settings_values(), position.x, position.y)
            }));
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The settings dialog's placement in the window as it is now, or `None`
    /// when it is shut — or open but unhostable, which is the same thing to
    /// everyone downstream.
    ///
    /// Unhostable has to read as shut and not as "open but invisible": an
    /// invisible modal still swallows Esc and every click, and that is a window
    /// nobody can get out of.
    /// `&mut` for one reason: the open picker's width is a fact about glyphs, and
    /// only the font knows it. The measuring happens here, beside the renderer,
    /// exactly as the peek strip's and the restore prompt's do, and the geometry
    /// itself stays a pure function of the numbers handed to it.
    pub(crate) fn settings_layout(&mut self) -> Option<Arc<settings::SettingsLayout>> {
        if !self.window.settings.is_open() {
            self.window.settings_geometry.clear();
            return None;
        }
        // **The dialog's own lane** (GitHub issue #3 — see
        // [`hang_watch::Station::Settings`]). Every road that draws, hovers or
        // hit-tests this page comes through here, and until this slice the
        // milliseconds it spends were charged to whichever of `window_event` and
        // `publish_frame_inner` happened to enclose it — two true labels, neither
        // of which named the dialog to the reader of a slow-hold line.
        let trace_start = self.app.trace_perf.then(Instant::now);
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let scale = self.window.renderer.scale_factor() as f32;
        // Content is read from its owners. Geometry is computed only when one
        // of these inputs changes; hover, focus and redraw are readers of it.
        let (rows, shortcuts, profile_lines, scheme_files, values) = self.settings_content();
        let content =
            self.settings_dialog(&rows, &shortcuts, &profile_lines, &scheme_files, &values);
        let content_us = trace_start.map_or(0, |start| start.elapsed().as_micros());
        // **The agent rows' own edge**, and it is not a convenience: their switches are inert
        // while the file they name is one this build will not edit, so a press cannot be what
        // notices that the reader has unpicked the link or cleared the read-only bit. Opening the
        // page is (re-review, round 3). One re-read per visit, never per frame and never per turn.
        let agents_opened = self
            .window
            .settings
            .take_agents_open_edge(content.shows_agents(self.window.settings.category()));
        if agents_opened {
            self.refresh_agent_rows();
            // Which agents are on this machine is the program walk's answer: asked again here.
            self.ask_the_program_walk(programs_lane::Trigger::AgentsPage);
            // And which copilot it is: the page that prints the version asks for it at every
            // visit, a failure included (T-FRESH-FACTS).
            attention_copilot::reask_probe(attention_copilot::ProbeTrigger::AgentsPage);
        }
        let profiles_opened = self.window.settings.take_profiles_open_edge(
            self.window.settings.category() == settings::SettingsCategory::Profiles
                && content.editor.is_none(),
        );
        if profiles_opened {
            shell_integration::begin_profile_observation_for(&self.app.profile_programs);
            // The environment the next pane is born from, which the editor's rows are
            // composed over (`shell_integration::page_environment`).
            shell_integration::request_page_environment();
            // Every row's program, and WSL's distributions, asked again as the page opens.
            self.ask_the_program_walk(programs_lane::Trigger::ProfilesPage);
        }
        // Use the refreshed fact on this very layout, including its geometry.
        let refreshed_values = agents_opened.then(|| settings::SettingsValues {
            claude_hooks: self.app.claude_hooks_installed,
            codex_notify: self.app.codex_notify_installed,
            copilot_hooks: self.app.copilot_hooks_installed,
            agent_config_refusals: self.app.agent_config_refusals,
            ..values.clone()
        });
        let content = settings::SettingsContent {
            values: refreshed_values.as_ref().unwrap_or(&values),
            ..content
        };
        // The second probe on the same door and for the same argument: the page that prints which
        // copilot this machine has is the page that asks. Idempotent, and an atomic load after the
        // first call — see `attention_copilot::begin_probe`.
        if content.probes_copilot(self.window.settings.category()) {
            attention_copilot::begin_probe();
        }
        // **And the mark on the gear is answered on the same door** (§7.51),
        // for the two probes' argument read the other way round: this is the one
        // place that knows the reader is looking at the page the row is on, and
        // "looking at it" is the whole of what the mark was asking for. The row
        // is not behind a disclosure and it is not the twelfth of twelve, so a
        // reader on this page has been shown it.
        //
        // Idempotent, and a disk write on exactly one frame — see
        // `update::answer_mark`, which is also where the gear's own repaint is
        // argued about.
        //
        // **The row keeps the mark for the rest of this visit** (0.4.6
        // T-GEAR-MARK-LANDS): the panel is told the gear was lit before the
        // answer puts it out, so the update row wears the dot the gear led the
        // reader to until they leave the page — `seen_tag` stays the one answer.
        if self.window.settings.category() == settings::SettingsCategory::About {
            self.window
                .settings
                .carry_update_mark(update::gear_mark_is_lit(&self.app.update_job));
            update::answer_mark();
        }
        let inputs = settings::geometry::Inputs::new(
            [width as f32, height as f32],
            scale,
            self.window.renderer.font_revision(),
            &self.window.settings,
            self.window.settings_scroll,
            content,
        );
        // **Every picker's width is measured, not only the open one's**
        // (§7.1.6c-5): a button is as wide as its own longest option now, so the
        // geometry needs the font for every row on the page and not just for the
        // popup. Handed over as a closure for `build`'s own reason — the
        // renderer owns the face, and the geometry stays a pure function of what
        // it is told.
        let probes_us = trace_start.map_or(0, |start| {
            start.elapsed().as_micros().saturating_sub(content_us)
        });
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let laid = self.window.settings_geometry.read(inputs, |inputs| {
            let leaving_station = hang_watch::enter(hang_watch::Station::Settings);
            let mut measure_us = 0;
            let mut measure_calls = 0;
            let mut measure = |text: &str, size: f32| {
                let start = trace_start.map(|_| Instant::now());
                let width = renderer.measure_chrome_text(gpu, text, size);
                measure_us += start.map_or(0, |start| start.elapsed().as_micros());
                measure_calls += 1;
                width
            };
            let laid = inputs.layout(&mut measure);
            if let Some(start) = trace_start {
                let total_us = start.elapsed().as_micros();
                crate::diagnostics::stderr_line(format!(
                    "BT_PERF_TRACE settings_layout content_us={content_us} probes_us={probes_us} measure_calls={measure_calls} measure_us={measure_us} geometry_us={} total_us={total_us}",
                    total_us.saturating_sub(content_us + probes_us + measure_us)
                ));
            }
            hang_watch::at(leaving_station);
            laid
        });
        // **The keyboard never rests on a control that is not a stop** (047-EXPERIENCE). A stop
        // can stop being one while nobody presses anything — About's Version control is a stop
        // only while enabled, and it disables itself while a check or a download runs — and the
        // ring is drawn only on a stop, so a focus left behind there is a keyboard with no ring.
        // Asked on the one road every draw, hover and hit test comes through, with the placement
        // this layout made, and answered by the rule every press already ends with
        // ([`settings::SettingsPanel::keep_focus_reachable`]: the nearest stop beside it on the
        // page).
        if let Some(layout) = &laid {
            self.window
                .settings
                .keep_focus_on_a_stop(content, layout.placed_profile_buttons());
        }
        laid
    }

    /// The dialog's contents this frame, for the callers that need them beside a
    /// layout — the focus walk and the draw.
    ///
    /// It hands back the two owned lists as well, because a `SettingsContent`
    /// borrows them and a borrow of a temporary does not outlive the call that
    /// made it. Every caller therefore reads the *same* two lists it hands to
    /// the panel, which is what keeps the focus order and the boxes from being
    /// derived one frame apart.
    pub(crate) fn settings_content(
        &self,
    ) -> (
        Vec<settings::SettingsRow>,
        Vec<shortcuts::ShortcutRow>,
        Vec<profiles::ProfileLine>,
        Vec<settings::SchemeFileLine>,
        settings::SettingsValues,
    ) {
        (
            {
                let mut rows = settings::visible_rows(self.window.rail.layout);
                settings::retain_powershell_profile_remover(
                    &mut rows,
                    shell_integration::powershell_profile_line_present(),
                );
                settings::retain_legacy_psreadline_remover(
                    &mut rows,
                    psreadline::legacy_copy_present(),
                );
                rows
            },
            {
                // **The fourth refusal, on the row it is about** (§7.54). The
                // claim is refused outside this table — by Windows, to a chord
                // this table considers perfectly legal — so the fact is joined
                // to the rows here, where both halves are in hand, rather than
                // inside `editor_rows`, which knows only the table.
                let mut shortcuts = self.app.shortcuts.editor_rows();
                if self.app.quake.hotkey_taken() {
                    shortcuts::note_the_summon_is_taken(&mut shortcuts);
                }
                shortcuts
            },
            // Derived here rather than held on `Runtime`, for the two lists
            // beside it: the page has to show what is true *this frame*, and a
            // copy kept on the struct is a copy that has to be refreshed by
            // whoever changed the table, in every place they changed it.
            profiles::page_lines(
                &self.app.profile_programs,
                self.default_profile(),
                self.default_profile_is_automatic(),
            ),
            // And the folder, for the third list's own reason: which of the
            // scheme pickers' rows are files the reader wrote is what is in
            // `%APPDATA%\Folio\schemes\` *now*, and the catalogue is re-read on
            // a reason rather than on a clock — so reading it here is reading
            // the same revision the pickers above are drawn from.
            //
            // It outlived the `Delete scheme…` menu it was struck for (user
            // ruling 2026-08-19): the verbs moved onto the picker's own rows,
            // and this is still the answer to which rows they are drawn on.
            schemes::catalogue()
                .user_files()
                .into_iter()
                .map(|(name, file)| settings::SchemeFileLine {
                    name: name.to_owned(),
                    file: file.to_owned(),
                })
                .collect(),
            // **And what every row reads**, which is what the dialog is holding as much as its
            // rows are: a sentence is a function of the answer beside it, and since 2026-08-25 a
            // row's height is a function of the sentence. Derived here with the lists, and for
            // their reason — every caller then reads the *same* answers it hands to the panel.
            self.settings_values(),
        )
    }

    /// The two lists plus the third fact the dialog is holding: which pages have
    /// their Advanced group open (§7.1.6c-5).
    ///
    /// A function and not a field, for `settings_content`'s own reason: the
    /// disclosure state lives in `settings.json` and this reads it there, so the
    /// height, the focus order and the draw cannot be built from a copy that has
    /// drifted from the file. Resolving the stored keys here is also where an
    /// unknown one is dropped — §5.4 逐叶降级, and `AdvancedOpen::from_keys` is
    /// the one place it happens.
    pub(crate) fn settings_dialog<'a>(
        &self,
        rows: &'a [settings::SettingsRow],
        shortcuts: &'a [shortcuts::ShortcutRow],
        profiles: &'a [profiles::ProfileLine],
        scheme_files: &'a [settings::SchemeFileLine],
        values: &'a settings::SettingsValues,
    ) -> settings::SettingsContent<'a> {
        settings::SettingsContent {
            rows,
            shortcuts,
            profiles,
            scheme_files,
            advanced: self.advanced_open(),
            // **`None` is "no clock"**, which is a window under reduced motion,
            // a group that has finished moving, and every test in `settings`.
            // The group is then simply open or shut exactly as `advanced` says,
            // which is the picture this dialog drew before it could animate at
            // all — the red line the whole block is written under, said in one
            // `Option`.
            // Read the animation clock's one sample. Sampling Instant::now()
            // here would make each hit test/draw ask for different geometry.
            advanced_reveal: self
                .window
                .advanced_reveal_sample
                .map(|(page, reveal)| (page, f32::from(reveal) / 1000.0)),
            editor: self.editor_subject(),
            values,
        }
    }

    /// What the editor sub-page is standing on, resolved out of the profile
    /// table — or `None` while the Profiles page is showing its list.
    ///
    /// **Re-read every frame rather than held**, which is what makes it safe for
    /// the editor to hold nothing but an index: a reorder or an Undo moves rows
    /// under it, and a copy of the profile taken when the page opened would be a
    /// page drawing a row that is no longer there. The index itself is kept in
    /// step by the verbs that move it.
    fn editor_subject(&self) -> Option<settings::EditorSubject> {
        let editor = self.window.settings.editor()?;
        let index = editor.index;
        if index >= profiles::count() {
            return None;
        }
        let user = profiles::is_user(index);
        let start_at = profiles::start_at(index);
        let env = profiles::env(index);
        Some(settings::EditorSubject {
            index,
            title: profiles::title(index),
            user,
            colour: match profiles::mark(index) {
                marks::ChromeMark::ProfileGeneric { colour } => {
                    marks::MarkColour::ALL.iter().position(|it| *it == colour)
                }
                // A profile of the reader's own that still wears the mark it was
                // duplicated with is on none of the eight, and its picker says
                // so rather than ticking a colour it is not.
                _ => None,
            },
            colour_reason: profiles::mark_is_its_own(index),
            // **The `Name` field's refusal travels with the answers** (moved here 2026-08-25): it
            // replaces that row's sentence, a sentence is a number of lines and a number of lines
            // is a row's height, so the layout has to be able to read it.
            refusal: editor.refusal.map(i18n::Text::text),
            // And the `Program` field's, for the same reason (0.4.6 ticket 74).
            program_refusal: editor.program_refusal.map(i18n::Text::text),
            login: profiles::login(index),
            start_at: match start_at {
                profiles::StartAt::Inherit => 0,
                profiles::StartAt::Home => 1,
                profiles::StartAt::Fixed(_) => 2,
            },
            fixed_folder: match &start_at {
                profiles::StartAt::Fixed(path) => Some(profiles::intern_path(path)),
                _ => None,
            },
            hyperlink: match env
                .iter()
                .find(|(name, _)| name == settings::FORCE_HYPERLINK_NAME)
                .map(|(_, value)| value.as_str())
            {
                // Nothing said is `Auto`: the terminal's own declaration stands,
                // which is `hyperlink_declaration`'s behaviour byte for byte.
                None => 0,
                Some("0") => 2,
                Some(_) => 1,
            },
            // **The editor's own length of the same sentence**: this row has the
            // page's whole width, where the list's third line shares its row
            // with an action run.
            capability: profiles::capability_in_editor(
                index,
                self.app
                    .profile_programs
                    .known_shell(&profiles::id(index))
                    .as_ref(),
            )
            .text(),
            // The ghosts are not the reader's and are not counted: they are what
            // this terminal says to a session of this profile.
            env_rows: editor.env.len(),
            integration: settings::INTEGRATION_OPTIONS
                .iter()
                .position(|choice| *choice == profiles::integration_choice(index))
                .unwrap_or_default(),
            integration_auto: profiles::integration_auto_label(
                index,
                self.app
                    .profile_programs
                    .known_shell(&profiles::id(index))
                    .as_ref(),
            ),
            // **Every variable this terminal sets for a session of this profile,
            // less whatever this profile has already said** (owner ruling
            // 2026-10-11 on H2 Q4): read off the launch's own composition, so a
            // PowerShell gets no `FORCE_HYPERLINK` ghost because its own script is
            // the half that declares links, a `NO_COLOR` row takes `COLORTERM` and
            // `TERM` away, and a name the reader has a row for is no longer the
            // terminal's to say — which is what makes adopting a ghost a move and
            // not a duplication.
            //
            // Composed from the environment the next pane would be born from — the account's
            // current one, asked for as the page opens — for the kind of program the walk found,
            // and empty while either is still being asked (`shell_integration::page_environment`,
            // `ProfilePrograms::known_shell`), as the sentence and the `Auto` label say
            // "checking" until the kind is known.
            ghosts: settings::held_ghosts(
                &profiles::row_of(&profiles::id(index))
                    .zip(shell_integration::page_environment())
                    .zip(self.app.profile_programs.known_shell(&profiles::id(index)))
                    .map(|((row, before_folio), program_kind)| {
                        shell_integration::declared_environment(
                            &row,
                            &program_kind,
                            shell_integration::Scripts::installed(),
                            &before_folio,
                            attention_wire::endpoint_name(),
                        )
                    })
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(name, value)| settings::EnvGhost {
                        name: profiles::intern_text(&name),
                        value: match value {
                            shell_integration::DeclaredValue::Said(value) => {
                                settings::GhostValue::Said(profiles::intern_text(&value))
                            }
                            shell_integration::DeclaredValue::PerPane => {
                                settings::GhostValue::PerPane
                            }
                        },
                    })
                    .collect::<Vec<_>>(),
            ),
        })
    }

    /// Which pages have their Advanced group open, as the settings file says.
    fn advanced_open(&self) -> settings::AdvancedOpen {
        settings::AdvancedOpen::from_keys(
            self.app
                .settings_store
                .loaded()
                .advanced_open
                .iter()
                .map(String::as_str),
        )
    }

    /// What every row in the settings dialog currently reads.
    pub(crate) fn settings_values(&self) -> settings::SettingsValues {
        settings::SettingsValues {
            theme: self.app.settings_store.loaded().theme_mode,
            high_contrast: bt_render::high_contrast_in_force().is_some(),
            cursor: current_cursor_style(),
            tab_layout: self.window.rail.layout,
            sidebar: self.window.rail.mode,
            display_formulas: self.app.settings_store.loaded().display_formulas,
            inline_formulas: self.app.settings_store.loaded().inline_formulas,
            repair_row_breaks: self.app.settings_store.loaded().repair_row_breaks,
            tables: self.app.settings_store.loaded().tables,
            block_max_height: self.app.settings_store.loaded().block_max_height,
            focus_card_height: self.app.settings_store.loaded().focus_card_height,
            // The one effective table — the object the Shortcuts page records
            // into — so the `Cards` row names the chord that answers, in this
            // platform's spelling and after any rebind (0.4.4 ticket 06).
            focus_mode_chord: self
                .app
                .shortcuts
                .accelerator(shortcuts::Action::ToggleFocusMode),
            scrollback_lines: self.app.settings_store.loaded().scrollback_lines,
            line_wrapping: self.app.settings_store.loaded().line_wrapping,
            copy_on_select: self.app.settings_store.loaded().copy_on_select,
            multiline_paste_ask: self.app.settings_store.loaded().multiline_paste_ask,
            terminal_notifications: self.app.settings_store.loaded().terminal_notifications,
            turn_end_notification: self.app.settings_store.loaded().turn_end_notification,
            shell_integration: self.app.settings_store.loaded().shell_integration,
            powershell_profile_line_present: shell_integration::powershell_profile_line_present(),
            git_panel: self.app.settings_store.loaded().git_panel,
            update_check: self.app.settings_store.loaded().update_check,
            version_update: update_card::version_row(
                &self.app.update_job,
                update::check_view(),
                update::offer().as_deref(),
                update::unix_epoch_ms(),
                i18n::current(),
            ),
            update_mark: self
                .window
                .settings
                .update_row_marked(update::gear_mark_is_lit(&self.app.update_job)),
            // The panel's, for this visit (T-UNINSTALL-UX).
            uninstall_remove_data: self.window.settings.uninstall_remove_data(),
            key_hints: self.app.settings_store.loaded().key_hints,
            option_sends_alt: self.app.settings_store.loaded().option_sends_alt,
            // The machine's own answer, cached at the three moments it can
            // change — see `App::context_menu_installed`.
            context_menu: self.app.context_menu_installed,
            // **Not cached on `App`**, unlike the row above, and the difference
            // is which thread writes it: this answer is produced on a worker and
            // already lives behind one lock in `explorer_menu`, so a second copy
            // here would be a copy that a window which happened not to be redrawn
            // could hold a stale version of. One `Mutex` read per frame the
            // dialog is up is not a cost worth a second store.
            explorer_first_page: explorer_menu::state().registered(),
            // **What the machine could do, beside what it has done.** Both halves
            // are cheap here — one remembered kernel answer and one cell the start
            // settled (`explorer_menu::package_file`), nothing asked of the disk —
            // and neither can be answered by the row itself, which is why the
            // answer is handed in.
            explorer_first_page_offered: explorer_menu::first_page_offered(
                explorer_menu::supported(),
                explorer_menu::package_file().is_some(),
            ),
            // The same, over a file instead of the registry.
            claude_hooks: self.app.claude_hooks_installed,
            // And the same again, over codex's own file.
            codex_notify: self.app.codex_notify_installed,
            // The other half of the same three reads: why a file will not be edited, when the
            // answer is not `Off` at all. Handed in, never asked for here — the three sentences
            // are read off the disk and this runs on every frame the dialog is up.
            agent_config_refusals: self.app.agent_config_refusals,
            // And over copilot's own hooks directory, with the second fact this one needs beside
            // it: whether the copilot on this machine can carry the signal at all.
            copilot_hooks: self.app.copilot_hooks_installed,
            copilot_readiness: self.app.copilot_readiness,
            split_direction: self.app.settings_store.loaded().split_direction,
            search_engine: self.app.settings_store.loaded().search_engine,
            launch_opens: self.app.settings_store.loaded().launch_opens,
            minimum_contrast: self.app.settings_store.loaded().minimum_contrast,
            web_color_scheme: self.app.settings_store.loaded().web_color_scheme,
            language: self.app.settings_store.loaded().language,
            default_profile: self.default_profile(),
            terminal_font: settings::family_index(
                &self.app.settings_store.loaded().terminal_font_family,
            ),
            terminal_cjk_font: settings::cjk_family_index(
                &self.app.settings_store.loaded().terminal_cjk_font_family,
            ),
            font_size: settings::font_size_index(
                self.app.settings_store.loaded().terminal_font_size,
            ),
            light_scheme: settings::scheme_index(
                &self.app.settings_store.loaded().light_scheme,
                true,
            ),
            dark_scheme: settings::scheme_index(
                &self.app.settings_store.loaded().dark_scheme,
                false,
            ),
            // **This window's posture, not the file's** (§7.1.6b′). All five
            // doors turn the window's bit and only the row also writes the file,
            // so the row has to report what it can see: the window it is standing
            // in. See `settings::SettingsValues::focus_mode`.
            focus_mode: self.window.focus_mode,
            profile_available: (0..profiles::count())
                .map(|index| self.app.profile_programs.row_is_available(index))
                .collect(),
            editor: self.editor_subject(),
            background_image: !self.app.settings_store.loaded().background_image.is_empty(),
            background_fit: settings::image_fit_index(
                self.app.settings_store.loaded().background_fit,
            ),
            background_image_opacity: self.app.settings_store.loaded().background_image_opacity,
            background_opacity: self.app.settings_store.loaded().background_opacity,
            acrylic: self.app.settings_store.loaded().acrylic,
            always_on_top: self.app.settings_store.loaded().always_on_top,
            quake_height: self.app.settings_store.loaded().quake_height,
            quake_width: self.app.settings_store.loaded().quake_width,
            quake_dismiss_on_blur: self.app.settings_store.loaded().quake_dismiss_on_blur,
            quake_top_gap: self.app.settings_store.loaded().quake_top_gap,
            quake_restore: self.app.settings_store.loaded().quake_restore,
            // **The resolved item and not the stored id**, on `default_profile`'s ruling one field
            // above: item zero is "whatever the default profile is", and an id this machine no
            // longer has ticks it — which is where a summon on an unknown profile really does open.
            quake_profile: profiles::position_of(
                &self.app.settings_store.loaded().quake_profile_id,
            )
            .map_or(0, |index| index + 1),
            // A fact about the machine and not about the file, which is why it is
            // read off the claim rather than out of the settings - see
            // `quake::Quake::hotkey_taken`.
            quake_hotkey_taken: self.app.quake.hotkey_taken(),
            // Both asked of the machine once, at startup, and remembered — see
            // `App::acrylic_available` / `translucency_available`. Asking
            // DWM per frame would be a syscall inside a hover repaint.
            acrylic_available: self.app.acrylic_available,
            translucency_available: self.window.translucency_available,
            scheme_in_force_is_user_file: self.scheme_in_force().is_some(),
        }
    }

    /// A slider row, driven to a whole percentage by an arrow key or a drag.
    ///
    /// One function for both rows, because the two differ only in which field
    /// they write — and one function for the keyboard and the pointer, because a
    /// drag that persisted differently from an arrow press would be two settings
    /// with one name.
    pub(crate) fn apply_slider(&mut self, row: settings::SettingsRow, value: u8) -> Result<bool> {
        let Some(range) = row.control().range() else {
            return Ok(false);
        };
        let value = range.clamp(value);
        let mut settings = self.app.settings_store.loaded().clone();
        match row {
            settings::SettingsRow::ImageOpacity => settings.background_image_opacity = value,
            settings::SettingsRow::BackgroundOpacity => settings.background_opacity = value,
            settings::SettingsRow::QuakeHeight => settings.quake_height = value,
            settings::SettingsRow::QuakeWidth => settings.quake_width = value,
            _ => return Ok(false),
        }
        if &settings == self.app.settings_store.loaded() {
            return Ok(false);
        }
        if !self.app.settings_store.store(settings) {
            return Ok(false);
        }
        // The ground is re-put whether or not it moved the clear: the picture's
        // own percentage lives on the quad and not on the clear, and
        // `set_window_ground` is the one thing that knows which of the two a
        // given change touched.
        self.apply_window_ground()?;
        // A drag repaints every frame it moves, and a ground whose only change
        // was the picture's opacity does not advance the revision — so the
        // repaint has to be asked for here rather than left to
        // `apply_window_ground`.
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// Read the open picker's thumb backwards into an offset (§7.1.6c-5).
    ///
    /// `preview::scroll_dragged_to` and not arithmetic of its own: a thumb is a
    /// picture of the offset, and dragging one is reading that picture backwards
    /// — a second implementation is how a bar and its own hit test come apart.
    fn drag_settings_menu_bar(
        &mut self,
        bar: &preview::ScrollBar,
        position: PhysicalPosition<f64>,
    ) -> Result<()> {
        let held = self
            .held_settings_menu_bar_drag()
            .copied()
            .unwrap_or_default();
        let along = bar.along([position.x as f32, position.y as f32]);
        let scrolled = preview::scroll_dragged_to(bar, along, held);
        if !self.window.settings.scroll_menu(scrolled) {
            return Ok(());
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The scheme the window is **actually wearing**, as a name and a file, when
    /// that file is one of the user's own (§7.1.6c-4d).
    ///
    /// `None` for a bundled scheme, which has no file anywhere, and `None` for
    /// the stored default, which is bundled by definition. The canvas decides
    /// which of the two rows is being asked, by luma — the question
    /// `Runtime::customise_scheme` asks and the question the chrome asks, and
    /// deliberately not the theme setting, because a `BT_BG` override makes
    /// those two disagree and it is the canvas the reader is looking at.
    ///
    /// One reading with two callers — the dialog's `Delete scheme` predicate and
    /// the press that acts on it — so a button that is live and a press that
    /// finds nothing to delete cannot come apart.
    fn scheme_in_force(&self) -> Option<(String, String)> {
        let light = bt_render::background_is_light(bt_render::background_rgb());
        let stored = if light {
            self.app.settings_store.loaded().light_scheme.clone()
        } else {
            self.app.settings_store.loaded().dark_scheme.clone()
        };
        if stored.is_empty() {
            return None;
        }
        let catalogue = schemes::catalogue();
        let file = catalogue.user_file_of(&stored, light)?.to_owned();
        Some((stored, file))
    }
}

impl<B: Backend> settings::geometry::PointerHost for Runtime<'_, B> {
    fn settings_geometry(&mut self) -> Option<Arc<settings::SettingsLayout>> {
        self.settings_layout()
    }

    fn settings_drag(&mut self, layout: &settings::SettingsLayout, x: f64, y: f64) -> Result<bool> {
        // Captured gestures own the pointer, even outside their original track.
        if let Some(row) = self.held_settings_slider_drag().copied() {
            if let Some(value) = layout.slider_at(row, x) {
                self.apply_slider(row, value)?;
            }
            return Ok(true);
        }
        if self.held_settings_menu_bar_drag().copied().is_some()
            && let Some(bar) = layout.menu_bar()
        {
            self.drag_settings_menu_bar(&bar, PhysicalPosition::new(x, y))?;
            return Ok(true);
        }
        Ok(false)
    }

    fn settings_values(&self) -> settings::SettingsValues {
        Runtime::settings_values(self)
    }

    fn settings_hover(&mut self, target: settings::SettingsTarget) -> Result<()> {
        if self.window.settings.set_hover(Some(target)) && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }
}

/// **Winit's key, in the settings dialog's own vocabulary.**
///
/// Total rather than partial — every key maps, and the ones with no verb map to
/// [`settings::SettingsKey::Other`] — because a modal swallows all of them and
/// every one of them is keyboard interaction that turns the focus ring on. An
/// `Option` here would have made "no verb" and "not a key press" the same
/// answer, and they are different answers to `:focus-visible`.
///
/// **A held modifier makes it `Other`.** `Ctrl+Tab` is the tab-switching chord,
/// `Alt+F4` is the window's; neither is this dialog's Tab, and a dialog that
/// took the second half of somebody else's chord as its own would be walking its
/// focus order on a keystroke aimed past it. The chords themselves stay
/// swallowed — a modal means MODAL — so this narrows what the dialog *does*,
/// never what it lets through.
///
/// Auto-repeat walks and does not press: holding Down runs down a picker, while
/// holding Enter chooses once. A repeat of an activating key is a key the user
/// is *still* holding, not a second decision.
/// Whether a physical-pixel rectangle holds a pointer position.
///
/// Half-open on both axes, exactly as `settings::hit`'s own `contains` is, so
/// two boxes that share an edge cannot both answer for the pixel on it.
fn contains_point(rect: [f32; 4], position: PhysicalPosition<f64>) -> bool {
    let (x, y) = (position.x as f32, position.y as f32);
    x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]
}
