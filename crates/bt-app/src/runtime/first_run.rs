//! `first_run` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{
    Announce, Runtime, attention_codex, attention_copilot, attention_hooks, attention_ownership,
    diagnostics, explorer_menu, first_run, i18n, install_channel, persist, psreadline, restore,
    toast, tooltip,
};
use anyhow::Result;
use std::path::PathBuf;
use winit::keyboard::{Key, NamedKey};

impl Runtime<'_> {
    /// What the Terminal page's PSReadLine row is describing.
    pub(crate) fn psreadline_row_state(&self) -> psreadline::RowState {
        psreadline::row_state(
            psreadline::probe(),
            self.app.settings_store.loaded().psreadline_invite,
            self.app.psreadline_installed.unwrap_or_default(),
        )
    }

    /// Re-read whether the module is on disk. Cheap enough at the three moments
    /// it is called and far too expensive on every frame — see the field.
    pub(crate) fn refresh_psreadline_installed(&mut self) {
        psreadline::refresh_installed(
            &mut self.app.psreadline_installed,
            self.app.psreadline_documents.as_deref(),
        );
    }

    /// Where this machine's `Documents` is, asked again if the launch could not
    /// say (§7.47).
    ///
    /// The answer is cached because the row re-reads the disk three times a
    /// session and a known-folder round trip per read is not free. **`None` is
    /// not cached**, though, and that is the whole of this function: a launch
    /// that could not resolve the folder used to freeze that "no" for the life
    /// of the process, and every press on the row after it returned in silence.
    /// A machine can grow the answer under a running window — a profile that
    /// finishes redirecting, a network home that comes back — and the press is
    /// exactly the moment worth asking again.
    pub(crate) fn psreadline_documents(&mut self) -> Option<PathBuf> {
        if self.app.psreadline_documents.is_none() {
            self.app.psreadline_documents = psreadline::documents_directory();
        }
        self.app.psreadline_documents.clone()
    }

    /// Write the module, or take Folio's own copy back off disk.
    ///
    /// **Every road out of here says something** (§7.47). The decision itself
    /// belongs to `psreadline::apply`, which has no silent exit; this function
    /// is the two lines that turn its answer into a card and a line in
    /// `diagnostics.log`. Before 2026-08-29 there were three ways for a press
    /// on this row to change nothing and report nothing — a `Documents` folder
    /// Windows would not name, a removal the guard refused, and (the one a user
    /// met on a new machine) an execution policy that greyed the item so the
    /// press never arrived at all.
    pub(crate) fn apply_psreadline(&mut self, install: bool) -> Result<bool> {
        let documents = self.psreadline_documents();
        let state = self.psreadline_row_state();
        let outcome = psreadline::apply_recorded(
            install,
            documents.as_deref(),
            state,
            psreadline::probe(),
            &persist::storage_dir(),
        );
        match outcome {
            psreadline::Outcome::Installed(root) => {
                eprintln!(
                    "BT_PSREADLINE installed {} to {}",
                    psreadline::PATCHED_VERSION,
                    root.display()
                );
                self.record_psreadline_invite(bt_persist::PsReadLineInviteV1::Installed);
                self.refresh_psreadline_installed();
                self.toast(
                    toast::ToastKind::Ok,
                    toast::ToastAnchor::Window,
                    None,
                    i18n::psreadline_installed_toast(psreadline::PATCHED_VERSION),
                )?;
                Ok(true)
            }
            psreadline::Outcome::Removed(root) => {
                eprintln!("BT_PSREADLINE removed {}", root.display());
                self.record_psreadline_invite(bt_persist::PsReadLineInviteV1::Dismissed);
                self.refresh_psreadline_installed();
                self.toast(
                    toast::ToastKind::Ok,
                    toast::ToastAnchor::Window,
                    None,
                    i18n::Text::PsReadLineRemovedToast.text().to_owned(),
                )?;
                Ok(true)
            }
            psreadline::Outcome::Refused(refusal) => {
                // The log line and the card carry the same fact, and the log
                // line is not the card: on the machine where this fires nobody
                // can see the screen, and a tag is what a `diagnostics.log`
                // can be grepped for in either language.
                eprintln!(
                    "BT_PSREADLINE refused install={install} why={} — {}",
                    refusal.tag(),
                    refusal.sentence()
                );
                self.toast(
                    toast::ToastKind::Error,
                    toast::ToastAnchor::Window,
                    None,
                    refusal.sentence(),
                )?;
                Ok(false)
            }
        }
    }

    /// Register the package that puts Folio on the first page of Explorer's
    /// menu, or take it back off (§7.4a).
    ///
    /// **The only row in this dialog whose press is not answered on this turn.**
    /// A deployment is a service call of a second or three, and running it here
    /// would freeze every window in this process for that long — so what this
    /// does is start it. The row is redrawn from the machine when it lands, which
    /// is `apply_explorer_place`'s discipline over a store that is slower to ask
    /// rather than a weaker version of it: there is still exactly one copy of
    /// this truth and it is still not in `settings.json`.
    ///
    /// Nothing is said here. A card raised now would be a card about what was
    /// asked for rather than about what happened, and the thing that happened
    /// arrives on `AppEvent::ExplorerPackageChanged`.
    pub(crate) fn request_explorer_package(
        &mut self,
        place: explorer_menu::ExplorerPlace,
        announce: Announce,
    ) {
        self.app.explorer_package_asked_by = Some(self.window_id());
        // Carried with the request rather than read at the far end, because by
        // the time the answer lands the card that asked has closed and there
        // would be nothing left to ask. The place travels for the same reason
        // and buys the same thing: the card names where the verb ended up, and
        // where it ended up is what this press said rather than what the
        // deployment database happens to hold when the answer lands.
        self.app.explorer_package_announce = announce;
        self.app.explorer_package_asked_place = place;
        // **Every press reaches the module** (review row R4-12). It used to be
        // refused here while a job was running, on the reasoning that the
        // machine was already going where the press wanted it — which is true of
        // a second `On` and exactly wrong for an `Off`. The module now writes a
        // press down and the running job spends it on its way out.
        explorer_menu::request(place.package());
    }

    /// Put the finished registration's answer on the window in front of the
    /// person who asked for it.
    ///
    /// **The card names the place the row was set to**, not the half this
    /// deployment did: since the rows merged there is one press and one answer,
    /// and a reader who moved from the first page down to the classic entry is
    /// owed "the verb is under Show more options" rather than "the first page
    /// lost something". The place is the one carried with the request — see
    /// [`Self::request_explorer_package`].
    ///
    /// A failure carries Windows' own sentence, `apply_explorer_place`'s rule: on
    /// the machine where a deployment is refused nobody else can see it, and the
    /// refusal names a condition — a certificate the machine will not trust, a
    /// package already registered by another user — that no words of ours could
    /// guess.
    pub(crate) fn report_explorer_package(&mut self, outcome: Result<bool, String>) -> Result<()> {
        match outcome {
            // The first-run card asked for this beside three others and the
            // Settings row now reads it; a card for each is noise (§7.56 §8).
            Ok(_) if self.app.explorer_package_announce == Announce::OnlyFailures => Ok(()),
            Ok(_) => self.toast(
                toast::ToastKind::Ok,
                toast::ToastAnchor::Window,
                None,
                explorer_menu::place_toast(
                    self.app.explorer_package_asked_place,
                    explorer_menu::shell_refresh_pending(),
                )
                .text()
                .to_owned(),
            ),
            Err(error) => self.toast(
                toast::ToastKind::Error,
                toast::ToastAnchor::Window,
                None,
                i18n::explorer_first_page_failed(&error),
            ),
        }
    }

    /// Write Folio's hooks into the user's own Claude Code configuration, or take them back out.
    ///
    /// **The row is redrawn from the file either way**, `apply_context_menu`'s discipline over a
    /// different kind of store and for the same reason: there is one copy of this truth and it is
    /// not in `settings.json`, so a refusal leaves the switch standing where the machine actually is
    /// rather than where the press hoped it would be.
    ///
    /// **Nothing is written anywhere but that one file**, and nothing at all is written into a
    /// working directory or a repository — `attention_hooks`'s header has upstream's own reason for
    /// that, which is stronger than ours.
    ///
    /// A refusal carries a sentence, because on the machine where it fires nobody else can see it.
    /// The one that matters is a settings file this build cannot read: it is left exactly as it is,
    /// because it belongs to somebody who wrote it.
    pub(crate) fn apply_claude_hooks(&mut self, install: bool, announce: Announce) -> Result<bool> {
        let config = attention_hooks::settings_path();
        let decision = attention_ownership::next_decision(
            &mut self.app.agent_takeovers[0],
            install,
            config.as_deref(),
        );
        let exe = std::env::current_exe().ok();
        let outcome = match attention_ownership::stable_executable(exe.as_deref()) {
            Ok(exe) => attention_hooks::apply(decision, exe),
            Err(reason) => attention_hooks::Outcome::Refused(reason),
        };
        if let attention_hooks::Outcome::TakeOverRequired(owners) = &outcome {
            self.app.agent_takeovers[0] = attention_ownership::Pending::new(config, owners.clone());
        }
        (
            self.app.claude_hooks_installed,
            self.app.agent_config_refusals[0],
        ) = attention_hooks::row_state();
        match outcome {
            attention_hooks::Outcome::Installed | attention_hooks::Outcome::Removed
                if announce == Announce::OnlyFailures =>
            {
                Ok(true)
            }
            attention_hooks::Outcome::Installed => {
                self.toast(
                    toast::ToastKind::Ok,
                    toast::ToastAnchor::Window,
                    None,
                    i18n::Text::ClaudeHooksAddedToast.text().to_owned(),
                )?;
                Ok(true)
            }
            attention_hooks::Outcome::Removed => {
                self.toast(
                    toast::ToastKind::Ok,
                    toast::ToastAnchor::Window,
                    None,
                    i18n::Text::ClaudeHooksRemovedToast.text().to_owned(),
                )?;
                Ok(true)
            }
            // The file already said what the press asked for. Nothing was written, and a card
            // saying so would be a card about this build's bookkeeping.
            attention_hooks::Outcome::Unchanged => Ok(true),
            attention_hooks::Outcome::TakeOverRequired(owners)
            | attention_hooks::Outcome::LeftOther(owners) => {
                self.toast(
                    toast::ToastKind::Error,
                    toast::ToastAnchor::Window,
                    None,
                    i18n::agent_owner_notice(install, &owners),
                )?;
                Ok(false)
            }
            attention_hooks::Outcome::Refused(reason) => {
                self.toast(
                    toast::ToastKind::Error,
                    toast::ToastAnchor::Window,
                    None,
                    i18n::agent_install_refused(i18n::Text::ClaudeHooksFailedToast.text(), reason),
                )?;
                Ok(false)
            }
        }
    }

    /// Write Folio's `notify` program into the user's own codex configuration, or take it back out.
    ///
    /// [`Self::apply_claude_hooks`]'s verb over a second upstream and a second file format, and
    /// every clause of that one's note holds here: the row is redrawn from the file either way, so
    /// a refusal leaves the switch standing where the machine actually is; nothing is written
    /// anywhere but the one user-level file; and a refusal carries a sentence, because on the
    /// machine where it fires nobody else can see it.
    ///
    /// The refusal this one has that the other does not is **somebody else's `notify`**. There is
    /// one such key in that file, so installing over it would delete a program this build cannot
    /// give back — see `attention_codex`'s header.
    pub(crate) fn apply_codex_notify(&mut self, install: bool, announce: Announce) -> Result<bool> {
        let config = attention_codex::config_path();
        let decision = attention_ownership::next_decision(
            &mut self.app.agent_takeovers[1],
            install,
            config.as_deref(),
        );
        let exe = std::env::current_exe().ok();
        let outcome = match attention_ownership::stable_executable(exe.as_deref()) {
            Ok(exe) => attention_codex::apply(decision, exe),
            Err(reason) => attention_codex::Outcome::Refused(reason),
        };
        if let attention_codex::Outcome::TakeOverRequired(owners) = &outcome {
            self.app.agent_takeovers[1] = attention_ownership::Pending::new(config, owners.clone());
        }
        (
            self.app.codex_notify_installed,
            self.app.agent_config_refusals[1],
        ) = attention_codex::row_state();
        match outcome {
            attention_codex::Outcome::Installed | attention_codex::Outcome::Removed
                if announce == Announce::OnlyFailures =>
            {
                Ok(true)
            }
            attention_codex::Outcome::Installed => {
                self.toast(
                    toast::ToastKind::Ok,
                    toast::ToastAnchor::Window,
                    None,
                    i18n::Text::CodexNotifyAddedToast.text().to_owned(),
                )?;
                Ok(true)
            }
            attention_codex::Outcome::Removed => {
                self.toast(
                    toast::ToastKind::Ok,
                    toast::ToastAnchor::Window,
                    None,
                    i18n::Text::CodexNotifyRemovedToast.text().to_owned(),
                )?;
                Ok(true)
            }
            // The file already said what the press asked for.
            attention_codex::Outcome::Unchanged => Ok(true),
            attention_codex::Outcome::TakeOverRequired(owners)
            | attention_codex::Outcome::LeftOther(owners) => {
                self.toast(
                    toast::ToastKind::Error,
                    toast::ToastAnchor::Window,
                    None,
                    i18n::agent_owner_notice(install, &owners),
                )?;
                Ok(false)
            }
            attention_codex::Outcome::Refused(reason) => {
                self.toast(
                    toast::ToastKind::Error,
                    toast::ToastAnchor::Window,
                    None,
                    i18n::agent_install_refused(i18n::Text::CodexNotifyFailedToast.text(), reason),
                )?;
                Ok(false)
            }
        }
    }

    /// Write Folio's hook file into the user's own copilot hooks directory, or take it back out.
    ///
    /// [`Self::apply_claude_hooks`]'s verb over a third upstream, and every clause of that note
    /// holds: the row is redrawn from the machine either way, so a refusal leaves the switch where
    /// the machine actually is; nothing is written anywhere but the one user-level file; and a
    /// refusal carries a sentence, because on the machine where it fires nobody else can see it.
    ///
    /// The refusals this one has that the others do not are **a version too old to mean what this
    /// build would read** — see `attention_copilot`'s header for the changelog entry that bought
    /// the gate — and **a `folio.json` that is somebody else's**, which is the same refusal codex's
    /// installer makes about a `notify` key, over a file name instead of a key name.
    ///
    /// The readiness is re-read after the press for the reason the installed flag is: the sentence
    /// under the row is a fact about the machine, and a press is one of the moments a fact about
    /// the machine can have changed.
    pub(crate) fn apply_copilot_hooks(
        &mut self,
        install: bool,
        announce: Announce,
    ) -> Result<bool> {
        let config = attention_copilot::hooks_path();
        let decision = attention_ownership::next_decision(
            &mut self.app.agent_takeovers[2],
            install,
            config.as_deref(),
        );
        let exe = std::env::current_exe().ok();
        let outcome = match attention_ownership::stable_executable(exe.as_deref()) {
            Ok(exe) => attention_copilot::apply(decision, exe),
            Err(reason) => attention_copilot::Outcome::Refused(reason),
        };
        if let attention_copilot::Outcome::TakeOverRequired(owners) = &outcome {
            self.app.agent_takeovers[2] = attention_ownership::Pending::new(config, owners.clone());
        }
        (
            self.app.copilot_hooks_installed,
            self.app.agent_config_refusals[2],
        ) = attention_copilot::row_state();
        self.app.copilot_readiness = attention_copilot::readiness();
        match outcome {
            attention_copilot::Outcome::Installed | attention_copilot::Outcome::Removed
                if announce == Announce::OnlyFailures =>
            {
                Ok(true)
            }
            attention_copilot::Outcome::Installed => {
                self.toast(
                    toast::ToastKind::Ok,
                    toast::ToastAnchor::Window,
                    None,
                    i18n::Text::CopilotHooksAddedToast.text().to_owned(),
                )?;
                Ok(true)
            }
            attention_copilot::Outcome::Removed => {
                self.toast(
                    toast::ToastKind::Ok,
                    toast::ToastAnchor::Window,
                    None,
                    i18n::Text::CopilotHooksRemovedToast.text().to_owned(),
                )?;
                Ok(true)
            }
            // The directory already said what the press asked for.
            attention_copilot::Outcome::Unchanged => Ok(true),
            attention_copilot::Outcome::TakeOverRequired(owners)
            | attention_copilot::Outcome::LeftOther(owners) => {
                self.toast(
                    toast::ToastKind::Error,
                    toast::ToastAnchor::Window,
                    None,
                    i18n::agent_owner_notice(install, &owners),
                )?;
                Ok(false)
            }
            attention_copilot::Outcome::Refused(reason) => {
                self.toast(
                    toast::ToastKind::Error,
                    toast::ToastAnchor::Window,
                    None,
                    i18n::agent_install_refused(i18n::Text::CopilotHooksFailedToast.text(), reason),
                )?;
                Ok(false)
            }
        }
    }

    fn record_psreadline_invite(&mut self, state: bt_persist::PsReadLineInviteV1) {
        if self.app.settings_store.loaded().psreadline_invite == state {
            return;
        }
        let mut settings = self.app.settings_store.loaded().clone();
        settings.psreadline_invite = state;
        self.app.settings_store.store(settings);
    }

    /// Raise the invitation once the probe has answered and the table says it is
    /// owed — `psreadline::invite_decision`.
    ///
    /// Polled from the event loop rather than pushed from the probe thread,
    /// because raising a modal is a change to the window and the window is this
    /// thread's. The check is three comparisons on the common path.
    pub(in crate::runtime) fn raise_psreadline_invite_if_due(&mut self) -> Result<()> {
        if self.window.psreadline_invite.is_open() || psreadline::probe().is_none() {
            return Ok(());
        }
        let installed = psreadline::installed_on_probe(
            &mut self.app.psreadline_installed,
            self.app.psreadline_documents.as_deref(),
            psreadline::probe(),
        );
        // **Any Folio copy silences the invitation**, this build's or an older
        // one's: the offer is "let Folio put its module on this machine", and it
        // is already there. What the older copy is owed is an *update*, and the
        // Terminal page's row is where that is offered — an unbidden modal for a
        // patch bump would be this product interrupting a reader over its own
        // release history.
        if installed != psreadline::InstalledCopy::None {
            return Ok(());
        }
        let decision = psreadline::invite_decision(
            psreadline::probe(),
            self.app.settings_store.loaded().psreadline_invite,
            self.window.psreadline_size_changed,
        );
        if decision != psreadline::InviteDecision::Show {
            return Ok(());
        }
        // The second showing is spent whether it is answered or not: a dialog
        // raised by a font-size change has had its one exception.
        if self.window.psreadline_size_changed {
            self.window.psreadline_size_changed = false;
        }
        self.window.psreadline_invite.open();
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The invitation, measured against a real font, or nothing while it is shut.
    pub(in crate::runtime) fn psreadline_invite_layout(&mut self) -> Option<restore::InviteLayout> {
        if !self.window.psreadline_invite.is_open() {
            return None;
        }
        let documents = self.app.psreadline_documents.clone()?;
        let (body, reason) = psreadline::invite_body(
            psreadline::probe(),
            &psreadline::module_directory(&documents),
        );
        let install_enabled = reason.is_none();
        let title = i18n::Text::PsReadLineInviteTitle.text();
        let decline_text = i18n::Text::PsReadLineNotNow.text();
        let install_text = i18n::Text::PsReadLineInstall.text();
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (width, height) = (width as f32, height as f32);
        let scale = self.window.renderer.scale_factor() as f32;
        let room = restore::content_width(width, scale);
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut wrap = |text: &str| {
            restore::wrap(text, room, |line| {
                renderer.measure_chrome_text(gpu, line, restore::SUB_FONT_LOGICAL_PX * scale)
            })
        };
        let message_lines = wrap(&body);
        let reason_lines = reason.as_deref().map(&mut wrap).unwrap_or_default();
        let content = restore::InviteContent {
            title,
            message_lines,
            reason_lines,
            decline_text,
            install_text,
            install_enabled,
            decline_text_width: renderer.measure_chrome_text(
                gpu,
                decline_text,
                restore::BUTTON_FONT_LOGICAL_PX * scale,
            ),
            install_text_width: renderer.measure_chrome_text(
                gpu,
                install_text,
                restore::BUTTON_FONT_LOGICAL_PX * scale,
            ),
        };
        Some(restore::invite_layout(&content, width, height, scale))
    }

    /// Put the first-run card up, once, on a machine that has never run Folio
    /// (§7.56).
    ///
    /// Polled from the event loop for `raise_psreadline_invite_if_due`'s reason
    /// — raising a modal is a change to the window and the window is this
    /// thread's — and for one more of its own: the copilot row cannot be offered
    /// until the version question has been answered, and that answer arrives on
    /// another thread.
    ///
    /// **`Shown` is written the moment it goes up, not when it is answered.** A
    /// crash, an `Alt+F4`, or a process killed while the card is on screen must
    /// not bring it back.
    pub(in crate::runtime) fn raise_first_run_if_due(&mut self) -> Result<()> {
        if self.window.first_run.is_open() || self.app.first_run_attempted {
            return Ok(());
        }
        let store = &self.app.settings_store;
        // **`BT_FIRST_RUN_CARD` raises it over a machine that has already
        // answered**, `BT_PSREADLINE_PROBE`'s door and for its reason: this card
        // is shown once per machine, ever, so without a door there is no way to
        // photograph it in a second language or to look at it again after a
        // change. It overrides **the gate and nothing else** — which rows are
        // offered, what `Done` spends, what is written down are all exactly what
        // they would be on a real first run, which is what makes a picture taken
        // through it worth anything.
        if !first_run::due(store.was_missing(), store.loaded().first_run_card)
            && !diagnostics::switched_on(std::env::var_os("BT_FIRST_RUN_CARD"))
        {
            return Ok(());
        }
        // The one row whose offer depends on a version, and the version comes
        // off another process. Starting the probe here rather than waiting for
        // the Agents page is what makes the wait finite; the card holds until it
        // lands, because a row offered on a copilot too old to honour it is a
        // row whose `Done` would raise a failure toast for a refusal that was
        // knowable before it was pressed.
        let copilot_on_path = self.agent_is_on_this_machine("copilot");
        if copilot_on_path {
            attention_copilot::begin_probe();
        }
        let copilot_ready = !copilot_on_path || attention_copilot::probe_settled();
        // **How this copy was installed decides whether the Explorer row
        // arrives on** (U-3), and it lands on its own worker. Read at start and
        // in milliseconds, so it is almost always here; when it is not, the card
        // waits one turn for it — the worker's wake brings that turn round — and
        // then reads a still-missing answer as unknown. Only once the copilot
        // wait is over, so the one turn is not spent while something else is
        // holding the card anyway.
        if copilot_ready
            && !first_run::install_channel_settled(
                &mut self.app.first_run_waited_for_channel,
                install_channel::channel().is_some(),
            )
        {
            return Ok(());
        }
        // The machine questions below include file reads. Consume readiness
        // once, before asking them, even if this platform offers no card rows.
        if !first_run::take_ready_edge(&mut self.app.first_run_attempted, copilot_ready) {
            return Ok(());
        }
        let machine = first_run::Machine {
            // Both halves of the first page: a Windows that shows one, and the
            // file that can be registered on it shipped beside the executable.
            // Through `first_page_offered` and not spelled again here, because
            // the Settings row asks the same question through the same function
            // and the card's switch has to mean what that row's switch means.
            explorer_first_page_available: explorer_menu::first_page_offered(
                explorer_menu::supported(),
                explorer_menu::package_file().is_some(),
            ),
            claude_found: self.agent_is_on_this_machine("claude"),
            claude_installable: attention_hooks::state() == attention_hooks::State::Absent,
            codex_found: self.agent_is_on_this_machine("codex"),
            codex_installable: attention_codex::state() == attention_codex::State::Absent,
            copilot_found: copilot_on_path,
            copilot_installable: attention_copilot::state() == attention_copilot::State::Absent
                && attention_copilot::readiness() == attention_copilot::Readiness::Ready,
            // Unknown on this launch, and honestly so: no shell has said where
            // its `$PROFILE` is yet. A reader whose file already carries the
            // line has their recorded intent cleared by the shell that reports
            // it — `first_run::pending_step`.
            powershell_integration_installed: false,
            install_channel: install_channel::channel()
                .unwrap_or(install_channel::Channel::Unknown),
        };
        let rows = first_run::rows(&machine);
        let shape = first_run::explorer_shape(&machine);
        // **Written down only if it went up** (macOS plan M3-6). The rows are
        // what this platform has the capability for, and a platform with none
        // of them is shown no card — `Card::open` is where that rule lives, and
        // recording `Shown` for a card nobody saw would spend the one first run
        // this machine has on nothing.
        if !self.window.first_run.open(rows, shape) {
            return Ok(());
        }
        self.record_first_run_card(bt_persist::FirstRunCardV1::Shown);
        self.rebuild_first_run_tip_anchors();
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Write down that the card has been up. Nothing shows it again.
    fn record_first_run_card(&mut self, state: bt_persist::FirstRunCardV1) {
        if self.app.settings_store.loaded().first_run_card == state {
            return;
        }
        let mut settings = self.app.settings_store.loaded().clone();
        settings.first_run_card = state;
        self.app.settings_store.store(settings);
    }

    /// **The card's rows, as the only things this window is willing to talk
    /// about while the card is up** (§7.56 v4, user ruling 2026-09-06).
    ///
    /// Every row carries the mechanism sentence v3 printed under its title: the
    /// reader is still owed the address of their own files, and a tooltip is
    /// where that is owed without being shown unasked. It is the window's own
    /// `.tip` — same host, same clock, same box — because a second popup would
    /// be a second clock over one pointer, which is how a window ends up
    /// showing two boxes at once ([`tooltip::TipFace`]).
    ///
    /// Called from the anchor rebuild and from the two places that move the
    /// body under a pointer that has not itself moved, because an anchor is
    /// only ever allowed to describe a box the card is actually drawing.
    pub(in crate::runtime) fn rebuild_first_run_tip_anchors(&mut self) {
        let mut anchors = tooltip::TooltipAnchors::default();
        if let Some(layout) = self.first_run_layout() {
            for (index, rect, text) in layout.tips() {
                anchors.push(tooltip::TooltipAnchorId::FirstRunRow(index), rect, text);
            }
        }
        self.window.tooltip_anchors = anchors;
    }

    /// The card, measured against a real font, or nothing while it is shut.
    pub(in crate::runtime) fn first_run_layout(&mut self) -> Option<first_run::Layout> {
        if !self.window.first_run.is_open() {
            return None;
        }
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (width, height) = (width as f32, height as f32);
        let scale = self.window.renderer.scale_factor() as f32;
        let rows: Vec<first_run::Row> = self.window.first_run.rows().to_vec();
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        // **Nothing here is wrapped except the faint line.** A row is one line
        // by construction in v4, so there is no measuring to do on it and no
        // font under which it can quietly become two.
        let row_contents = rows
            .iter()
            .map(|row| first_run::RowContent {
                group_break_above: row.group_break_above,
                line: row.line.text().to_owned(),
                tip: row.tip.text(),
                on: row.on,
            })
            .collect();
        let settings_lines = restore::wrap(
            i18n::Text::FirstRunSettingsLine.text(),
            first_run::settings_line_width(width, scale),
            |line| {
                measure(
                    line,
                    first_run::MEASURED_SETTINGS_LINE_FONT_LOGICAL_PX * scale,
                )
            },
        );
        let later = i18n::Text::FirstRunLater.text();
        let done = i18n::Text::FirstRunDone.text();
        let content = first_run::Content {
            title: i18n::Text::FirstRunTitle.text().to_owned(),
            rows: row_contents,
            settings_lines,
            later: later.to_owned(),
            later_width: measure(later, first_run::MEASURED_BUTTON_FONT_LOGICAL_PX * scale),
            done: done.to_owned(),
            done_width: measure(done, first_run::MEASURED_BUTTON_FONT_LOGICAL_PX * scale),
        };
        Some(first_run::layout(
            &content,
            width,
            height,
            scale,
            self.window.first_run.scroll(),
        ))
    }

    /// One press on the card, or the key that stands for one.
    pub(in crate::runtime) fn answer_first_run(&mut self, target: first_run::Target) -> Result<()> {
        self.window.first_run.press(target);
        match target {
            first_run::Target::Panel => return Ok(()),
            // **The whole row, and not the switch alone** (v4). The band that
            // lights under the pointer and carries the tooltip is the band that
            // answers the press; anything else is this window drawing a promise
            // it will not keep (§7.1.5f).
            first_run::Target::Row(index) => {
                if !self.window.first_run.flip(index) {
                    return Ok(());
                }
                if self.refresh_overlay() {
                    self.present_chrome_change()?;
                }
                return Ok(());
            }
            // **Both verbs go through the same door**, and the difference
            // between them is entirely in what comes back from
            // [`first_run`]. Spelling `Not now` as "do not call the applier"
            // would put the rule in a branch here as well as in that module,
            // and the two would be free to disagree.
            first_run::Target::Later => {
                let spent = first_run::declined();
                self.apply_first_run(&spent)?;
            }
            first_run::Target::Done => {
                let spent = self.window.first_run.done();
                self.apply_first_run(&spent)?;
            }
        }
        self.window.first_run.close();
        // Preserve the diagnostic override's ability to show another card
        // after a gesture. Normal launches remain gated by the stored answer.
        self.app.first_run_attempted = false;
        // **The card's anchors go out with the card.** While it was up this
        // window's whole tooltip list was its six rows (see
        // [`Self::rebuild_first_run_tip_anchors`]); leaving them standing would
        // leave six boxes of text hung on air, and rebuilding the chrome is
        // what puts the strip's own anchors back.
        self.note_tooltip(None)?;
        let chrome = self.refresh_chrome();
        if self.refresh_overlay() || chrome {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Spend what `Done` decided, **through the Settings dialog's own door**.
    ///
    /// Five of the six answers leave [`first_run`] as the press the Settings
    /// page sends, and are applied by handing that press to
    /// [`Self::apply_settings_choice`] — the same funnel, the same row readers,
    /// the same `apply_*` method. The card therefore cannot grow a second way to
    /// install anything, and a row that moves on the Settings page moves here
    /// without anybody remembering to come and look.
    ///
    /// **Success is silent** ([`Announce::OnlyFailures`]). The reader asked for
    /// these half a second ago and the Settings rows now read On; four success
    /// toasts stacked on a new install is noise. A failure still raises its own
    /// existing card, because a card that closed and pretended would be this
    /// product lying about a file it had just failed to touch.
    ///
    /// **A partial `Done` still closes.** Three rows that worked and one that
    /// did not is three rows that worked, and holding the card open over one
    /// toast-sized fact would make the reader dismiss it twice.
    fn apply_first_run(&mut self, spent: &[first_run::Application]) -> Result<()> {
        for application in spent {
            if let Some(target) = first_run::settings_target(*application) {
                self.apply_settings_choice_announcing(target, Announce::OnlyFailures)?;
            } else {
                // The one answer that is not a row: an intent about a `$PROFILE`
                // no shell has named yet.
                debug_assert_eq!(*application, first_run::Application::PowerShellIntent);
                self.record_powershell_install_pending(true);
            }
        }
        Ok(())
    }

    /// One key while the first-run card is up (§7.56 §6).
    ///
    /// **`Enter` presses `Done` from any switch**, which is the one place this
    /// card parts company with the PSReadLine invitation beside it: that dialog
    /// refuses `Enter` because its affirmative writes files and it can appear
    /// under somebody's hands mid-sentence. This one appears on a machine's
    /// first launch, with focus on a row nobody has touched and every row that
    /// writes anything switched off — so the `Enter` this card can receive by
    /// accident does what `Not now` does.
    ///
    /// Every other key is swallowed rather than typed into a shell behind a
    /// scrim.
    /// **The ring is lit by a key that does something, not by a key that
    /// arrives** (user ruling 2026-09-07: the ring was on the first switch
    /// before anybody had walked to it). This card swallows every key it does
    /// not act on, and a bare `Shift` is one of them — so lighting the ring on
    /// the way in put an accent ring round a control the reader had not
    /// touched, on the first surface a machine ever shows. Each arm below that
    /// moves, flips or scrolls lights it; `Tab` and the arrows light it by
    /// moving the focus, which is the same statement said once.
    pub(in crate::runtime) fn press_first_run_key(&mut self, key: &Key, shift: bool) -> Result<()> {
        let switches = self.window.first_run.rows().len();
        let focus = self.window.first_run.focus();
        match key {
            Key::Named(NamedKey::Escape) => {
                return self.answer_first_run(first_run::Target::Later);
            }
            Key::Named(NamedKey::Enter) => {
                return self.answer_first_run(match focus {
                    Some(first_run::Focus::Later) => first_run::Target::Later,
                    // From a switch, and from `Done` itself.
                    _ => first_run::Target::Done,
                });
            }
            // **A switch flipped by the keyboard keeps its ring.**
            // `answer_first_run` routes a *pointer* press, and a pointer press
            // is what puts the ring away — so a Space that went through it would
            // extinguish the very focus that answered it. The three other
            // targets close the card, so what their ring does afterwards is
            // moot and they go the ordinary way.
            Key::Named(NamedKey::Space) => {
                if let Some(first_run::Focus::Switch(index)) = focus {
                    self.window.first_run.light_the_ring();
                    self.window.first_run.flip(index);
                } else {
                    return self.answer_first_run(match focus {
                        Some(first_run::Focus::Later) => first_run::Target::Later,
                        Some(first_run::Focus::Done) => first_run::Target::Done,
                        _ => first_run::Target::Panel,
                    });
                }
            }
            Key::Named(NamedKey::Tab) => {
                if let Some(focus) = focus {
                    let next = first_run::stepped(focus, switches, !shift);
                    self.window.first_run.move_focus(next);
                }
            }
            Key::Named(NamedKey::ArrowDown | NamedKey::ArrowUp) => {
                if let Some(focus) = focus {
                    let down = matches!(key, Key::Named(NamedKey::ArrowDown));
                    let next = first_run::arrowed(focus, switches, down);
                    self.window.first_run.move_focus(next);
                }
            }
            // Off and on for the focused switch, the platform idiom.
            Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowRight) => {
                if let Some(first_run::Focus::Switch(index)) = focus {
                    let on = matches!(key, Key::Named(NamedKey::ArrowRight));
                    self.window.first_run.set(index, on);
                }
            }
            Key::Named(NamedKey::PageDown | NamedKey::PageUp | NamedKey::Home | NamedKey::End) => {
                if let Some(layout) = self.first_run_layout() {
                    self.window.first_run.light_the_ring();
                    let to = match key {
                        Key::Named(NamedKey::PageDown) => layout.scrolled_by(layout.page()),
                        Key::Named(NamedKey::PageUp) => layout.scrolled_by(-layout.page()),
                        Key::Named(NamedKey::Home) => 0.0,
                        _ => layout.scroll_extent(),
                    };
                    self.window.first_run.scroll_to(to);
                }
            }
            _ => return Ok(()),
        }
        // **A ring below the fold is a focus the reader cannot see**, so the
        // walk brings it back into view. Asked of a layout built after the move,
        // because where a row stands is a fact about the card as it now is.
        self.follow_first_run_focus();
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Scroll the body so that whatever has the ring is inside it.
    fn follow_first_run_focus(&mut self) {
        let Some(focus) = self.window.first_run.focus() else {
            return;
        };
        let Some(layout) = self.first_run_layout() else {
            return;
        };
        let to = layout.scroll_showing(focus);
        if self.window.first_run.scroll_to(to) {
            self.rebuild_first_run_tip_anchors();
        }
    }

    /// How tall one page of the card's body is, for the wheel's own travel rule.
    pub(in crate::runtime) fn first_run_page(&mut self) -> f32 {
        self.first_run_layout().map_or(0.0, |layout| layout.page())
    }

    /// One wheel notch over the card.
    pub(in crate::runtime) fn scroll_first_run(&mut self, delta: f32) -> Result<()> {
        let Some(layout) = self.first_run_layout() else {
            return Ok(());
        };
        if !layout.scrolls() {
            return Ok(());
        }
        let to = layout.scrolled_by(-delta);
        if self.window.first_run.scroll_to(to) {
            // The rows moved under a pointer that did not, so what is tippable
            // moved with them.
            self.rebuild_first_run_tip_anchors();
            if self.refresh_overlay() {
                self.present_chrome_change()?;
            }
        }
        Ok(())
    }

    /// One press on the invitation, or Esc.
    pub(in crate::runtime) fn answer_psreadline_invite(
        &mut self,
        target: restore::InviteTarget,
    ) -> Result<()> {
        match target {
            restore::InviteTarget::Panel => return Ok(()),
            restore::InviteTarget::Decline => {
                let next = psreadline::state_after_decline(
                    self.app.settings_store.loaded().psreadline_invite,
                );
                self.record_psreadline_invite(next);
            }
            restore::InviteTarget::Install => {
                self.apply_psreadline(true)?;
            }
        }
        self.window.psreadline_invite.close();
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Whether a PowerShell pane with no integration is offered one (§7.1.6j).
    ///
    /// **It reaches the panes that are already open**, which is what separates it from
    /// [`Self::apply_terminal_notifications`] two functions up: that switch is read at the moment
    /// a toast would be raised, and this one is read by a strip that is on the glass right now.
    /// Turning it off with a strip up and leaving the strip up would be a row that promises to
    /// stop asking while the question is still on screen; turning it on wants the offer back in
    /// the pane the reader is looking at rather than in the next one they open.
    ///
    /// [`Offer::Closed`] is not disturbed by either direction. A pane whose strip was dismissed
    /// with `×` said "not now" about that pane, and this row is about the asking in general —
    /// re-asking there on an unrelated press would be this switch answering a question it was not
    /// asked.
    pub(crate) fn apply_powershell_integration_offer(&mut self, enabled: bool) -> Result<()> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.powershell_integration_offer = enabled;
        if !enabled {
            settings.powershell_install_pending = false;
        }
        if !self.app.settings_store.store(settings) {
            return Ok(());
        }
        self.settle_pane_notices()
    }
}
