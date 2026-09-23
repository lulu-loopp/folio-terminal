//! `profiles` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{
    FilesFocusArrival, Popup, Runtime, cli, i18n, launch_wire, persist, profile_menu_anchor,
    profiles, seats, settings, shell_integration, text_field, toast,
};
use anyhow::Result;
use bt_layout::SeatId;
use bt_render::{FrameSource, FrameTrigger};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

impl Runtime<'_> {
    /// **Which profile a second launch's tab starts as, and what could not be
    /// honoured** (§7.59).
    ///
    /// Through [`cli::resolve`] and not through a rule of this door's own, which
    /// is the whole reason the request carries the id the caller *typed* rather
    /// than an index: `folio --profile fish` means the same thing whether or not
    /// a Folio was already running, including what it means when this build has
    /// no such profile — the default, and a card that says so.
    ///
    /// The folder is handed to [`Self::new_tab_with_profile`] in Windows' own
    /// namespace rather than the crossed one this returns beside the profile,
    /// because that door crosses it itself; what is taken from here is the
    /// profile and the list of things to say out loud.
    pub(crate) fn launch_profile(
        &self,
        request: &launch_wire::LaunchRequest,
    ) -> (String, Vec<cli::CliRefusal>) {
        let plan = cli::resolve(
            &cli::CliRequest {
                cwd: request.cwd.clone(),
                profile: request.profile.clone(),
                path: None,
                embedding: false,
                new_window: request.new_window,
                tab: request.tab,
                origin: request.origin,
            },
            self.default_profile(),
            cli::machine_path_kind,
        );
        (profiles::id(plan.profile), plan.refusals)
    }

    /// The picker's verb: a tab on the profile the row names, optionally
    /// standing somewhere the caller has already been told.
    ///
    /// The profile parameter is the whole of the difference between the `+` and
    /// a picker row, and it is here rather than deeper because this build
    /// launches one shell: routing it further would be a parameter carried
    /// through three call frames to be ignored at the end of them. What the door
    /// has to be is *open* — one entry point that takes which profile, so a
    /// second profile is a launcher and not a new path through the tab
    /// machinery.
    ///
    /// **`place` overrides the inheritance and does not merely feed it** (user
    /// ruling 2026-08-20, `New terminal in folder…`). `cwd_for_spawn` answers
    /// "where does a tab opened from *this* pane start", which is the right
    /// question for every other door and the wrong one for this one: the folder
    /// was named out loud in a dialog, and translating it out of the pane you
    /// happened to be looking at would let the pane overrule the answer. `None`
    /// is therefore "nobody said", and only then is the pane asked.
    ///
    /// It arrives in **the chooser's** namespace, which is Windows', and is
    /// crossed into the profile's here — the same crossing `cwd_for_spawn` makes
    /// on the other branch, because [`LeafSeed::cwd`] is by contract already
    /// written in the namespace of the profile that will be spawned. A profile
    /// that has no name for the chosen folder inherits nothing rather than a
    /// path it cannot read, which is `cwd_for_spawn`'s own rule and not a second
    /// one: it then starts where a fresh tab of that profile starts.
    pub(crate) fn new_tab_with_profile(
        &mut self,
        profile: &str,
        place: Option<PathBuf>,
    ) -> Result<()> {
        // **Both facts are read off the *same* leaf** — the focused session,
        // which is also what `working_directory()` is asked of. A profile taken
        // from one pane and a directory from another would be the exact mismatch
        // the rule exists to prevent. A tab with no shell reports no folder
        // (§7.1.6h), which `cwd_for_spawn` already has an answer for: a new tab
        // opened from a folder tab starts where a fresh one would.
        //
        // This wrapper is the whole of what "the pane you are looking at" means,
        // and it is the only thing that separates every existing door from the
        // one 丙2 added — see [`Self::new_tab_seeded_from`].
        let source_cwd = self
            .focused()
            .and_then(|leaf| leaf.session.working_directory().map(Path::to_path_buf));
        let source_profile = self.session_profile();
        self.new_tab_seeded_from(profile, place, &source_profile, source_cwd)
    }

    /// Write the line into this pane's `$PROFILE`.
    ///
    /// **The one write this product makes into a file that belongs to somebody
    /// else's shell**, and it happens here and only here: on a press, on the
    /// pane the offer is about, into the path that pane's own shell named. A
    /// copy of the file is taken first (`shell_integration::add_to_profile`).
    ///
    /// A write that fails leaves the strip exactly as it was, offering the same
    /// verb. There is nothing else honest to do: the reader pressed a word that
    /// says it will add a line, and a strip that changed to "added" over a file
    /// that did not change would be this product lying about a file it had just
    /// failed to touch.
    pub(crate) fn add_to_profile(&mut self, seat: SeatId) -> Result<()> {
        let Some(profile) = self
            .sessions
            .get(&seat)
            .and_then(|leaf| leaf.integration_offer.as_ref())
            .and_then(shell_integration::Offer::profile)
            .map(Path::to_path_buf)
        else {
            return Ok(());
        };
        match shell_integration::install_into_profile(&profile, SystemTime::now()) {
            Ok(written) => {
                match &written.backup {
                    Some(backup) => eprintln!(
                        "BT_SHELL_INTEGRATION wrote {} (copy kept at {})",
                        written.profile.display(),
                        backup.display()
                    ),
                    None => eprintln!("BT_SHELL_INTEGRATION wrote {}", written.profile.display()),
                }
                if let Some(leaf) = self.sessions.get_mut(&seat) {
                    leaf.integration_offer = Some(shell_integration::Offer::Added);
                }
                self.settle_pane_notices()
            }
            Err(error) => {
                eprintln!(
                    "BT_SHELL_INTEGRATION could not write {}: {error}",
                    profile.display()
                );
                Ok(())
            }
        }
    }

    /// Where the profile picker hangs right now, or `None` when it is shut.
    ///
    /// `&mut self` because the menu is content-sized and measuring a string goes
    /// through the renderer's font system, which shapes and caches as it goes.
    pub(crate) fn profile_menu_layout(&mut self) -> Option<profiles::ProfileMenuLayout> {
        if !self.window.profile_menu.is_open() {
            return None;
        }
        let now = Instant::now();
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        // The button that opened the menu is the button the menu hangs off, and
        // which surface holds it is [`profile_menu_anchor`]'s whole subject —
        // three geometries and a precedence rule, kept pure and kept out of
        // here, because the version that lived inline picked its answers in an
        // order that could return out of the function between them. See that
        // function's doc for the dead switch this shape exists to prevent.
        //
        // All three surfaces are measured before the choice is made rather than
        // measured inside the arm that wants them. That is what "pure" costs and
        // what it buys: every one of the three is a pure function of numbers this
        // window already has, and none of them can now abort the walk.
        let column = self
            .window
            .focus_mode
            .then(|| self.focus_rail_geometry_now(now))
            .flatten();
        let rail = self.rail_geometry_now(now);
        let strip = seats::tab_strip_geometry(
            width as f32,
            scale,
            self.platform_chrome(),
            &self.tab_trailers(now),
            self.window.active_tab,
            self.window.tab_scroll,
        );
        // The posture the three geometries above were measured against, not the
        // stored preference: `rail_geometry_now` and `focus_rail_geometry_now`
        // both read `sampled_rail(now)`, so anything else here would be asking
        // the walk to choose between boxes solved for a different window.
        let (anchor, side) = profile_menu_anchor(
            column.as_ref(),
            rail.as_ref(),
            strip.new_tab_menu,
            self.sampled_rail(now),
        )?;
        // The menu is content-sized, so laying it out is a measuring job — the
        // same renderer, the same font, the same call `build` makes when it
        // draws the strings this width was computed from.
        // The effective table, for the one row of this menu that is also a row
        // of it — see `ProfileMenuLayout::files_pane_accel`.
        let shortcuts = &self.app.shortcuts;
        let recent = self.app.recent.entries();
        // The machine, taken beside the two above and for their reason: it is
        // read here as well as at the draw because since 2026-08-29 it decides
        // which rows the list has (`ProfileTable::offered_to_start`).
        let programs = &self.app.profile_programs;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        Some(profiles::layout(
            anchor,
            side,
            programs,
            (width as f32, height as f32),
            scale,
            recent,
            shortcuts,
            &mut measure,
        ))
    }

    /// Which profile the `+` starts, the window is titled after and the picker
    /// marks `default` — the user's setting, resolved against this machine.
    ///
    /// One reader, called by all four, and that is the point of it being a method
    /// rather than a field: the setting can change mid-session and a field would
    /// have to be refreshed by whoever wrote it, in every place they wrote it.
    /// [`profiles::default_profile`] is cheap — a walk of four entries and an
    /// array index — and none of these callers is a hot path.
    pub(crate) fn default_profile(&self) -> usize {
        profiles::default_profile(
            &self.app.settings_store.loaded().default_profile,
            &self.app.profile_programs,
        )
    }

    /// The same answer in the spelling a seed takes it in (T-PROFILE-TABLE-MOVE).
    ///
    /// The resolution itself is a question about *this* table and is asked here,
    /// against the table as it stands; what leaves this function is the row's
    /// stable id, because everything downstream of a new-tab door holds its
    /// profile across at least one gesture — a folder chooser, a tear-out, a
    /// save — and a position does not survive one.
    pub(crate) fn default_profile_id(&self) -> String {
        profiles::id(self.default_profile())
    }

    /// Whether that answer came from the machine rather than from the reader —
    /// the Profiles page's badge, and nothing else asks.
    ///
    /// A second call rather than one function answering both, for the reason
    /// above: this is derived where it is drawn, and the two readings cannot
    /// drift because [`profiles::default_profile_is_automatic`] and
    /// [`profiles::default_profile`] ask the same private rule.
    pub(crate) fn default_profile_is_automatic(&self) -> bool {
        profiles::default_profile_is_automatic(
            &self.app.settings_store.loaded().default_profile,
            &self.app.profile_programs,
        )
    }

    /// The `˅`'s verb: show the profile list, or put away the one on screen.
    ///
    /// **E61's rule stated in both directions.** The root menu's opener has
    /// always closed this one; this one did not close the root menu, and got
    /// away with it only because the press that opens it also falls through the
    /// root menu's outside-press handler on its way here. That is precisely the
    /// "mutual exclusion left to a press falling through" the mock-up's own
    /// judgement forbids — it holds until some other door (a chord, a menu row,
    /// a restored gesture) opens this menu without a press in the right place.
    pub(crate) fn toggle_profile_menu(&mut self) -> Result<()> {
        self.close_popups_except(Popup::Profile);
        self.window.profile_menu.toggle();
        self.start_chevron_turn();
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Put the picker away and repaint if it was up. Every press that is not the
    /// chevron's and not the menu's own goes through here first, exactly as the
    /// mock-up's document-level `click` handler does.
    pub(crate) fn close_profile_menu(&mut self) -> Result<bool> {
        if !self.window.profile_menu.is_open() {
            return Ok(false);
        }
        // **Through [`Self::close_popup`] rather than past it** (§7.1.6e″). This
        // used to shut the picker itself and turn the arrow itself, which is a
        // second spelling of the one arm E61 exists to keep single — and the day
        // that arm grew a third thing to do (re-asking the rail's zone, because
        // the panel this list was holding out may now have to park) was the day
        // the copy started being wrong. The arrow still turns: that is
        // `close_popup`'s own `Profile` arm, unchanged.
        self.close_popup(Popup::Profile);
        // The gate goes with it. A grace still running against a menu that has
        // already gone would fire a second close on an empty state — harmless
        // today, and exactly the kind of live clock that stops being harmless
        // when a third chevron is added.
        self.window.chevrons.profile.clear();
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// One press on a row's arrows: move it up, or move it down.
    ///
    /// **Both leave through [`Self::store_profiles`]**, which is the one place
    /// the file, the programs probe and the window's measurements are moved
    /// together. A verb that wrote the table and forgot one of the three would be
    /// a list that changed on screen and not on disk, or a duplicate whose
    /// program had never been looked for and which therefore drew itself greyed.
    pub(crate) fn apply_profile_action(&mut self, action: settings::ProfileAction) -> Result<()> {
        let moved = match action {
            settings::ProfileAction::MoveUp(index) => profiles::move_profile(index, false),
            settings::ProfileAction::MoveDown(index) => profiles::move_profile(index, true),
        };
        if !moved {
            return Ok(());
        }
        self.store_profiles()
    }

    /// Open the editor sub-page on one profile, seeded from the table.
    ///
    /// **Seeded and not bound**: the fields hold text and the table holds the
    /// answer, and every keystroke that the table will take is pushed straight
    /// through to it (§7.1.6c-4a — this dialog writes on change, there is
    /// nothing to save). What the fields exist for is the moment in between,
    /// where a name is half typed and a path is being pasted.
    pub(crate) fn open_profile_editor(&mut self, index: usize) -> Result<()> {
        if index >= profiles::count() {
            return Ok(());
        }
        let program = profiles::program_text(index, self.app.profile_programs.row_program(index));
        self.window.settings.open_editor(settings::ProfileEditor {
            index,
            name: text_field::TextField::holding(&profiles::display_title(index)),
            program: text_field::TextField::holding(&program),
            args: text_field::TextField::holding(&profiles::join_arguments(&profiles::args(index))),
            env: profiles::env(index)
                .into_iter()
                // The three the terminal fills in are not here: they are what it
                // says to every session, and a copy of them in editable state is
                // the invisible second layer plan §1.7 exists to abolish.
                .map(|(name, value)| {
                    (
                        text_field::TextField::holding(&name),
                        text_field::TextField::holding(&value),
                    )
                })
                .collect(),
            refusal: None,
        });
        self.window.settings_scroll = 0.0;
        Ok(())
    }

    /// Take one profile out of the table, and put a card up that can put it back.
    ///
    /// **Immediate, with an undo, and no confirmation** (plan §2.3): this dialog
    /// has no dirty gate to route a question through and every choice in it is
    /// written the instant it is made, so what an irreversible one is owed is a
    /// way back rather than a second question.
    ///
    /// The card names one fact and it is a fact this window holds: how many panes
    /// in it are running the profile, and that they keep running. It does not
    /// count seeds on disk — a number read out of a session file at delete time
    /// can be wrong by the day it matters, and the honest place for that is the
    /// degrade banner the restarting seat already prints.
    pub(crate) fn delete_profile(&mut self, index: usize) -> Result<()> {
        let title = profiles::title(index).to_owned();
        // **The row's id, read before the table moves**, because that is what the
        // panes are holding: counting them by position would count whichever rows
        // happen to sit where this one sat, and after the delete there is no
        // position left to ask about at all.
        let subject = profiles::id(index);
        let panes = self
            .sessions
            .values()
            .filter(|leaf| leaf.profile == subject)
            .count();
        let Some(removed) = profiles::delete(index) else {
            return Ok(());
        };
        // The editor was standing on the row that has gone, so the page it was
        // showing is not there any more. Back to the list, which is where a
        // reader who has just deleted something is looking.
        if self
            .window
            .settings
            .editor()
            .is_some_and(|editor| editor.index == index)
        {
            self.window.settings.close_editor();
        }
        self.store_profiles()?;
        let id = self.toast_with_verb(
            toast::ToastKind::Info,
            toast::ToastAnchor::Window,
            i18n::profile_deleted(&title, panes),
            i18n::Text::ProfilesUndo.text(),
        )?;
        self.window.profile_undo = Some((id, removed, index));
        Ok(())
    }

    /// The card's verb: put the row back where it was, with its own id.
    ///
    /// A verb pressed on a card that is no longer the one holding the undo does
    /// nothing — see [`WindowRuntime::profile_undo`].
    pub(crate) fn take_profile_undo(&mut self, card: toast::ToastId) -> Result<()> {
        let Some((id, profile, at)) = self.window.profile_undo.take() else {
            return Ok(());
        };
        if id != card {
            self.window.profile_undo = Some((id, profile, at));
            return Ok(());
        }
        profiles::reinsert(profile, at);
        self.store_profiles()
    }

    /// Write the table to `profiles.json` and re-probe what it can start.
    ///
    /// **Three things move together and are moved in one place**: the file, the
    /// programs probe (a duplicate is a new row whose executable has never been
    /// looked for) and the window's own measurements, which follow
    /// `profiles::profile_revision` through `LayoutKey`. Every verb in this
    /// window that changes the table calls this and nothing else does.
    ///
    /// The sentence used to read *anything* that changes the table, and
    /// §7.1.6c-6d retired that word rather than let it rot: the table can now
    /// also change because somebody edited the file, and a change that arrived
    /// **from** the file has no write to make. What it does owe is the other
    /// two, which is why they are [`Self::adopt_profile_table`] and are called
    /// from both.
    pub(crate) fn store_profiles(&mut self) -> Result<()> {
        self.app.profiles_store.store(profiles::to_file());
        self.adopt_profile_table()
    }

    /// The two of those three that a table which arrived **from** the file owes
    /// as well (§7.1.6c-6d).
    ///
    /// `reread_profiles` has no write to make — the file is where its table came
    /// from — and everything else `store_profiles` does it owes for the same
    /// reasons, which is why they are one call rather than two lists that have
    /// to be kept in step.
    fn adopt_profile_table(&mut self) -> Result<()> {
        self.app.profile_programs =
            profiles::ProfilePrograms::probe(&bt_pty::SystemShellEnvironment);
        self.app.first_run_attempted = false;
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })
    }

    /// Say once, on the window, that `profiles.json` could not be used whole.
    ///
    /// [`Self::announce_keybindings_fault`]'s twin and for its reason: the
    /// fallback is invisible by construction — the profiles simply are the ones
    /// this build ships — so a user whose file was refused has no other way to
    /// find out.
    pub(crate) fn announce_profiles_fault(&mut self) -> Result<()> {
        let Some(fault) = self.app.profiles_fault.take() else {
            return Ok(());
        };
        self.toast(
            toast::ToastKind::Error,
            toast::ToastAnchor::Window,
            None,
            fault,
        )
    }

    /// Point "Default profile" at the profile called `id` (mock-up 7708-7712).
    ///
    /// Written to disk immediately, for the reason `apply_display_formulas`
    /// gives, and with the same one-line body — this is a preference and not a
    /// verb, so nothing is re-spawned, re-scanned or reopened. **What is already
    /// on screen does not move.** A tab is the shell it is running; changing what
    /// the `+` will start next is not a claim about the ones already started, and
    /// a setting that retro-actively converted live panes would be the same
    /// mistake as a leaf silently adopting the current default off disk.
    ///
    /// What *does* move within the frame is everything that merely *names* the
    /// default: the `+`'s tooltip and the picker's `default` hint both read
    /// [`Self::default_profile`] on each build rather than caching it, so a
    /// change is drawn on the next paint — mock-up 4293's `stripIds()` folding
    /// `state.defaultProfile` into the strip's rebuild key, arrived at by not
    /// having a cache to invalidate.
    ///
    /// The OS window title is deliberately *not* in that list. It is the active
    /// tab's name, whose last layer is the focused pane's own profile title
    /// ([`TabState::focused_profile_title`]) — the shell that is running, not the
    /// one that would be started next.
    pub(crate) fn apply_default_profile(&mut self, id: &str) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.default_profile = id.to_owned();
        if !self.app.settings_store.store(settings) {
            return Ok(false);
        }
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        Ok(true)
    }

    /// Read the file again, and put in force whatever that changed.
    ///
    /// **What follows the change, and what deliberately does not.**
    ///
    /// * **The lists follow, because they are never held.** The Profiles page's
    ///   rows and the `Default profile` picker's options are derived in
    ///   `settings_content` from the table every time the dialog is built, so
    ///   installing a new table *is* refreshing them. The same is true of the
    ///   `˅` menu, the pane submenu and every tab drawing its profile's name:
    ///   `profiles::profile_revision` is in `LayoutKey`, so the widths measured
    ///   from those names are re-measured rather than reused.
    /// * **A session already running does not follow.** A profile is a birth
    ///   certificate and not a contract: the program and the environment of a
    ///   shell that is already up are in a process, and nothing on this side of
    ///   the pipe can re-argue them. What each pane *does* follow is its own
    ///   profile **by id**, and it follows it by construction now rather than by
    ///   a pass made here: a leaf holds the id, not a position in a table
    ///   somebody may have just reordered in a text editor. A pane whose row has
    ///   been removed from the file keeps its id, goes on running the shell it
    ///   started, and meets the standing answer for a profile that is gone — the
    ///   fallback, and never the reader's configured default — the next time it
    ///   is asked to start something (`startable_profile`).
    /// * **The editor sub-page keeps its draft and follows its subject**, for
    ///   the reason a scheme row follows its file: the reader is looking at the
    ///   thing that just changed, and a page that reseeded its fields would be
    ///   this window deleting what somebody is halfway through typing. So the
    ///   index is re-pointed at the same id, the text fields are not touched,
    ///   and the next keystroke writes the whole table back — the person typing
    ///   wins the field they are typing in, which is what "last writer wins"
    ///   means when one of the writers is a hand on a keyboard. When the id is
    ///   gone the page goes with it, exactly as it does when the row is deleted
    ///   from the dialog itself.
    /// * **No `Undo` card for a row that vanished.** `Undo` is the verb of a
    ///   deletion *this window* performed and the card exists to offer the way
    ///   back it took away; an editor's deletion is a fact, and its way back is
    ///   the file.
    pub(crate) fn reread_profiles(&mut self) -> Result<()> {
        match self.app.profiles_store.reread() {
            persist::ProfilesNews::Unchanged => {
                self.app.profiles_file_broken = false;
                return Ok(());
            }
            // The colours-do-not-move ruling, one file over: what is in force
            // stays in force and one card says the file could not be read. The
            // reader is looking at their own half-typed JSON, and emptying their
            // profile list *because* of it would be the window fighting them.
            persist::ProfilesNews::Unreadable => {
                if !std::mem::replace(&mut self.app.profiles_file_broken, true) {
                    self.toast(
                        toast::ToastKind::Error,
                        toast::ToastAnchor::Window,
                        None,
                        i18n::profiles_file_kept(persist::PROFILES_FILE_NAME),
                    )?;
                }
                return Ok(());
            }
            persist::ProfilesNews::Changed => self.app.profiles_file_broken = false,
        }
        // Taken by id *before* the table moves, because after it has moved
        // there is nothing left to ask: an index means whatever the new table
        // says it means.
        let editing = self
            .window
            .settings
            .editor()
            .map(|editor| profiles::id(editor.index));
        // **Nothing to re-point on the panes** (T-PROFILE-TABLE-MOVE). There used
        // to be a capture of every leaf's id here and a second pass putting the
        // new positions back on them, and it was the only by-id re-resolution in
        // the product — which is why every *other* way the table moves (this
        // dialog's own `Move up`, `Delete`, `Duplicate`, `Undo`) left the panes
        // naming whichever row had slid into their slot. A leaf holds the id now,
        // so a table that moves cannot move a pane's profile at all, and the loop
        // that used to heal one door of four is a loop with nothing left to do.
        let faults = profiles::install(self.app.profiles_store.loaded());

        if let Some(id) = editing {
            match profiles::table().position_of_id(&id) {
                Some(index) => {
                    if let Some(editor) = self.window.settings.editor_mut() {
                        editor.index = index;
                    }
                }
                None => self.window.settings.close_editor(),
            }
        }
        // A row menu names a row by its index and the rows have just moved under
        // it; there is no id to follow it by, because it is not a place — it is
        // a gesture, and the gesture was made against a list that is gone.
        self.window.settings.close_row_menu();
        // The dialog may be standing on a page, a row or a picker that the new
        // table does not have. Same call the verbs of this dialog make after
        // they move the list, for the same reason — and **before** the frame
        // below, because the frame is what draws the ring.
        let (rows, shortcuts, profile_lines, scheme_files, values) = self.settings_content();
        let content =
            self.settings_dialog(&rows, &shortcuts, &profile_lines, &scheme_files, &values);
        self.window.settings.keep_focus_reachable(content);
        self.adopt_profile_table()?;
        // One card per refused entry, `report_skipped_schemes`' shape: a file
        // with two bad rows is two things to fix. They are the same sentences
        // startup prints, because it is the same reader reading the same file.
        for fault in faults {
            let sentence = i18n::profile_entry_fault(&fault);
            eprintln!("BT_PERSIST {}: {sentence}", persist::PROFILES_FILE_NAME);
            self.toast(
                toast::ToastKind::Error,
                toast::ToastAnchor::Window,
                None,
                sentence,
            )?;
        }
        Ok(())
    }

    /// **The column this menu hangs inside and the places it would offer** — or
    /// `None`, which means it draws nothing.
    ///
    /// [`Self::preview_menu_stand`]'s sentence for the root menu, and the same
    /// two readers: the layout below and [`Self::popups_up`]. The anchor is not
    /// among the answers because this one's anchor cannot vanish — the button
    /// falls back to the caption it wraps — so what is left to decide is whether
    /// the column is on the glass at all and whether there is anywhere to go.
    pub(crate) fn root_menu_stand(
        &self,
        seat: SeatId,
    ) -> Option<([f32; 4], Vec<profiles::RootChoice>)> {
        let is_a_column =
            self.seat_layout.rects.iter().any(|placement| {
                placement.id == seat && placement.kind == bt_layout::SeatKind::Files
            });
        if !is_a_column {
            return None;
        }
        let rect = seats::full_pane_rect(&self.seat_layout, seat)?;
        let choices = self.root_choices(seat);
        (!choices.is_empty()).then_some((rect, choices))
    }

    /// The root menu's box this frame, or `None` when it is shut.
    ///
    /// **Laid out against the live head, every frame** (E59/E60). The menu
    /// floats over the panes while its anchor lives inside one, and a pane that
    /// moved — a divider dragged, a sibling closed, the window resized — leaves
    /// a menu pointing at where its button used to be. Re-deriving it from the
    /// current layout is the fix the mock-up arrived at after the same bug three
    /// times, and it makes the second half free: an anchor that has gone folds
    /// the menu instead of measuring a rectangle that is no longer anywhere.
    pub(crate) fn root_menu_layout(&mut self) -> Option<profiles::RootMenuLayout> {
        let seat = self.window.root_menu.seat()?;
        let (rect, choices) = self.root_menu_stand(seat)?;
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let names = self.files_names();
        let widths = self.measure_files_names(&names);
        let head = seats::pane_head_geometry(
            rect,
            bt_layout::SeatKind::Files,
            self.seat_layout.seat_is_on_stage(seat),
            scale,
        );
        // **The button when the head seats one, else the caption it wraps** —
        // [`Self::preview_menu_stand`]'s rule, for the other menu that hangs off
        // a name: a column narrowed under an open menu loses the button
        // (`seats::files_root_box` gives it up rather than drawing half of it),
        // and a menu that folded there would be a window keeping the keyboard
        // over an empty glass.
        let anchor = seats::files_root_box(&head, scale, widths.get(&seat).copied().unwrap_or(0.0))
            .unwrap_or(head.title);
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        Some(profiles::root_menu_layout(
            anchor,
            (width as f32, height as f32),
            scale,
            &choices,
            &mut measure,
        ))
    }

    /// The button on the head: open the menu here, or shut it if it is already
    /// here (E57).
    pub(crate) fn toggle_root_menu(&mut self, seat: SeatId) -> Result<()> {
        // A popup opening closes whatever else was up, and it has to be the
        // opener that does it: E61's judgement is that mutual exclusion cannot
        // be left to a press falling through, because every opener stops its own
        // press from travelling.
        self.close_popups_except(Popup::Root);
        self.window.root_menu.toggle(seat);
        // **The disk is asked once per look** (user ruling 2026-09-05). The
        // remembered folders are the only rows on this list that can name
        // somewhere that has gone, and "has it gone" is a question about the
        // machine now rather than about what happened — so it is asked here,
        // where the reader is about to read the answer, and not in the store,
        // which would then be a memory that changed by itself. Once per gesture
        // and not once per frame: a menu that stat'ed a dead network share
        // sixty times a second would hang the window it is drawn on.
        self.app
            .recent_folders
            .refresh(&|path| std::fs::metadata(path).is_ok_and(|meta| meta.is_dir()));
        // Pressing the head is also how you say "type here", and the column is
        // somewhere you can type — so the button lends the keyboard exactly as
        // the tree below it does, ringless for the same reason: it is a press.
        self.set_files_keyboard(Some(seat), FilesFocusArrival::Pointer);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    pub(crate) fn close_root_menu(&mut self) -> Result<bool> {
        if !self.window.root_menu.close() {
            return Ok(false);
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }
}
