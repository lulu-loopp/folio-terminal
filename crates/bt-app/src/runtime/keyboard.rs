//! `keyboard` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::TextStep;
use crate::{
    ImeCaretSource, ImeOwner, KeyboardOwner, LeafId, NewWindowPlan, NoticeHost, PreviewSurface,
    RenameClipboard, RenameExit, RenameVerdict, Runtime, Step, UserInputKind, composing_event_of,
    composition_ruling, diagnostics, file_menu_powers, git_graph, goto_tab_index, hang_watch,
    ime_caret_source, ime_commit_bytes, ime_cursor_area_of, ime_outbound, ime_owner, ime_report,
    input, keyboard_owner_is_a_shell, keyhint, marks, native_window, paste_card_key,
    popup_takes_the_key, preedit_caret_byte, profiles, quit, recoverable_clipboard_write,
    rename_key, rename_pastes, restore, settings, settings_key_of, shortcuts, toast,
    window_ime_cursor_area, write_pty_input, write_terminal_clipboard_text,
};
use anyhow::Result;
use bt_layout::{Axis, SeatId};
use bt_render::{FrameSource, FrameTrigger, ImeCursorArea, Preedit};
use bt_viewport::ViewportFrame;
use std::time::Instant;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{Ime, KeyEvent};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

impl Runtime<'_> {
    /// **The hint card's own layer**, or nothing when no hold is being answered
    /// (§7.1.5e′).
    ///
    /// The lines are read out of the effective table on *this* frame rather than
    /// remembered from the frame the card appeared on — the tip's own rule
    /// (`el.title` is rewritten on every paint), and here it earns its keep
    /// twice: a chord recorded in the settings dialog takes effect on the next
    /// press, and the scope a row is in force in moves with the keyboard.
    pub(in crate::runtime) fn key_hint_layer(&mut self) -> marks::Band {
        // Recorded at the end and only on the path that paints, so the
        // frame-debt comparison is against what is *on screen* — the tip's own
        // note, one surface over.
        self.window.key_hint_drawn_opacity = None;
        let now = Instant::now();
        // **A card at nought is still drawn**, which is the tip's own answer to
        // the same instant: the frame a fade starts on has an opacity of exactly
        // zero, and a layer skipped there would leave the debt below unpayable
        // for one turn and spend a `WaitUntil` on it. A transparent layer costs
        // one draw of nothing; a spin costs the loop's sleep.
        let Some((held, opacity)) = self.key_hint_on_screen(now) else {
            return marks::Band::default();
        };
        let lines = self.app.shortcuts.hint_lines(held, self.shortcut_focus());
        let caps = shortcuts::live_caps(held);
        let scale = self.window.renderer.scale_factor() as f32;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let Some(layout) = keyhint::place(
            &lines,
            &caps,
            (width as f32, height as f32),
            scale,
            &mut |run, size| renderer.measure_chrome_text(gpu, run, size),
        ) else {
            return marks::Band::default();
        };
        let palette = bt_render::chrome_palette();
        self.window.key_hint_drawn_opacity = Some(opacity);
        keyhint::build(&layout, &palette, scale, opacity)
    }

    /// **Whether this window would answer a hand holding `modifiers`**
    /// (§7.1.5e′).
    ///
    /// Three conditions and they are three different sentences:
    ///
    /// * **The setting.** One row on the General page, default on.
    /// * **Whether a chord would fire at all.** `keyboard_input` answers a
    ///   modal, a gate, a rename and every menu long before `Shortcuts::lookup`
    ///   is asked, so with any of them up the whole table is out of force — and a
    ///   card listing verbs that would not fire is a card telling the reader
    ///   something untrue. Asked of [`Self::keyboard_owner`], which is the
    ///   window's one reading of who has the keys, rather than of a second list
    ///   beside it.
    /// * **Whether *this* hold has anything to say**, which is the same question
    ///   the card's own contents answer. It is here rather than left to
    ///   `keyhint::place`'s empty case for a reason that is not tidiness: a hold
    ///   that was armed, came due and then drew nothing would leave a host
    ///   reporting a card the paint never recorded, the frame debt below would
    ///   never settle, and `ControlFlow::WaitUntil` would be handed an instant
    ///   already in the past on every turn — the pinned definition of a loop that
    ///   never sleeps. A hold with nothing to say is never armed, so it costs no
    ///   wake-up at all.
    ///
    /// The window merely being unfocused is deliberately *not* here: winit stops
    /// reporting modifiers with the window, so a hold cannot survive a blur in
    /// the first place, and `Focused(false)` takes the card down explicitly.
    fn key_hints_offered(&self, modifiers: ModifiersState) -> bool {
        if !self.app.settings_store.loaded().key_hints {
            return false;
        }
        let owner = self.keyboard_owner();
        if owner.rename || owner.git_prompt || owner.menu_or_dialog {
            return false;
        }
        !self
            .app
            .shortcuts
            .hint_lines(modifiers, self.shortcut_focus())
            .is_empty()
    }

    /// The hold this window is answering right now and how solid its card is —
    /// **what should be on the glass**, which is the half of the frame-debt
    /// question the paint cannot be trusted to have recorded.
    ///
    /// The offer is re-asked rather than assumed from the promotion: the
    /// keyboard can move to a surface where the hold says nothing while the card
    /// is still standing, and this is what makes that a card that is no longer
    /// showing rather than a debt nothing can pay.
    fn key_hint_on_screen(&self, now: Instant) -> Option<(ModifiersState, f32)> {
        let held = self.window.key_hint.active()?;
        self.key_hints_offered(held)
            .then(|| (held, self.window.key_hint.opacity(now, self.app.motion)))
    }

    /// Whether the card on screen differs from the card last painted.
    fn key_hint_owes_frame(&self, now: Instant) -> bool {
        self.window.key_hint_drawn_opacity != self.key_hint_on_screen(now).map(|(_, it)| it)
    }

    /// When this window next has hint work: the 800ms while a hold is settling,
    /// the fade's frames until it lands — and nothing at all for a window whose
    /// hands are empty.
    pub(in crate::runtime) fn key_hint_deadline(&self, now: Instant) -> Option<Instant> {
        let next_frame = self.next_animation_deadline();
        if self.key_hint_owes_frame(now) {
            return next_frame;
        }
        let owner =
            self.window
                .key_hint
                .deadline(now, self.app.motion, self.window.frame_clock.interval());
        if self.window.key_hint.is_fading(now, self.app.motion) {
            next_frame
        } else {
            owner.map(|deadline| self.clamp_animation_deadline(deadline))
        }
    }

    /// Note what the modifiers are now, and repaint if the answer moved a card.
    pub(crate) fn note_key_hint(&mut self, now: Instant) -> Result<()> {
        let offered = self.key_hints_offered(self.window.modifiers);
        if self
            .window
            .key_hint
            .observe(self.window.modifiers, offered, now)
            && self.refresh_overlay()
        {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Show a hold whose 800ms has run out, and keep paying the fade's frames
    /// until it lands.
    ///
    /// **A hold that has come due with nothing to say is still promoted**, and
    /// the card simply is not drawn ([`keyhint::place`] answers `None`). The
    /// alternative — refusing to promote — would leave the deadline armed and
    /// already in the past, which is the `WaitUntil` pin's own definition of a
    /// loop that never sleeps.
    pub(in crate::runtime) fn advance_key_hint_if_due(&mut self, now: Instant) -> Result<()> {
        // The tip's own arrangement, for the tip's own reason (closure review
        // O4, 2026-09-18): the eight hundred milliseconds maturing is state and
        // is never paced, and only the fade that follows it is.
        let promoted = self.window.key_hint.activate_if_due(now);
        if !promoted && !self.animation_frame_is_due() {
            return Ok(());
        }
        if (promoted || self.key_hint_owes_frame(now)) && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// **The hand answered, so the question is spent** (§7.1.5e′, user ruling
    /// 2026-08-25).
    ///
    /// One function and three doors — a key that is not a modifier, a notch of
    /// the wheel, a button going down — because "what spends a hold" is one
    /// sentence and three copies of it are three chances for the next gesture to
    /// be added to two of them. The card was born knowing only about the
    /// keyboard, and the report that ended that is `Ctrl`+wheel zooming a page:
    /// a hold being answered over and over while the card hangs there insisting
    /// the hand has forgotten something.
    ///
    /// **The bare-modifier exemption is not here**, and that is deliberate: it
    /// belongs to the keyboard, which is the only door at which a modifier can
    /// *be* the gesture. A wheel notch and a mouse button are never one, so a
    /// guard repeated at those two doors would be a guard that can never fire
    /// and a reader would have to work out why.
    ///
    /// It answers nothing and consumes nothing — there is no path from here that
    /// can stop an event — which is how "the hint never takes a gesture" stays
    /// structural rather than remembered.
    pub(in crate::runtime) fn spend_key_hint(&mut self) -> Result<()> {
        if self.window.key_hint.spend(self.window.modifiers) && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Take the card down without closing the offer — the window losing focus, a
    /// menu opening. See [`keyhint::KeyHintHost::hide`].
    fn hide_key_hint(&mut self) -> Result<()> {
        if self.window.key_hint.hide() && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// **Everything a shortcut verb does to the process**, whichever door it
    /// came through.
    ///
    /// One function for the pointer and the keyboard alike, for
    /// `apply_settings_choice`'s reason: a verb reachable two ways whose body
    /// lives on one of them is a verb that half works. The file is written here
    /// and nowhere else, so "the table changed" and "the disk knows" cannot come
    /// apart.
    pub(crate) fn apply_shortcut_edit(&mut self, target: settings::SettingsTarget) -> Result<()> {
        let lines = self.app.shortcuts.editor_rows();
        match target {
            settings::SettingsTarget::RestoreRow(index) => {
                let Some(line) = lines.get(index) else {
                    return Ok(());
                };
                for id in &line.ids {
                    self.app.shortcuts.restore(id);
                }
            }
            // **No confirmation, and that is the ruling** (2026-08-17). This
            // dialog has no dirty gate to route one through — every choice in it
            // is written the instant it is made — so a question here would be
            // the product's first modal over a modal and would owe §7.1.5's Esc
            // ladder a rung of its own. What it discards is exactly the table
            // the user is looking at while they press it, and the file it
            // rewrites is the one file this product invites them to keep open in
            // an editor.
            settings::SettingsTarget::RestoreAll => self.app.shortcuts.restore_all(),
            _ => return Ok(()),
        }
        self.store_keybindings();
        let (rows, shortcuts, profile_lines, scheme_files, values) = self.settings_content();
        let content =
            self.settings_dialog(&rows, &shortcuts, &profile_lines, &scheme_files, &values);
        self.window.settings.keep_focus_reachable(content);
        Ok(())
    }

    /// Write the table's departures to `keybindings.json`.
    ///
    /// The departures and not the table: [`shortcuts::Shortcuts::overrides`]
    /// derives them by walking the effective table beside the defaults, so a row
    /// a user has put back leaves no line behind and a later build is free to
    /// retune a chord nobody touched.
    pub(crate) fn store_keybindings(&mut self) {
        let overrides = self
            .app
            .shortcuts
            .overrides()
            .into_iter()
            .map(|entry| bt_persist::BindingOverrideV1 {
                action: entry.id,
                chord: entry.chord,
            })
            .collect();
        self.app.keybindings_store.store(overrides);
    }

    /// Say once, on the window, that `keybindings.json` could not be used.
    ///
    /// **A notice and not a silent fallback**, because the fallback is invisible
    /// by construction: the shortcuts simply work the way they always did, and a
    /// user whose customisations have quietly stopped applying has no way at all
    /// to find out. `Error` rather than `Info` for the same reason §7.1.6h gives
    /// the kind to Git's refusals — something the user asked for did not happen.
    pub(crate) fn announce_keybindings_fault(&mut self) -> Result<()> {
        let Some(fault) = self.app.keybindings_fault.take() else {
            return Ok(());
        };
        self.toast(
            toast::ToastKind::Error,
            toast::ToastAnchor::Window,
            None,
            fault,
        )
    }

    /// **Whether a held modifier raises the hint card** (§7.1.5e′).
    ///
    /// The key is stored and nothing else is invalidated, because nothing else
    /// holds a copy: the card is derived from the effective shortcut table and
    /// this key on the frame it is drawn, and the key is read through
    /// [`Self::key_hints_offered`] at every door. It is the application's
    /// setting, so the other windows learn it the way they learn all of them —
    /// see `settle_application_change` — and this window learns it on the next
    /// turn, where `note_key_hint` takes down a card the reader has just
    /// switched off.
    pub(crate) fn apply_key_hints(&mut self, enabled: bool) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.key_hints = enabled;
        if !self.app.settings_store.store(settings) {
            return Ok(false);
        }
        // A card standing over the row that has just switched it off is the one
        // frame this change must not leave on the glass, so it is taken here
        // rather than waited for.
        if !enabled {
            self.hide_key_hint()?;
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// Carry out one row of the shortcut registry.
    ///
    /// Every arm is a call into a verb that already exists for the pointer to
    /// reach: a shortcut is a second door onto the same room, never a second
    /// implementation of it.
    pub(crate) fn run_shortcut(&mut self, action: shortcuts::Action) -> Result<()> {
        match action {
            // I86: the keyboard's new tab is the default profile's. Choosing a
            // different one is the strip's `+` menu, which is where the choice
            // is visible.
            shortcuts::Action::NewTab => self.new_tab(),
            // **A debt recorded, not a window opened** (multiwindow slice C).
            // Opening one needs the `ActiveEventLoop`, which lives for the
            // duration of a handler callback and has never been reachable from
            // here; the loop spends this at its own door, exactly as it spends
            // the dirty gate's request to shut. See [`App::pending_new_windows`].
            shortcuts::Action::NewWindow => {
                let id = self.window.window.id();
                self.app.pending_new_windows.push(NewWindowPlan::fresh(id));
                Ok(())
            }
            // **A debt recorded, for the reason above it and one more**
            // (multiwindow slice E2). Opening a window needs the event loop;
            // *quitting* needs every window at once, and a `Runtime` is one
            // window by construction. The loop spends this at its own door — see
            // [`FolioApp::settle_quit`].
            shortcuts::Action::Quit => {
                self.app.ask_to_quit();
                Ok(())
            }
            // I103's chain lives inside `close_pane`: the last pane of a tab
            // closes the tab, and the last tab hands off to the window's own
            // shut flow rather than leaving an empty window behind.
            shortcuts::Action::ClosePane => self.close_pane(self.focused_leaf),
            shortcuts::Action::NextTab => self.step_tab(true),
            shortcuts::Action::PrevTab => self.step_tab(false),
            shortcuts::Action::GotoTab(ordinal) => {
                // Out of range is ignored, not clamped: Ctrl+Shift+9 in a window
                // with three tabs means "the ninth", and there is no ninth. A
                // clamp would silently answer a different question.
                match goto_tab_index(self.window.tabs.len(), ordinal) {
                    Some(index) => self.activate_tab(index, false),
                    None => Ok(()),
                }
            }
            shortcuts::Action::ReopenClosed => self.reopen_recent(0),
            // Bound, claimed, and deliberately inert. The attention queue (P1-8)
            // and the command palette (P1-9) were ruled and keyed but unbuilt;
            // the row was real so that nothing else took the chord and no byte
            // leaked to the shell in the meantime. Same latitude as the formulas
            // switch: the seat exists before the machine that sits in it.
            //
            // The picture-in-picture slots join them (2026-08-17), one rung
            // further out: they are claimed *and* unassigned, so nothing arrives
            // here until a user gives one of them a chord. That is the shape the
            // ruling asked for — "a summon key is per-slot configurable, and the
            // prototype's default is only a default" — and it is why this arm
            // exists before the window it summons does.
            // **P1-8 landed** (§7.1.6b′ F3): the row is no longer inert. It walks
            // the attention queue oldest-first and puts the keyboard in the pane
            // it lands on — the same `activate_tab` a card's own click makes, so
            // in focus mode a jump *is* the stage changing.
            shortcuts::Action::JumpAttention => self.jump_to_attention(),
            shortcuts::Action::SummonPip(_) => Ok(()),
            // **The row this window usually never sees** (§7.54). The chord is
            // claimed from Windows with `RegisterHotKey` and a claimed chord is
            // taken out of the input stream before any window is handed it, this
            // one included - so a press that arrives *here* is a press Windows
            // did not claim, which means the claim was refused. Another program
            // holds the key, and the reader is inside Folio pressing it anyway.
            //
            // Answering it is the honest reading of the row rather than a
            // fallback: a chord in this table is a chord this window answers, and
            // where it is read depends on who managed to claim it. It leaves a
            // bit rather than acting, on `settle_quake`'s own rule - the press
            // and the window it opens belong to the loop's turn.
            shortcuts::Action::SummonQuake => {
                self.app.quake.press();
                Ok(())
            }
            // **P1-9 landed too** (DESIGN.md §7.55). The palette gave its chord
            // back on 2026-08-28 rather than draw a key with nothing behind it,
            // and took it again in v0.2 with the verb in hand. It is chrome and
            // not content, so this arm raises a popup over a window that stays
            // visible underneath rather than opening anything that owns a seat.
            shortcuts::Action::CommandPalette => self.open_command_palette(),
            // The glyph names the divider it draws, as in Windows Terminal: the
            // minus lays a horizontal rule across the pane and the new shell
            // opens below it; the equals stands a vertical one and the new shell
            // opens beside it.
            shortcuts::Action::SplitHorizontal => self.split_focused_terminal(Axis::Col),
            shortcuts::Action::SplitVertical => self.split_focused_terminal(Axis::Row),
            // **The chord with no direction in its name**, and therefore the
            // chord the `Split direction` setting governs (user ruling,
            // 2026-08-16). Its default is still Windows Terminal's
            // `duplicatePane` rule — `split: "auto"`, cut across the pane's
            // longer side, so the two halves come out as square as the pane
            // allows and a wide pane never becomes two slivers — but a user who
            // has said `Right` or `Down` has said it about this chord too. The
            // two chords one line up name their own rule and are untouched.
            shortcuts::Action::DuplicatePaneSplit => {
                self.split_focused_terminal(self.duplicate_split_axis())
            }
            // **The keyboard's door onto §7.1.6l** (user ruling 2026-08-25, B7),
            // and it is the same door the head's double-click and the `⌄` menu's
            // row already walk through — `toggle_pane_zoom`, once, so the three
            // gestures cannot drift into meaning three things.
            //
            // The subject is the focused pane and not the hovered one: a chord
            // answers for wherever the keyboard is, which is the same rule
            // `close-pane` one arm up already follows.
            shortcuts::Action::ZoomPane => self.toggle_pane_zoom(self.focused_leaf),
            // The pane holding the keyboard, through the one door (ticket 37). The rows are in
            // force only while a terminal holds it, so the focused leaf is that terminal.
            shortcuts::Action::TextLarger => {
                self.step_pane_text_scale(self.focused_leaf, TextStep::Larger, 1)
            }
            shortcuts::Action::TextSmaller => {
                self.step_pane_text_scale(self.focused_leaf, TextStep::Smaller, 1)
            }
            shortcuts::Action::TextActualSize => {
                self.step_pane_text_scale(self.focused_leaf, TextStep::Actual, 1)
            }
            // The keyboard door onto the files column, and the `˅` menu's
            // `Files pane` row is the mouse one. Both are `toggle_files_pane`,
            // so neither can drift into meaning something the other does not.
            shortcuts::Action::FilesPane => self.toggle_files_pane(),
            // R28. It answers for the column's page and for nothing else, so a
            // window with no column and a window whose Git panel is switched off
            // both get the same silence.
            shortcuts::Action::GitPage => self.toggle_git_page(),
            shortcuts::Action::OpenSettings => self.toggle_settings_panel(),
            // **Door 2 of two** (§7.1.6b′ ②): one chord, both directions,
            // because the mode is one bit and the Appearance row shows which way
            // it is set. A separate "leave" key would be a second truth about the
            // same bit.
            shortcuts::Action::ToggleFocusMode => self.toggle_focus_mode(),
            // Ruling 9's row, dispatched like every other: the same verb the
            // header's save button and the editor's own `Ctrl+S` reach.
            shortcuts::Action::SavePreview => self.save_preview(),
            // T3's two rows, dispatched beside the save because they are reached
            // the same way: the buffer the keyboard's surface is looking at, and
            // silence on a surface with nothing to type into.
            shortcuts::Action::UndoPreview => self.step_preview_history(Step::Back),
            shortcuts::Action::RedoPreview => self.step_preview_history(Step::Forward),
            // §7.1.5c ③, and the whole of it: the walk is over the **full**
            // history, never over the ticks the rail happened to draw. A
            // collapsed bucket is a density decision made by a painter with a
            // pane's height to fit into, and the keyboard is not looking at the
            // pane.
            shortcuts::Action::PrevCommandMark => self.step_command_mark(Step::Back),
            shortcuts::Action::NextCommandMark => self.step_command_mark(Step::Forward),
            // §7.1.5d. Both chords of the row arrive here — the alias is a
            // second chord, never a second verb — and so will the `Find…` a
            // menu will one day carry.
            shortcuts::Action::OpenSearch => self.open_search(self.search_host_seat()),
            // B81's second stance: the capsule is up, the hands are back on the
            // shell, and the function key still walks. `Enter` cannot do this —
            // it belongs to the shell the moment the caret leaves the box.
            shortcuts::Action::NextMatch => self.step_search(true),
            shortcuts::Action::PrevMatch => self.step_search(false),
            // §7.7 W2 ④: the one rung of the Escape ladder that a page can be
            // standing under while still owning the key. On a terminal the
            // ladder has already answered and this arm is unreachable — see
            // [`shortcuts::Action::CloseSearch`].
            shortcuts::Action::CloseSearch => {
                self.close_search()?;
                Ok(())
            }
            shortcuts::Action::WebAddress => self.open_web_address(),
            shortcuts::Action::WebDevTools => self.open_web_dev_tools(),
            // §7.7 ⑨: the row above with the scope taken off, which makes it a
            // different verb — the page's own key can assume a page, and a key
            // that answers with the hands on a shell has to be able to make one.
            shortcuts::Action::WindowAddress => self.open_address_here(),
        }
    }

    /// The keyboard half: typing goes here now.
    ///
    /// Guarded on there being a session at the seat, because a files column has
    /// nothing to type into and a folder tab has no shell at all — the guard
    /// every focus move in this window carries, written once.
    ///
    /// **And a composition does not come with it** (review 2026-09-17 P2-b).
    /// §7.1.5a″'s ruling is that a composition belongs to the field it started
    /// in and is cancelled when that field goes away — never committed into
    /// whatever is holding the keyboard next. [`settle_composition_owner`] keeps
    /// that promise at the tail of every pass, but it asks it of
    /// [`composition_outlived_its_field`], which compares **kinds** of owner:
    /// one shell and another shell are both [`ImeOwner::Shell`], so a
    /// composition begun at pane A's prompt survives the keyboard moving to pane
    /// B and the next commit arrives at B. A pane is a field, so that is the
    /// same report said about two terminals.
    ///
    /// **Closed here, at the door, and therefore for every road at once.** This
    /// is the one place `focused_leaf` moves — the pointer's
    /// ([`Self::focus_pane_at`]) and the named one's ([`Self::focus_seat`], which
    /// the palette, the attention queue and a dropped path all come through) —
    /// and the pointer road had the identical hole, so fixing it anywhere else
    /// would have closed one of the two. The branch this sits in is exactly
    /// "the shell holding the keyboard is changing", which is exactly when the
    /// old field goes away.
    ///
    /// **Cancelled and not committed**, through the window's one door
    /// ([`Self::cancel_composition`]), because that is the ruling: half-typed
    /// letters meant for A's prompt must not be handed to B's.
    pub(crate) fn take_keyboard_into(&mut self, seat: SeatId) -> Result<()> {
        if !(self.sessions.contains_key(&seat) && self.focused_leaf != seat) {
            return Ok(());
        }
        if self.window.composing == Some(ImeOwner::Shell) {
            self.cancel_composition(ImeOwner::Shell, "take_keyboard_into")?;
        }
        self.focused_leaf = seat;
        // The frame slot holds the pane that *was* focused. Leaving it would
        // let the next present assert a stale grid against the new pane.
        self.window.last_presented_frame = None;
        Ok(())
    }

    /// Whether a hosted page is the thing typing goes into right now.
    ///
    /// Two facts and both are needed: this window has a page, and the seat it
    /// stands on is the one holding the keyboard. The engine's own `GotFocus` is
    /// *not* asked, and that is deliberate — it arrives one message pump later
    /// than the click that caused it, so a chord pressed in the same breath as
    /// the click would be judged against the focus the window had before it.
    ///
    /// **The layout's focus and never `focused_leaf`** (found on the machine,
    /// 2026-08-22: clicking inside a page and pressing `Ctrl+L` did nothing).
    /// `focused_leaf` names *the shell the keyboard falls back to* — it is only
    /// ever written for a seat that is in `sessions`, which is to say for a
    /// terminal — so a preview seat can never be it, and a predicate written
    /// against it answers `false` on every page there has ever been. What names
    /// the pane a press put the keyboard in is `focused_preview_seat`, which is
    /// the same reading `Focus::preview` one line above already takes.
    pub(crate) fn page_holds_the_keyboard(&self) -> bool {
        self.page_with_the_keyboard().is_some()
    }

    /// **Which page typing goes into right now, docked or floated** (§7.14c,
    /// closing §7.14b 挂账 ⓑ).
    ///
    /// One reading of [`Self::preview_keyboard_surface`] and a fork on what kind
    /// of surface it named, which is the same shape [`a_page_still_has_a_pane`]
    /// already takes for the same reason: **the float answers instead of the
    /// tree, not beside it.** `pop_out_preview` closes the seat by design, so a
    /// popped-out page has no seat for the layout to focus and never will again
    /// — and a rule written against the tree alone answered "no page has the
    /// keyboard" about the one page a reader had just clicked into. Measured on
    /// the machine before this ticket (`BT_MOUSE_TRACE`): `press_web_page
    /// focus=SeatId(1) page_leaf=Some(LeafId { tab: TabId(1), seat: SeatId(2) })
    /// holds=false`, and [`Self::settle_the_web_keyboard`] duly took the keys
    /// back from the engine on the very next frame.
    ///
    /// A **leaf** and not a seat, because that is what the answer is good for:
    /// the settling loop is keyed by leaf, and a floated page's seat number
    /// names nothing that has a rectangle. [`Self::focused_web_seat`] stays the
    /// seat-shaped question and stays docked-only, because what *it* is for is
    /// hanging chrome off a pane head — see the note there.
    pub(crate) fn page_with_the_keyboard(&self) -> Option<LeafId> {
        let surface = self.preview_keyboard_surface()?;
        // **A surface showing its page's source is not a surface typing goes
        // into a page on** (user ruling 2026-08-27; DESIGN §7.32). It matters
        // now that the source face edits: the ordinary way to reach it is to
        // read the page, press `</>`, and start typing — and the press that read
        // the page had already handed the keyboard to the engine. Without this
        // clause `settle_the_web_keyboard` finds the page still "the typing
        // target", keeps the keys where they are, and every letter goes into a
        // browser that is not even on the glass.
        //
        // Asked here rather than in the settling loop because this is the one
        // sentence both readers share: the loop takes the keyboard back on it,
        // and `Focus::web_page` stops claiming `Ctrl+L` and `F12` on it, which
        // are the page's verbs and not this document's.
        if self.page_source_shown_on(surface).is_some() {
            return None;
        }
        match surface {
            PreviewSurface::Float(id) => self.page_carried_by(id),
            // And not while the quick edit has it: that is answered by
            // `preview_keyboard_surface` above and is a different surface's
            // keyboard, whatever seat is focused underneath it.
            surface => {
                let seat = self.focused_preview_seat()?;
                (self.preview_here(seat) == surface && self.seat_holds_a_page(seat))
                    .then(|| self.leaf_here(seat))
            }
        }
    }

    /// **Whether a shell is the one holding the keyboard** — `InputOwner ==
    /// Terminal` (`docs/DESIGN.md` §7.1.5), asked as one question.
    ///
    /// User report, 2026-08-13: click a files tree or a preview and the
    /// terminal's caret goes on blinking behind you. The split block had already
    /// ruled that an unfocused pane's caret freezes and fades — its argument is
    /// on [`bt_render::seat_caret`] — but it was written when the only way to
    /// lose the keyboard was to focus *another terminal*, so it answered "which
    /// pane" and never "which kind of thing". Every owner below is a way for the
    /// keyboard to leave every shell at once, and against all of them the
    /// window's one lit caret was claiming a keystroke it would not receive.
    ///
    /// **A blink means "typing lands here", and nothing else** (ruling
    /// 2026-08-13). So the answer is the owner, not the focus: the moment the
    /// owner is anything but a terminal, every caret on screen wears the standing
    /// it already had for the pane beside it — steady, and in the faded ink —
    /// and it comes straight back when the owner does.
    ///
    /// The modal is not in the list and does not need to be: a dialog is drawn
    /// over a dimmed window, and `Focused(false)` has already stopped the blink
    /// whenever the window itself lost focus.
    pub(crate) fn keyboard_owner_is_a_shell(&self) -> bool {
        keyboard_owner_is_a_shell(self.keyboard_owner())
    }

    /// Who holds the keyboard, read off the window as it stands.
    ///
    /// One reading for the two questions asked of it — whether a caret may blink
    /// ([`keyboard_owner_is_a_shell`]) and where a composition goes
    /// ([`ime_owner`]) — because the day those two disagree is the day a caret
    /// blinks in a shell that is not receiving the characters.
    pub(crate) fn keyboard_owner(&self) -> KeyboardOwner {
        let owner = KeyboardOwner {
            rename: self.window.rename.is_some(),
            // `Menu` and `Dialog`, which own the keyboard outright while they are
            // up (§7.1.5, and the mock-up's "an open menu owns the keyboard" at
            // 6188).
            // The prompt inside a git context menu, which is a popup and would
            // otherwise be swallowed by the rung under this one.
            git_prompt: self
                .window
                .git_menu
                .as_ref()
                .is_some_and(|menu| menu.prompt.is_some()),
            // The four modals, and then **every popup, through the one reading
            // the key ladder takes** ([`PopupsUp`]): a popup that takes the
            // keyboard away from the shell has to take the keystroke too, and
            // the way to keep those two answers together is to make them one
            // answer.
            menu_or_dialog: self.app.quit.as_ref().is_some_and(quit::Quit::is_asking)
                || self.window.dirty_gate.is_open()
                || self.window.first_run.is_open()
                || self.window.psreadline_invite.is_open()
                // The multi-line paste card (0.4.4 ticket 02): modal by the owner's ruling of
                // 2026-09-23, so a drop is refused under it and a composition goes nowhere.
                || self.paste_card_seat().is_some()
                || self.window.settings.is_open()
                || popup_takes_the_key(self.popups_up()).is_some(),
            files_tree: self.files_keyboard_seat().is_some(),
            // The graph's search field, when it is the focused preview's and it
            // holds the keyboard.
            graph_search: self
                .preview_keyboard_surface()
                .is_some_and(|surface| self.graph_search_focused(surface)),
            // `PreviewEdit` — and the preview's read-only browsing, which is the
            // same owner wearing its other state: the arrows scroll the document,
            // so they are not the shell's either.
            preview: self.preview_keyboard_surface().is_some(),
            // The search capsule, and only while the caret is in it.
            search: self.window.search.is_focused(),
            palette: self.window.palette.is_some(),
        };
        self.trace_ime_owner(owner);
        owner
    }

    /// The composition the **terminal** is entitled to draw.
    ///
    /// One `preedit` field for the window, because there is one composition, and
    /// one owner for it: letters being composed belong wherever the keyboard is,
    /// so a composition made in the preview must not also be overlaid on the
    /// grid behind it. Held as one field rather than two so that the rung which
    /// leaves the editing keys to the IME mid-composition
    /// (`input::is_ime_owned_key`) keeps working for both owners without being
    /// told which one is composing.
    pub(in crate::runtime) fn shell_preedit(&self) -> Option<&Preedit> {
        matches!(ime_owner(self.keyboard_owner()), ImeOwner::Shell)
            .then_some(self.window.preedit.as_ref())
            .flatten()
    }

    /// The caret of whichever **field** holds the keyboard, in window pixels.
    ///
    /// Reached only through [`ime_caret_source`], which is why the three rungs
    /// that are not a field answer `None` here rather than being asked to
    /// invent one: this function measures, it does not decide.
    ///
    /// Every arm reads the rectangle its **painter** used — the capsule's own
    /// `caret_line`, the toolbar's `graph_search_field`, the prompt's
    /// `caret_line`, the strip's recorded box — because a candidate list placed
    /// from a second derivation is a list that stands beside the caret it claims
    /// to follow. The `y`/`height` are the field's whole line box rather than
    /// the bar drawn inside it: what winit is being told is which strip of the
    /// window the candidate list may not cover, and a rectangle inset by the
    /// caret's own four pixels is four pixels of the field the list would sit on.
    fn field_ime_caret(&mut self, owner: ImeOwner) -> Option<ImeCursorArea> {
        let scale = self.window.renderer.scale_factor() as f32;
        let line = match owner {
            ImeOwner::Rename => self.window.rename_caret_line?,
            ImeOwner::GraphSearch => {
                let surface = self.preview_keyboard_surface()?;
                let rects = self.graph_toolbar_rects(surface)?;
                let toolbar = self
                    .window
                    .git_graphs_shown
                    .get(&surface)?
                    .toolbar
                    .as_ref()?;
                git_graph::graph_search_field(rects, toolbar, scale)?.caret
            }
            ImeOwner::GitPrompt => self.git_menu_layout()?.prompt_rects()?.caret_line(scale),
            // The document's own caret, which is measured in rows and columns
            // rather than in a prefix's width — the one field here whose box is
            // not a box somebody typed a line into.
            ImeOwner::Preview => return self.preview_ime_cursor_area(),
            ImeOwner::Search => {
                let capsule = self.search_capsule()?;
                let (_, _, caret_x) = self.search_field_look();
                capsule.caret_line(caret_x, scale)
            }
            // The very rectangle the painter struck the caret in — one
            // derivation, read twice, which is what stops a candidate list from
            // drifting away from the letters it is offering to finish.
            ImeOwner::Palette => self.window.palette_layout.as_ref()?.caret_line(),
            // Not a field. `ime_caret_source` sends neither of these here, and a
            // rectangle invented for them would be a candidate list following a
            // caret that is receiving nothing.
            ImeOwner::Modal | ImeOwner::FilesTree | ImeOwner::Shell => return None,
        };
        Some(ime_cursor_area_of(line))
    }

    /// **The one door.** Whoever holds the keyboard hands the IME its caret.
    ///
    /// `grid` is the frame just composed, when this is being called from the pass
    /// that made one; without it the terminal's rung simply has nothing to say
    /// this turn, which is correct — the grid's caret moves when a frame moves.
    ///
    /// Called from the event loop's own tail as well as from the publish, so
    /// "every frame the caret can move" needs no list of the ways it can move: a
    /// keystroke, a scroll, a resize and a capsule relaid all wake the loop, and
    /// [`ImeCursorThrottle`] turns a burst of identical rectangles into nothing
    /// and a burst of moving ones into one call per 60Hz slot.
    pub(in crate::runtime) fn offer_ime_caret(&mut self, grid: Option<&ViewportFrame>) {
        if !self.window.ime_active {
            return;
        }
        let owner = ime_owner(self.keyboard_owner());
        let area = match ime_caret_source(owner) {
            ImeCaretSource::Nowhere => return,
            ImeCaretSource::Field => self.field_ime_caret(owner),
            // Only while the shell is the one composing. A terminal that keeps
            // printing behind a preview being typed into still publishes frames,
            // and each of them used to drag the candidate window back to the
            // grid's caret — under the pointer's own reading, the candidate list
            // would sit over the shell while the letters went into the file.
            // `grid` is the focused pane's frame, composed this pass at that pane's own metrics
            // (ticket 37), so the caret rectangle is measured in the same cells.
            ImeCaretSource::TerminalCursor => grid
                .zip(self.focused().map(|leaf| leaf.metrics))
                .map(|(frame, metrics)| {
                    window_ime_cursor_area(
                        self.window.renderer.seat_viewport(),
                        self.window.renderer.ime_cursor_area(metrics, frame),
                    )
                }),
        };
        let Some(area) = area else {
            return;
        };
        if let Some(area) = self.window.ime_cursor_throttle.offer(area, Instant::now()) {
            self.apply_ime_cursor_area(area, "sent");
        } else if ime_outbound::enabled() {
            self.trace_ime_area(
                area,
                if self.window.ime_cursor_throttle.is_pending() {
                    "throttled"
                } else {
                    "unchanged"
                },
            );
        }
    }

    /// One key, with the terminal menu holding the keyboard.
    ///
    /// The file menu's four rules verbatim — Esc closes, the arrows walk,
    /// Enter/Space run, everything else is swallowed — because §7.1.3's
    /// 「可键盘化」 is a promise about context menus rather than about file rows,
    /// and a menu that could not be walked would be the one menu in this window
    /// reachable only by a pointer. Since §7.1.6i's floor it also carries the
    /// pane menu's two extra rules, for the same reason that menu carries them:
    /// `→` on a heading opens its child, `←` inside one shuts it, and Esc unwinds
    /// one layer at a time rather than dismissing the whole menu from inside the
    /// child.
    fn term_menu_key(&mut self, event: &KeyEvent) -> Result<()> {
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                if !event.repeat && !self.set_term_submenu(false)? {
                    self.close_term_menu()?;
                }
            }
            Key::Named(NamedKey::ArrowRight)
                if matches!(
                    self.window.term_menu.as_ref().and_then(|menu| menu.hover),
                    Some(profiles::TermMenuHover::Row(entry)) if entry.has_submenu()
                ) =>
            {
                self.set_term_submenu(true)?;
            }
            Key::Named(NamedKey::ArrowLeft)
                if matches!(
                    self.window.term_menu.as_ref().and_then(|menu| menu.hover),
                    Some(profiles::TermMenuHover::Submenu(_))
                ) =>
            {
                self.set_term_submenu(false)?;
            }
            // Repeats on the travel keys and nowhere else: holding an arrow down
            // is one continuous "further", and holding Enter is not one
            // continuous "again".
            //
            // **The walk stays on the parent while a child is up.** The child is
            // the profile list, and it is walked by the pane menu's own keyboard
            // in the head; here it is a pointer surface with an `←` out of it,
            // which is the same promise this menu's other rows keep.
            Key::Named(NamedKey::ArrowDown) | Key::Named(NamedKey::ArrowUp) => {
                let forwards = matches!(event.logical_key, Key::Named(NamedKey::ArrowDown));
                if let Some(menu) = self.window.term_menu.as_mut() {
                    let current = match menu.hover {
                        Some(profiles::TermMenuHover::Row(entry)) => Some(entry),
                        Some(profiles::TermMenuHover::Submenu(_)) | None => None,
                    };
                    menu.hover = profiles::term_menu_step(
                        current,
                        menu.subject,
                        forwards,
                        menu.pane,
                        menu.lone,
                    )
                    .map(profiles::TermMenuHover::Row);
                }
                if self.refresh_overlay() {
                    self.present_chrome_change()?;
                }
            }
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                if !event.repeat
                    && let Some(hover) = self.window.term_menu.as_ref().and_then(|menu| menu.hover)
                {
                    match hover {
                        // The heading opens its child rather than running, which
                        // is what `→` does and what a click does.
                        profiles::TermMenuHover::Row(entry) if entry.has_submenu() => {
                            self.set_term_submenu(true)?;
                        }
                        profiles::TermMenuHover::Row(entry) => {
                            self.run_term_menu_row(profiles::TermMenuHit::Row(entry))?;
                        }
                        profiles::TermMenuHover::Submenu(index) => {
                            self.run_term_menu_row(profiles::TermMenuHit::Submenu(index))?;
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Hang the candidate window off `area`, which is in **window** pixels.
    ///
    /// Renderer pixels, winit `PhysicalPosition`, and a per-monitor-aware Win32
    /// client area all share the client-origin device-pixel axis. No
    /// screen-origin translation belongs here.
    ///
    /// A seat-origin one does — and it belongs to the *caller* now that there is
    /// more than one caller. The terminal's caret is computed by the frame
    /// machinery in the seat's own coordinates and is translated by
    /// [`window_ime_cursor_area`] where it is read (§4.1, one translation); the
    /// preview's is measured off the pane rectangle and is already the window's.
    /// Translating here would have applied the seat's origin to a rectangle that
    /// never had one.
    fn apply_ime_cursor_area(&mut self, area: ImeCursorArea, action: &'static str) {
        self.trace_ime_area(area, action);
        hang_watch::during(hang_watch::Station::ImeCursorArea, || {
            self.window.window.set_ime_cursor_area(
                PhysicalPosition::new(area.x, area.y),
                PhysicalSize::new(area.width, area.height),
            );
        });
        let system_caret = hang_watch::during(hang_watch::Station::ImeSystemCaret, || {
            self.trace_ime_line(|| {
                ime_outbound::caret_line("update", "cursor_area", Some((area.x, area.y)))
            });
            self.window.ime_system_caret.update(area.x, area.y)
        });
        if let Err(error) = system_caret {
            hang_watch::during(hang_watch::Station::DiagnosticWrite, || {
                eprintln!("Chinese IME system-caret update ignored: {error}");
            });
        }
    }

    pub(in crate::runtime) fn flush_ime_cursor_area(&mut self, now: Instant) {
        if let Some(area) = self.window.ime_cursor_throttle.flush_due(now) {
            self.apply_ime_cursor_area(area, "flushed");
        }
    }

    /// **Say the caret's rectangle again, unchanged, because the window moved
    /// out from under the answer** (user report 2026-09-14; `docs/DESIGN.md`
    /// §13.16 ⑥).
    ///
    /// Everything this program computes for the input method is **window**
    /// pixels ([`Self::apply_ime_cursor_area`]), and that is the contract on
    /// both platforms. What the platform *stores* is not: AppKit's
    /// `firstRectForCharacterRange:` must answer in **screen** coordinates, so
    /// winit converts through the window at the moment it is asked
    /// (`convertRect:toView:nil` then `-[NSWindow convertRectToScreen:]`, which
    /// is right on any display), and `NSTextInputContext` then **caches** that
    /// answer. Apple's own instruction for the cache is
    /// `invalidateCharacterCoordinates`, and the one thing in this process that
    /// reaches it is `Window::set_ime_cursor_area` — which is called only when
    /// the *window-relative* rectangle changes.
    ///
    /// A window carried to a second display changes none of it. The prompt is
    /// still the same number of pixels from the same window's top-left, so the
    /// throttle drops the offer, the context is never invalidated, and the next
    /// composition is placed off the screen rectangle computed while the window
    /// was somewhere else — the candidate list stranded mid-window, which is the
    /// report. The correction is not arithmetic: the arithmetic was right both
    /// times. It is telling the platform to ask again.
    ///
    /// Not `reset`: that is a composition ending. This keeps the 60Hz cadence,
    /// so a drag across the seam costs the same as a caret moving.
    pub(in crate::runtime) fn reoffer_ime_cursor_area(&mut self) {
        let Some(area) = self.window.ime_cursor_throttle.last_sent() else {
            return;
        };
        self.window.ime_cursor_throttle.rearm();
        if let Some(area) = self.window.ime_cursor_throttle.offer(area, Instant::now()) {
            self.apply_ime_cursor_area(area, "reoffered");
        } else if ime_outbound::enabled() {
            self.trace_ime_area(area, "reoffer_throttled");
        }
    }

    pub(in crate::runtime) fn reset_cursor_blink(&mut self, now: Instant) -> bool {
        let changed = self.window.cursor_blink.reset(now, self.app.motion);
        self.window
            .renderer
            .set_cursor_blink_visible(self.window.cursor_blink.visible());
        changed
    }

    pub(in crate::runtime) fn advance_cursor_blink_if_due(&mut self, now: Instant) -> Result<()> {
        // **A caret that is not the keyboard's does not blink** (ruling
        // 2026-08-13). There is no phase to advance and therefore no frame owed
        // for one — which is also the literal "冻结" the report asked for, and
        // why this is a freeze rather than a paint: the loop stops waking twice
        // a second to redraw a caret nobody is typing at. Parked at its lit
        // phase, so the instant a shell has the keyboard back the caret is
        // *there* rather than half a beat away.
        if !self.keyboard_owner_is_a_shell() {
            self.window.cursor_blink.reset(now, self.app.motion);
            self.window
                .renderer
                .set_cursor_blink_visible(self.window.cursor_blink.visible());
            return Ok(());
        }
        if !self.window.cursor_blink.advance(now) {
            return Ok(());
        }
        self.window
            .renderer
            .set_cursor_blink_visible(self.window.cursor_blink.visible());
        // The phase it just changed lives in the renderer, not in the frame:
        // this is a present the caret owes, not a picture. Same road as the
        // strip's ring, and for the same reason.
        self.publish_chrome_frame(now)
    }

    /// **Every key this window is told about, and the one gate above the
    /// ladder** (§7.1.5, §7.54d).
    ///
    /// `is_synthetic` is winit's, and it is taken here rather than at the door
    /// so that there is one place in this window that says what a keystroke is.
    /// See [`input::is_a_keystroke`] for the two facts it answers with and for
    /// the summon that made the second of them visible: everything below this
    /// line — the hint card, the dialogs, the menus, the shortcut table and the
    /// child — is written for a key a hand pressed *at this window*, and a
    /// report of a key that was already down when the window arrived is not one.
    pub(crate) fn keyboard_input(&mut self, event: &KeyEvent, is_synthetic: bool) -> Result<()> {
        if !input::is_a_keystroke(event.state, is_synthetic) {
            return Ok(());
        }
        // **A program that types for you is still typing** (T-REMOTE-INPUT-PACKET,
        // user report 2026-09-15: a sentence sent from an OPPO phone through
        // O+ Connect's phone keyboard never appeared in the pane).
        //
        // Such a program sends characters, not key presses, and the press winit
        // hands on for one carries the character in `text` and nothing in either
        // key field. Every rung below this line reads `logical_key` — the
        // encoder, the four one-line fields, the quick edit, the column — so the
        // character had no rung and was dropped. [`input::injected_logical_key`]
        // has the measurements and the whole argument; what matters here is
        // *where* it is applied.
        //
        // **On the event, once, above the ladder**, and not as a rung of its own.
        // A rung would have to be placed, and there is no placement that is
        // right: put it high and a character typed into the settings sheet stops
        // reaching the sheet, put it low and every field above has already
        // swallowed the press. Rewriting the key instead leaves the ladder
        // exactly as it is — one field to read, one modifier policy, one door to
        // the child — and the event arrives at whichever rung owns the keyboard
        // indistinguishable from the same character typed by a hand. That is
        // also what records it as typing: the encoder's own
        // `send_user_input(.., UserInputKind::Keyboard)` and the
        // `note_user_typing` above it, rather than a second road to the pipe with
        // a paste's bracketing or a provenance of its own.
        //
        // Below the gate, because a release needs no key: the `WM_KEYUP` half of
        // every injected character is one, and it has already returned.
        let injected = input::injected_logical_key(
            event.physical_key,
            &event.logical_key,
            event.text.as_deref(),
        )
        .map(|key| {
            let mut event = event.clone();
            event.logical_key = key;
            event
        });
        let event = injected.as_ref().unwrap_or(event);
        let now = Instant::now();
        // **The hint card is spent by the first key that is not a modifier**
        // (§7.1.5e′), and this is the top of the function for the one reason
        // that matters: every branch below it can return, and a card left
        // standing over the thing a chord just did is the failure this surface
        // exists to avoid. See [`Self::spend_key_hint`], which the wheel and the
        // mouse button read too; the exemption is here because this is the one
        // door at which a modifier can be the gesture.
        //
        // Repeats included: a held `j` in `vim` is a hand that is very much not
        // waiting to be told anything.
        if !shortcuts::is_a_bare_modifier(&event.logical_key) {
            self.spend_key_hint()?;
        }
        if self.reset_cursor_blink(now) {
            self.publish_frame(FrameTrigger {
                occurred_at: now,
                source: FrameSource::Keyboard,
            })?;
        }
        // Any keystroke dismisses the transient peek flyout (Esc included, per the peek verb
        // ruling) without consuming the key: typing means the user has moved on from hovering.
        self.dismiss_peek()?;

        // Esc unwinds one layer per press, top-most first, and a drag in flight
        // is the top-most layer there is: "Esc mid-drag means 'never mind', and
        // without this the drop still committed on pointerup" (mock-up 6045-6051).
        // It stands above even the modal, because a drag is a gesture the user is
        // in the middle of making and a dialog is one they have already opened.
        //
        // A resize is a drag for this purpose too, and it stands beside the tab
        // drag rather than below it: J122 rules the two mutually exclusive — one
        // gesture owns the pointer at a time — so the order between them is a
        // formality and only one of the two calls can ever answer `true`.
        // The glance card falls on Esc like every transient — but as a
        // *bystander*, not a rung of the ladder: the press is never consumed
        // here, so the same Esc still unwinds whatever the layers below owe it
        // and, with nothing above the shell open, still reaches the PTY
        // (ruling 2026-08-14: now that the card can hold the pointer, it needs
        // the retreat every hover surface has).
        if matches!(event.logical_key, Key::Named(NamedKey::Escape))
            && !event.repeat
            && self.hide_file_peek()
        {
            self.refresh_chrome();
            self.present_chrome_change()?;
        }
        if matches!(event.logical_key, Key::Named(NamedKey::Escape))
            && !event.repeat
            && (self.cancel_drag()? || self.cancel_divider_drag()?)
        {
            return Ok(());
        }
        // **The quit card owns the keyboard outright, in every window**, above
        // the gate for the reason it is drawn above it. Enter answers with the
        // focused button, which is `Cancel`, and Esc answers the same way — a
        // question about losing work can only be dismissed as "not that", and
        // this one is asked of the whole application, so no window may go on
        // taking keys while it stands.
        if self.app.quit.as_ref().is_some_and(quit::Quit::is_asking) {
            if !event.repeat {
                match &event.logical_key {
                    Key::Named(NamedKey::Enter) => {
                        self.answer_quit_card(quit::QUIT_FOCUSED_ANSWER)?;
                    }
                    Key::Named(NamedKey::Escape) => {
                        self.answer_quit_card(quit::QuitAnswer::Cancel)?;
                    }
                    _ => {}
                }
            }
            return Ok(());
        }
        // **The gate owns the keyboard outright**, above the settings dialog for
        // the reason it is drawn above it. Enter answers with the focused button,
        // which is `Cancel` — the answer that changes nothing — and Esc answers
        // the same way, because dismissing a question about losing work can only
        // ever mean "not that". Every other key is swallowed.
        if self.window.dirty_gate.is_open() {
            if !event.repeat {
                match &event.logical_key {
                    Key::Named(NamedKey::Enter) => {
                        self.answer_dirty_gate(restore::GATE_FOCUSED_ANSWER)?;
                    }
                    Key::Named(NamedKey::Escape) => {
                        self.answer_dirty_gate(restore::GateAnswer::Cancel)?;
                    }
                    _ => {}
                }
            }
            return Ok(());
        }
        // **The first-run card owns the keyboard**, in the order it is drawn,
        // and it is a form rather than a question — so it has a focus order and
        // not two verbs (§7.56).
        if self.window.first_run.is_open() {
            if !event.repeat {
                let shift = self.window.modifiers.shift_key();
                self.press_first_run_key(&event.logical_key, shift)?;
            }
            return Ok(());
        }
        // **The invitation owns the keyboard too**, in the order it is drawn.
        // Esc declines, which is the answer that changes nothing on the machine
        // — and every other key is swallowed rather than typed into a shell
        // behind a scrim. Enter presses nothing on purpose: the affirmative here
        // writes files, and a dialog that appeared while somebody was typing
        // must not be able to install a module with the return key they were
        // already reaching for.
        if self.window.psreadline_invite.is_open() {
            if !event.repeat && matches!(event.logical_key, Key::Named(NamedKey::Escape)) {
                self.answer_psreadline_invite(restore::InviteTarget::Decline)?;
            }
            return Ok(());
        }
        // **The multi-line paste card owns the keyboard** (owner's ruling 2026-09-23: "While the
        // paste card is up, keys do not reach the shell: the card is modal and answers only
        // Enter, the Join key and Esc"). `Enter` runs the lines as today — the reader has just
        // read the count, and the informed Enter is the point (2026-09-22) — `Tab` joins them and
        // `Esc` cancels. Every other key is swallowed and the card stays up.
        //
        // Since the owner's ruling of 2026-09-23 it is a standard two-button dialog: `Tab` and
        // `Shift+Tab` move the focus, `Enter` activates the focused word, and the focus opens on
        // the default — `Join` for a block wrapped with the shell's continuation mark (0.4.4
        // ticket 45).
        if self.paste_card_seat().is_some() {
            if !event.repeat
                && let Some(focus) = self.paste_card_focus()
                && let Some(key) = paste_card_key(&event.logical_key, self.window.modifiers, focus)
            {
                self.press_paste_card_key(key)?;
            }
            return Ok(());
        }
        // **A modal owns the keyboard, and now has somewhere to put it.** Esc
        // unwinds one layer per press (§7.1.5: the open picker first, then the
        // dialog), Tab and the arrows walk the dialog's own focus order, and
        // Enter or Space presses whatever the ring is on. Every *other* key is
        // still swallowed rather than typed into a terminal the user cannot see
        // — the guard is unchanged, it is only no longer the whole story.
        //
        // This sits above the IME branch on purpose — a composition that
        // outlived the dialog opening must not be able to reach the child
        // either, and nothing in this dialog takes text yet. The first surface
        // here that does (a shortcut recorder, a theme name) opts in through
        // `ImeOwner`, which is where that decision belongs.
        if self.settings_layout().is_some() {
            // **The recorder is above the focus walk, and it takes everything.**
            // A box that is listening for a chord cannot also be a dialog whose
            // Tab walks a focus order: `Ctrl+Shift+Tab` is a chord somebody may
            // well want, and a dialog that walked its own focus on the second
            // half of it would be a recorder that cannot record the one binding
            // it exists to change. Every key press reaches
            // `record_settings_key` while a capture is open, and Esc gets out.
            if self.window.settings.recording_row().is_some() {
                if event.repeat {
                    return Ok(());
                }
                return self.record_settings_key(event);
            }
            // **A field takes its own keys, and only its own** (§7.1.6c-6b).
            // Below `Escape`, which is the ladder's, and below the recorder,
            // which takes everything: `settings_key_of` maps `Escape` to its own
            // verdict and the walk below still owns `Tab`, so a caret in a box
            // can always be left.
            if !matches!(
                event.logical_key,
                Key::Named(NamedKey::Escape | NamedKey::Tab)
            ) && self.settings_field_key(event)?
            {
                return Ok(());
            }
            let key = settings_key_of(&event.logical_key, self.window.modifiers, event.repeat);
            let before = self.window.settings.category();
            let (rows, shortcuts, profile_lines, scheme_files, values) = self.settings_content();
            let content =
                self.settings_dialog(&rows, &shortcuts, &profile_lines, &scheme_files, &values);
            let verdict = self
                .window
                .settings
                .key(key, content, &self.settings_values());
            // Turning a page puts the reader at its top — the wheel's distance
            // belonged to the page they left. Read off the panel rather than
            // reported by the verdict, because the arrows turn pages as they
            // walk and a verdict that had to say so would be a second place the
            // rule is written.
            if self.window.settings.category() != before {
                self.window.settings_scroll = 0.0;
            }
            match verdict {
                settings::SettingsKeyVerdict::Inert => return Ok(()),
                settings::SettingsKeyVerdict::Moved => {}
                settings::SettingsKeyVerdict::Chose(
                    target @ (settings::SettingsTarget::RestoreRow(_)
                    | settings::SettingsTarget::RestoreAll),
                ) => {
                    self.apply_shortcut_edit(target)?;
                }
                settings::SettingsKeyVerdict::Chose(target) => {
                    self.apply_settings_choice(target)?;
                }
                settings::SettingsKeyVerdict::Adjusted(row, value) => {
                    self.apply_slider(row, value)?;
                }
                settings::SettingsKeyVerdict::Closed => {
                    if let Some(position) = self.window.pointer_position {
                        self.update_chrome_hover(position)?;
                    }
                }
            }
            // **Scrolling follows the focus** (minimal movement): a row Tab
            // reached below the fold is brought into the content box, because a
            // ring drawn outside the box that clips it is a ring nobody sees.
            // Asked of a fresh layout, since the row that now has the focus may
            // be one this frame's scroll had cut off.
            if let (Some(focus), Some(layout)) =
                (self.window.settings.focus(), self.settings_layout())
            {
                let scrolled = layout.scroll_to_show(focus, self.window.settings_scroll);
                if scrolled != self.window.settings_scroll {
                    self.window.settings_scroll = scrolled;
                    // The stack moved under a stationary pointer, which is the
                    // wheel's own rule (`scroll_settings`) reached by the other
                    // door: the row now under the cursor is not the row that was.
                    if let Some(position) = self.window.pointer_position
                        && let Some(moved) = self.settings_layout()
                    {
                        let hover =
                            settings::hit(&moved, &self.settings_values(), position.x, position.y);
                        self.window.settings.set_hover(Some(hover));
                    }
                }
            }
            // **And the picker's own list follows the highlight** (§7.1.6c-5),
            // which is the same rule inside the second scrolling region: an
            // arrow that walks onto the ninth of thirty faces has to bring it
            // into a body that shows eight, or the keyboard would be moving a
            // selection nobody can see. Minimal movement again, so a highlight
            // already in view does not shift the list under it.
            //
            // **The verb at the foot is walked onto like any other row** (user
            // ruling 2026-08-19), so it is brought into view like any other row.
            // It is drawn after the last value, which is exactly where its index
            // is: a picker that let the arrows reach a door and then did not
            // show it would be a door nobody can find.
            match self.window.settings.focus() {
                Some(settings::SettingsTarget::Choice(_, index)) => {
                    self.show_settings_choice_at(index);
                }
                Some(
                    settings::SettingsTarget::MenuAction(row)
                    | settings::SettingsTarget::MenuItemEdit(row, _)
                    | settings::SettingsTarget::MenuItemDelete(row, _),
                ) => {
                    let at = match self.window.settings.focus() {
                        Some(settings::SettingsTarget::MenuAction(_)) => row.option_count(),
                        Some(
                            settings::SettingsTarget::MenuItemEdit(_, index)
                            | settings::SettingsTarget::MenuItemDelete(_, index),
                        ) => index,
                        _ => 0,
                    };
                    self.show_settings_choice_at(at);
                }
                _ => {}
            }
            if self.refresh_chrome() {
                self.present_chrome_change()?;
            }
            return Ok(());
        }
        // The tab-name editor owns the keyboard while it is open (J103;
        // `docs/DESIGN.md` §7.1.5 `InputOwner = Rename`). It sits directly under
        // the modal for the same reason the modal sits where it does — the thing
        // underneath is a terminal, and every key that escapes this branch is a
        // key typed into a shell the user is not looking at. Escape is consumed
        // here rather than falling through to §7.1.5's PTY pass-through, which
        // is exactly what that layering says: Esc reaches the child only when
        // the owner is the terminal.
        if self.window.rename.is_some() {
            // The clipboard is read **before** the editor is taken and only when
            // the chord is the one that needs it — `settings_field_key`'s own
            // borrow order, for its own reason: the read is a call into another
            // process and a field that made it every keystroke would be paying
            // for a paste nobody asked for.
            let pasted = if rename_pastes(&event.logical_key, self.window.modifiers) {
                self.clipboard_line()
            } else {
                String::new()
            };
            let mut clipboard = RenameClipboard {
                paste: &pasted,
                copied: None,
            };
            let mut editor = self.window.rename.take().expect("the editor is open");
            let verdict = rename_key(
                &mut editor,
                &event.logical_key,
                self.window.modifiers,
                &mut clipboard,
            );
            self.window.rename = Some(editor);
            if let Some(copied) = clipboard.copied {
                // The same door every other copy in this window goes through, so
                // a name copied out of a rename box and a path copied off a tree
                // row reach the clipboard by one route — and a clipboard another
                // process is holding open is recoverable here for the reason it
                // is everywhere else.
                let result = write_terminal_clipboard_text(&copied);
                recoverable_clipboard_write(result, "copy from the name editor");
            }
            match verdict {
                RenameVerdict::Commit => self.finish_rename(RenameExit::Submit)?,
                RenameVerdict::Cancel => self.finish_rename(RenameExit::Discard)?,
                RenameVerdict::Held => {
                    // Typing reveals the caret, exactly as it does in the
                    // terminal — a caret that blinks out from under the letter
                    // you just typed reads as a dropped keystroke.
                    self.window.rename_blink.reset(now, self.app.motion);
                    self.refresh_chrome();
                    self.present_chrome_change()?;
                }
            }
            return Ok(());
        }
        // **A git context menu owns the keyboard on the file menu's own terms**
        // (v2 ④), and above it because it is the one popup here that can hold a
        // text field: the branch prompt takes every key, and a rung below the
        // file menu's would never see them.
        if self.window.git_menu.is_some() {
            self.git_menu_key(event)?;
            return Ok(());
        }
        // **The terminal's own menu owns the keyboard on the same terms**
        // (ticket #62), and here rather than lower for the reason the whole
        // ladder is ordered by: the thing underneath it is a *terminal*, and
        // every key that escapes this branch is a key typed into a shell whose
        // view is behind a menu.
        if self.window.term_menu.is_some() {
            self.term_menu_key(event)?;
            return Ok(());
        }
        // **The file menu owns the keyboard outright while it is up**, which the
        // two popups below it deliberately do not.
        //
        // Not an inconsistency — a difference in what the two were promised.
        // §7.1.3 rules the file row's menu "显式、可发现、**可键盘化**", and a
        // menu that can be walked has to be a menu that keeps the keys it walks
        // with: an Up that also scrolled the shell behind it would be a menu
        // borrowing the keyboard rather than holding it. The other two are
        // pointer surfaces with an Esc, and they are audited as such (they are
        // recorded in the block's own audit as still owing this).
        if self.window.file_menu.is_some() {
            match &event.logical_key {
                Key::Named(NamedKey::Escape) => {
                    if !event.repeat {
                        self.close_file_menu()?;
                    }
                }
                // Repeats are honoured on the travel keys and nowhere else, for
                // the reason the tree's own handler gives: holding an arrow down
                // is one continuous "further", and holding Enter is not one
                // continuous "again".
                Key::Named(NamedKey::ArrowDown) | Key::Named(NamedKey::ArrowUp) => {
                    let forwards = matches!(event.logical_key, Key::Named(NamedKey::ArrowDown));
                    // **The rows this menu is actually showing** (user rulings
                    // 2026-08-24 and 2026-08-25): a folder's menu has no `Open`,
                    // a breadcrumb's has no `Reveal`, and the `…` chip's has
                    // nothing but places — so a walk over the vocabulary rather
                    // than over the subject's own list would offer a row nobody
                    // drew.
                    if let Some(menu) = self.window.file_menu.as_mut() {
                        // **And over the rows this menu's *host* can carry out**
                        // (review row D7): the keyboard walks the same list the
                        // paint drew, which is what stops an arrow key landing
                        // on a `Delete` a floating tree never offered.
                        let powers = file_menu_powers(menu.row.as_ref());
                        menu.hover =
                            profiles::file_menu_step(menu.subject, powers, menu.hover, forwards);
                    }
                    if self.refresh_overlay() {
                        self.present_chrome_change()?;
                    }
                }
                Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                    if !event.repeat
                        && let Some(row) =
                            self.window.file_menu.as_ref().and_then(|menu| menu.hover)
                    {
                        self.run_file_menu_row(row)?;
                    }
                }
                // Everything else is swallowed rather than passed down. With a
                // menu on screen there is nothing to type into — the same
                // sentence D49 makes about a focused column, and the same answer.
                _ => {}
            }
            return Ok(());
        }
        // The pane head's menu owns the keyboard on the same terms and by the
        // same four rules, plus the two its picker and its submenu add: `←→`
        // aim the picker's compass and open and shut the child, and Esc unwinds
        // one layer at a time rather than dismissing the whole menu from inside
        // the submenu.
        if self.window.pane_menu.is_some() {
            match &event.logical_key {
                Key::Named(NamedKey::Escape) => {
                    if !event.repeat {
                        // One press, one layer — §7.1.5's ladder read inside a
                        // single popup. A submenu is a surface you opened, and a
                        // key that closed both would make the child unclosable
                        // without also losing the parent.
                        self.window.chevrons.clear();
                        if !self.set_pane_submenu(None)? {
                            self.close_pane_menu()?;
                        }
                    }
                }
                // `→` on a heading opens its child, and `←` inside a child
                // shuts it. Both are guarded rather than unconditional, so an
                // arrow that means neither falls through to the walk below —
                // which is where `←` and `→` aim the picker's compass.
                Key::Named(NamedKey::ArrowRight)
                    if matches!(
                        self.window.pane_menu.as_ref().and_then(|menu| menu.hover),
                        Some(profiles::PaneMenuHover::Row(row)) if row.has_submenu()
                    ) =>
                {
                    let row = match self.window.pane_menu.as_ref().and_then(|menu| menu.hover) {
                        Some(profiles::PaneMenuHover::Row(row)) => Some(row),
                        _ => None,
                    };
                    self.set_pane_submenu(row)?;
                }
                Key::Named(NamedKey::ArrowLeft)
                    if matches!(
                        self.window.pane_menu.as_ref().and_then(|menu| menu.hover),
                        Some(profiles::PaneMenuHover::Submenu(_))
                    ) =>
                {
                    self.set_pane_submenu(None)?;
                }
                Key::Named(NamedKey::ArrowDown)
                | Key::Named(NamedKey::ArrowUp)
                | Key::Named(NamedKey::ArrowLeft)
                | Key::Named(NamedKey::ArrowRight) => {
                    let step = match event.logical_key {
                        Key::Named(NamedKey::ArrowDown) => profiles::MenuStep::Down,
                        Key::Named(NamedKey::ArrowUp) => profiles::MenuStep::Up,
                        Key::Named(NamedKey::ArrowLeft) => profiles::MenuStep::Left,
                        _ => profiles::MenuStep::Right,
                    };
                    // **How long the child is, and not how many profiles
                    // there are** (B9). The two were the same number while the
                    // profile list was the only child; a walk that clamped a
                    // window list against `profiles::count()` would step past
                    // its own last row.
                    let laid_out = self.pane_menu_layout();
                    let rows = laid_out
                        .as_ref()
                        .and_then(|layout| layout.submenu_rows().map(<[[f32; 4]]>::len))
                        .unwrap_or(0);
                    // **And which rows the parent is showing** (user ruling
                    // 2026-08-25) — read off the picture rather than off the
                    // enum, so the walk cannot stop on a `Move to window ▸` that
                    // this session has no second window for.
                    let shown: Vec<profiles::PaneMenuRow> = laid_out
                        .as_ref()
                        .map(|layout| layout.rows().to_vec())
                        .unwrap_or_default();
                    if let Some(menu) = self.window.pane_menu.as_mut()
                        && let Some(moved) =
                            profiles::PaneMenuHover::step(menu.hover, step, rows, &shown)
                    {
                        menu.hover = Some(moved);
                    }
                    // The keyboard's walk lights the same window the pointer's
                    // would (B9) — one aim, read off the highlight either hand
                    // moved.
                    let aim = match (
                        self.pane_menu_layout()
                            .and_then(|layout| layout.submenu_kind()),
                        self.window.pane_menu.as_ref().and_then(|menu| menu.hover),
                    ) {
                        (
                            Some(profiles::PaneMenuRow::MoveToWindow),
                            Some(profiles::PaneMenuHover::Submenu(at)),
                        ) => self.other_window_ids().get(at).copied(),
                        _ => None,
                    };
                    self.aim_at_window(aim);
                    if self.refresh_overlay() {
                        self.present_chrome_change()?;
                    }
                }
                Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                    if !event.repeat
                        && let Some(hover) =
                            self.window.pane_menu.as_ref().and_then(|menu| menu.hover)
                    {
                        match hover {
                            // The heading opens its child rather than running,
                            // which is what `→` does and what a click does.
                            profiles::PaneMenuHover::Row(row) if row.has_submenu() => {
                                self.set_pane_submenu(Some(row))?;
                            }
                            profiles::PaneMenuHover::Row(row) => {
                                self.run_pane_menu_row(profiles::PaneMenuHit::Row(row))?;
                            }
                            profiles::PaneMenuHover::Zone(zone) => {
                                self.run_pane_menu_row(profiles::PaneMenuHit::Zone(zone))?;
                            }
                            profiles::PaneMenuHover::Submenu(index) => {
                                self.run_pane_menu_row(profiles::PaneMenuHit::Submenu(index))?;
                            }
                        }
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        // **A tab's own menu owns the keyboard on the same terms** (丙2), and on
        // the pane menu's rung because it is the same kind of surface: a popup
        // with a child, walked with the same four rules and unwound with the
        // same one-layer Esc. The two are never up together (E61: the opener
        // closes the others), so which of them is asked first is bookkeeping.
        if self.window.tab_menu.is_some() {
            self.tab_menu_key(event)?;
            return Ok(());
        }
        // **The palette's rung** (DESIGN.md §7.55). Above every other popup's,
        // and it is the only one of them that has to be: the rest answer four
        // keys and swallow the remainder, while this one is a text field — a
        // rung below the blanket swallow would be a box that cannot be typed
        // into, and a rung below the shortcut registry would be a box in which
        // `Ctrl+Shift+P` closes and reopens itself while a letter never
        // arrives.
        if self.window.palette.is_some() {
            self.palette_key(event)?;
            return Ok(());
        }
        // **The download sheet's rung** (§7.7 ④, W2 slice ④). Above the pane
        // menu and below the modals, which is where the ruling puts it: 「Esc 梯
        // 子里排在 pane 菜单之上,顶层优先」. It is the one failure card with a
        // way out, and the reason is that there is a page under it to come back
        // to — the other four *are* the seat, and dismissing one of those would
        // leave the black hole a hidden WebView draws.
        if matches!(event.logical_key, Key::Named(NamedKey::Escape))
            && !event.repeat
            && self.dismiss_web_sheet()?
        {
            return Ok(());
        }
        // A popup is not a modal, so it owns exactly one key: the one that puts
        // it away. Everything else is still the terminal's.
        // The preview's switcher is asked **before** the file menu's own rung
        // above and before these two, which is P66's layering read off the
        // mock-up's Esc chain: `#pv-menu` sits at position four, above
        // `#term-menu`, `#file-menu` and `#root-menu`. One press unwinds one
        // layer, and the topmost menu is the one that goes.
        if matches!(event.logical_key, Key::Named(NamedKey::Escape))
            && !event.repeat
            && (self.close_preview_menu()?
                || self.close_profile_menu()?
                || self.close_root_menu()?)
        {
            // Esc is a hand saying "no", which is the one input that must not be
            // undone by a clock: a gate left armed would re-open under a pointer
            // that has not moved, which is the version of a hover menu that
            // cannot be dismissed at all.
            self.window.chevrons.clear();
            return Ok(());
        }
        // **The float's rung of §7.1.5's ladder**: 进行中的拖拽/divider → Menu →
        // Dialog → pinned 浮窗 → owner=Terminal 时透传 PTY. Every layer above it
        // has already answered and returned; below it the key belongs to the
        // shell, so this is the last surface entitled to take an Escape before a
        // running program does.
        //
        // It closes a *transient* peek too, and G87 is why that is one branch and
        // not two: `dismiss` takes the armed intent with it, so an Escape pressed
        // while a hover was still maturing does not close the peek and then watch
        // it reopen 180ms later under a pointer that never moved.
        if matches!(event.logical_key, Key::Named(NamedKey::Escape))
            && !event.repeat
            && self.dismiss_top_float()?
        {
            return Ok(());
        }
        // **The search capsule's rung of §7.1.5's ladder** (B83), and the
        // mock-up's own chain puts it exactly here: `… pv-float, flyout, SEARCH,
        // focus-mode`. Its comment gives the argument in one line — *"the search
        // capsule sits inside a pane, so floats and flyouts above it close
        // first"* — and below it there is nothing but the pane, which is why
        // this is the last surface entitled to take an Escape before a running
        // program does.
        //
        // It answers **whether or not the caret is in the field**. A capsule
        // that could only be dismissed after clicking back into it would be a
        // surface with a mode nobody can see; B81's second stance — search open,
        // hands back on the terminal — still has an Escape that means "put it
        // away".
        if matches!(event.logical_key, Key::Named(NamedKey::Escape))
            && !event.repeat
            && self.close_search()?
        {
            return Ok(());
        }
        // **And one rung below the capsule, the integration notice**
        // (7.1.6j). Below it and not above, because the two are separated by
        // exactly the thing this ladder is ordered by: the capsule is an
        // instrument the reader asked for seconds ago and the strip is one they
        // did not ask for at all, so the press that means "put this away"
        // reaches the asked-for one first.
        //
        // It closes the strip on the **focused** pane and answers nothing. A
        // reader who dismisses it has said "not now" about this pane, so the
        // next PowerShell they open is offered again — `Don't show again` is the
        // press that ends the asking, and it is a word on the strip rather than
        // a key, because a setting is not something a key at a `vim` prompt may
        // write.
        if matches!(event.logical_key, Key::Named(NamedKey::Escape))
            && !event.repeat
            && self.focused_pane_wears_notice()
        {
            self.close_pane_notice(NoticeHost::Seat(self.focused_leaf))?;
            return Ok(());
        }
        // **The ladder ends there** (user ruling 2026-08-20). Focus
        // mode used to hold one more rung below it, and the ruling took that rung
        // out: 「它是设置里的一个设置，怎么这么容易就退出」. Every rung above is a
        // transient that appeared in the last few seconds; the mode is a line in
        // `Appearance` that somebody may have been living in for a week, and it
        // is not something a key pressed at a `vim` prompt should be able to
        // knock over. So an Escape inside focus mode means what it means outside
        // it, all the way down to the `0x1b` the shell gets.

        // A non-empty winit Preedit is the composition authority. Editing/navigation keys are
        // intentionally left to the IME here even if it also exposes a physical named key; no PTY
        // byte may escape this branch and regress M0-beta's composition isolation.
        if self.window.preedit.is_some()
            && input::is_ime_owned_key(&event.logical_key, self.window.modifiers)
        {
            return Ok(());
        }
        // The terminal's copy and paste, **unless the quick edit has the
        // keyboard**. Those two are the one part of the editor's vocabulary that
        // is claimed this far up the ladder, so the exemption has to be made
        // here rather than in the branch below it: without it, Ctrl+V typed into
        // a file would go to a shell the user is not looking at, which is the
        // exact failure `InputOwner` exists to prevent.
        let editing = self.preview_edit_focus().is_some();
        if !editing
            && input::should_copy_selection(
                &event.logical_key,
                self.window.modifiers,
                self.focused()
                    .is_some_and(|leaf| leaf.session.view_selection().is_some()),
            )
        {
            if !event.repeat {
                self.copy_selection()?;
            }
            return Ok(());
        }
        if !editing && input::is_paste_shortcut(&event.logical_key, self.window.modifiers) {
            if !event.repeat {
                self.paste_from_clipboard()?;
            }
            return Ok(());
        }
        // The scaffold that used to stand here — `Ctrl+Alt+Shift+P`, a dev chord
        // that opened and closed the preview seat so its ruled address could be
        // felt before anything could really open one — is gone (N25). Every real
        // verb exists now: Enter and double-click on a file row, the row's
        // context menu, a drag to a pane's edge or centre, a click on an inline
        // image, and the pane's own `×` to close it. A chord that duplicated
        // them would be a second way to reach a state, and it wore Alt at that,
        // which is the AltGr ground the shortcut audit rules out of bounds.
        //
        // The shortcut registry (P2-7), above the PTY encoder because that is what
        // "we claim this chord" means: the table is consulted before any key is
        // encoded, and a chord that is in it never reaches the child. Only chords
        // in the table are taken — `Shortcuts::lookup` matches modifiers exactly,
        // so ordinary typing, the `Ctrl+letter` control codes no row holds and
        // the AltGr family all fall straight through to the encoder below. A row
        // that holds one takes it (0.4.4 ticket 04), and says `shell` for it.
        //
        // **The table asked is the effective one** (2026-08-17): defaults with
        // `keybindings.json` laid over them. Dispatch reads one table and the
        // settings page edits that same table, which is what keeps a chord a
        // user just recorded from taking a restart to arrive.
        if let Some(action) = self.app.shortcuts.lookup(
            &event.logical_key,
            &event.key_without_modifiers(),
            self.window.modifiers,
            self.shortcut_focus(),
        ) {
            if !event.repeat {
                self.run_shortcut(action)?;
            }
            return Ok(());
        }
        // The prompt answers Enter with the button it opened focused, and Esc
        // with nothing at all: an unanswered question folds back into
        // `lastSession` (§7.1.4), so Esc must dismiss the *prompt* without
        // deciding for the user. It sits above the PTY encoder so neither key
        // reaches the child while the question is up.
        if self.window.restore_prompt.is_open() && !self.app.restore_question.is_empty() {
            match &event.logical_key {
                Key::Named(NamedKey::Enter) => {
                    if !event.repeat {
                        self.answer_restore_prompt(restore::FOCUSED_ANSWER)?;
                    }
                    return Ok(());
                }
                Key::Named(NamedKey::Escape) if self.window.restore_prompt.consumes_escape() => {
                    if !event.repeat {
                        self.window.restore_prompt.close();
                        if self.refresh_chrome() {
                            self.present_chrome_change()?;
                        }
                    }
                    return Ok(());
                }
                _ => {}
            }
        }
        // **The transcript's own keys, and only a transcript has them**
        // (§7.1.6h). `Shift+PageUp`, `Ctrl+Home` and `Ctrl+End` all steer a
        // projection over a shell's scrollback; a folder tab has neither, so the
        // whole ladder is skipped and the keys fall through to whatever is below
        // — which is where a column's own keyboard already lives.
        if let Some(leaf) = self.focused()
            && !leaf.session.terminal_modes().alternate_screen
        {
            let page = leaf.grid.rows.get() as i32;
            match &event.logical_key {
                Key::Named(NamedKey::PageUp) if self.window.modifiers == ModifiersState::SHIFT => {
                    return self.scroll_view(page);
                }
                Key::Named(NamedKey::PageDown)
                    if self.window.modifiers == ModifiersState::SHIFT =>
                {
                    return self.scroll_view(-page);
                }
                Key::Named(NamedKey::Home) if self.window.modifiers == ModifiersState::CONTROL => {
                    self.shell_mut().projection.scroll_to_top();
                    self.woke_terminal_thumb(self.focused_leaf)?;
                    return self.publish_interaction_frame();
                }
                Key::Named(NamedKey::End) if self.window.modifiers == ModifiersState::CONTROL => {
                    self.shell_mut().projection.scroll_to_bottom();
                    self.woke_terminal_thumb(self.focused_leaf)?;
                    return self.publish_interaction_frame();
                }
                _ => {}
            }
        } else if matches!(&event.logical_key, Key::Named(NamedKey::End))
            && self.window.modifiers == ModifiersState::CONTROL
            && self
                .focused()
                .is_some_and(|leaf| leaf.projection.is_scrolled())
        {
            // The application's own jump-to-bottom binding: also return the projection-local
            // overflow review to rest so both scroll layers land at the bottom together. The
            // bytes still reach the application below.
            self.shell_mut().projection.scroll_to_bottom();
            self.publish_interaction_frame()?;
        }

        // **An open picker owns the keyboard the way it owns the next click.**
        // The mock-up says it in as many words, in a guard of its own above the
        // one that types: "an open menu owns the keyboard … typing must not land
        // in the terminal behind it" (line 6188), and it returns for
        // `profile-menu` before a single character is delivered.
        //
        // It sits *here* — under the shortcut registry and over the encoder — and
        // that placement is the whole of the rule. A popup is not a modal, so it
        // does not swallow the window's own chords: `Ctrl+Shift+N` is not typing
        // into a hidden terminal, it is a verb, and the settings dialog above is
        // the thing that takes everything. What this stops is a keystroke the
        // user aimed at a menu they can see going into a shell they cannot,
        // which is a keystroke you only find out about later.
        //
        // Esc has already been answered above, where it puts the picker away.
        //
        // **Every popup, and the list is the declaration's** (2026-08-25). This
        // rung used to name two of them — the profile picker and the preview
        // switcher — with the four menus that answer keys of their own having
        // already returned above. That left the files column's root menu and the
        // commit graph's branch filter on no rung at all, while
        // `KeyboardOwner::menu_or_dialog` counted all eight: with either of
        // those open the window said the keyboard had left every shell, so the
        // caret went steady and faded and its clock stopped, and the keystroke
        // went to the shell anyway. One question, asked once — see [`PopupsUp`].
        if popup_takes_the_key(self.popups_up()).is_some() {
            return Ok(());
        }
        // **`InputOwner::FilesTree`** (§7.1.5, D47) — the last layer above the
        // encoder, and the one that finally gives that enum member a body.
        //
        // It sits *here*, under every popup and over the encoder, and the
        // placement is the rule: a column holding the keyboard is not a modal,
        // so the window's own chords, the copy and paste commands and a
        // terminal's `Shift+PageUp` all keep working over the top of it — none
        // of those is typing. What it stops is everything that *is* typing,
        // which is the mock-up's `/* files pane focused: nothing to type into */`
        // (6199) and the reason Esc is answered inside rather than encoded: with
        // this owner, Esc is how you give the keyboard back, and it must never
        // reach a child (§7.1.5's layering says so in as many words).
        //
        // **And which of the column's two pages answers is decided here too**
        // (焦点跟随可见视图, 2026-08-19): the rung lends the keyboard to a
        // *column*, and [`Self::files_column_key`] hands it to whichever body
        // that column has on the glass. It used to hand every key to the tree,
        // which meant a column standing on its Git page walked a list nobody
        // could see.
        if let Some(seat) = self.files_keyboard_seat()
            && self.files_column_key(seat, event)?
        {
            return Ok(());
        }
        // **`InputOwner::PreviewEdit`** (§7.1.5), beside the tree's rung and for
        // the same reasons: under every popup, so the window's own chords still
        // work over a file being edited; over the encoder, so not one character
        // typed into that file reaches the shell behind it. It answers Esc for
        // itself, which is how the keyboard is given back.
        if self.preview_key(event)? {
            return Ok(());
        }
        // **The search capsule's field** (§7.1.5d), on the identical terms and
        // in this family rather than up with the popups. B69 states the half it
        // claims — *"search keys are the search box's; the terminal must not
        // hear them"* — and the placement states the half it does not: a field
        // holding the keyboard is not a modal, so `Ctrl+Shift+N` still opens a
        // tab over the top of it and `Shift+PageUp` still scrolls the pane it is
        // standing on. Only typing is taken.
        //
        // Its Escape was answered far above, at the ladder's own rung for it.
        if self.search_field_key(event)? {
            return Ok(());
        }
        // A tab with no shell has no cursor mode and, one line down, nowhere for
        // the bytes to go — so the keystroke is simply not turned into any
        // (§7.1.6h). Declined here rather than at the write, so that a folder tab
        // never reaches the "typing into the stand-in shell is what makes it
        // yours" bookkeeping in `send_user_input`.
        let Some(application_cursor_mode) = self
            .focused()
            .map(|leaf| leaf.session.application_cursor_mode())
        else {
            return Ok(());
        };
        let Some(bytes) = input::keyboard_bytes(
            &event.logical_key,
            self.window.modifiers,
            application_cursor_mode,
        ) else {
            return Ok(());
        };
        // **回答才消费** (§7.1.5b P1-8, widened by the ruling of 2026-08-25). The event that gives
        // up a place is a user action put into the session that took it, so it is read here — at
        // the door that knows what kind of gesture this is, and not from anything downstream:
        // `send_user_input` is handed a byte string and cannot tell an answer from a paste that
        // happens to end in a newline.
        //
        // **Every key, not `Enter` alone.** Answering a permission prompt in Claude Code is `1`,
        // `y`, `Esc` or `↓`, and a door that only heard `Enter` left the badge lit for a person who
        // had already replied.
        self.answer_attention(self.focused_leaf, UserInputKind::Keyboard);
        // Review row R1-24 — see `Runtime::note_user_typing`.
        self.note_user_typing(self.focused_leaf);
        self.send_user_input(
            &bytes,
            "write keyboard input to PTY",
            UserInputKind::Keyboard,
        )
    }

    /// **Every surface whose rung in [`Runtime::keyboard_input`] stands above
    /// the clipboard's**, asked as one question (T-MAC-EDIT-CLIPBOARD).
    ///
    /// Four cards, the modal, the name editor and the five popups that own the
    /// keyboard outright. None of them is an oversight to be filled in later: a
    /// `Cmd+V` typed in any one of these states does not reach a shell either,
    /// so a *menu* row that did would be the leak the ladder refuses arriving by
    /// a second door.
    ///
    /// The three menus that are **not** here — the profile, root and preview
    /// switchers, and the graph's branch filter — are not an omission either.
    /// "A popup is not a modal, so it owns exactly one key: the one that puts it
    /// away", and the ladder duly lets a clipboard chord past them to the pane
    /// underneath. This answers the same way because it is the same sentence.
    pub(crate) fn a_surface_above_the_clipboard_rung_holds_the_keyboard(&mut self) -> bool {
        self.app.quit.as_ref().is_some_and(quit::Quit::is_asking)
            || self.window.dirty_gate.is_open()
            || self.window.first_run.is_open()
            || self.window.psreadline_invite.is_open()
            || self.paste_card_seat().is_some()
            || self.settings_layout().is_some()
            || self.window.rename.is_some()
            || self.window.git_menu.is_some()
            || self.window.term_menu.is_some()
            || self.window.file_menu.is_some()
            || self.window.pane_menu.is_some()
            || self.window.tab_menu.is_some()
            || self.window.palette.is_some()
    }

    fn ime_native_facts(&self) -> ime_report::NativeFacts {
        native_window(&self.window.window)
            .ok()
            .map(bt_platform::ime_observation::snapshot)
            .unwrap_or_default()
    }

    fn emit_ime_observation(&self, reason: &str, facts: ime_report::NativeFacts) {
        let line = self.window.ime_report.line(
            reason,
            ime_report::now_ms(),
            self.keyboard_owner_is_a_shell(),
            !self.window.web.is_empty(),
            facts,
        );
        let line = format!("{line} window={:?}", self.window.window.id());
        diagnostics::note(&line);
        ime_report::TRACE.line(|| line);
    }

    pub(crate) fn write_ime_observation(&self, reason: &str) {
        self.emit_ime_observation(reason, self.ime_native_facts());
    }

    pub(crate) fn observe_ime_key(&mut self, event: &KeyEvent, is_synthetic: bool) {
        if !self.window.ime_report.has_first_key() {
            self.window.ime_report.first_key(ime_report::now_ms());
            self.window
                .ime_report
                .trace_order(self.window.window.id(), "first-key");
        }
        // Releases do not break a run of presses. No native read, clock read,
        // allocation, or logging on ordinary keys after the first one.
        if !input::is_a_keystroke(event.state, is_synthetic)
            || !self.window.ime_report.watching_keys()
        {
            return;
        }
        let modifiers = self.window.modifiers;
        let latin = !modifiers.control_key()
            && !modifiers.alt_key()
            && !modifiers.super_key()
            && ime_report::printable_latin(event.text.as_deref());
        let terminal = self.keyboard_owner_is_a_shell();
        if self.window.ime_report.key(terminal, latin)
            && self.window.ime_report.may_probe(ime_report::now_ms())
        {
            // Only the threshold candidate refreshes native facts, and no
            // oftener than the report's own interval. In particular an
            // English-mode reading at focus is not reused.
            let facts = self.ime_native_facts();
            if self.window.ime_report.confirm(facts) {
                self.emit_ime_observation("plain-text", facts);
            }
        }
    }

    /// A composition event, routed by [`ime_owner`].
    ///
    /// `Enabled`/`Disabled` are the IME's own bookkeeping and belong to the
    /// window rather than to any surface in it, so they pass every rung: the
    /// state they set has to stay consistent for whoever owns the keyboard when
    /// the composition next starts. `Preedit` and `Commit` are text, and text
    /// goes exactly where [`Self::keyboard_owner`] says the keyboard is.
    pub(crate) fn ime_input(&mut self, event: Ime) -> Result<()> {
        let kind = match &event {
            Ime::Enabled => ime_report::ImeKind::Enabled,
            Ime::Preedit(..) => ime_report::ImeKind::Preedit,
            Ime::Commit(_) => ime_report::ImeKind::Commit,
            Ime::Disabled => ime_report::ImeKind::Disabled,
        };
        self.window.ime_report.ime(kind, ime_report::now_ms());
        let preedit_bytes = match &event {
            Ime::Preedit(text, _) => text.len(),
            _ => 0,
        };
        if let Some(line) = self.window.ime_report.pairing(kind, preedit_bytes) {
            let line = format!("{line} window={:?}", self.window.window.id());
            diagnostics::note(&line);
            ime_report::TRACE.line(|| line);
        }
        if matches!(event, Ime::Enabled) {
            self.window
                .ime_report
                .trace_order(self.window.window.id(), "enabled");
        }
        // **Diagnostic scaffolding: write every IME event to a file.** Off
        // unless `BT_IME_TRACE` names a path; then each event lands as one
        // line with its instant. It exists for the same reason
        // `BT_CHROME_DUMP` does — a caret standing in the wrong place cannot be
        // grepped, and the question "did the IME say that, or did we" has to
        // be answered from what the IME actually said. Written before any
        // routing so a swallowed event is still on the record.
        hang_watch::during(hang_watch::Station::ImeTrace, || {
            self.trace_ime_input(&event);
        });
        let composing = matches!(event, Ime::Preedit(..) | Ime::Commit(_));
        // **Which rung this composition was started in**, written above every
        // one of them so that the answer is the same one that routes the letters
        // below (§7.1.5a″). A commit is the end of a composition and an empty
        // pre-edit is a cancelled one, so both put it out; `Enabled`/`Disabled`
        // are the window's own bookkeeping and are answered in their own arms.
        if composing {
            self.window.composing = match &event {
                Ime::Preedit(text, _) if !text.is_empty() => Some(ime_owner(self.keyboard_owner())),
                _ => None,
            };
            // **§7.1.5a″ — and the letters have to belong to *this* field**
            // (review 2026-09-17 P2). Above the ladder and not inside one of its
            // arms, because the ladder is *routing*: it answers where text goes
            // now, which is exactly the question a composition that has outlived
            // its field must not be allowed to ask. Every crossing is the same
            // crossing — one terminal to another, a terminal to a page, a page
            // to a terminal, a palette to anything — so one rule stands in front
            // of all of them. See [`composition_ruling`].
            let here = self.composition_origin_now();
            if let Some(what) = composing_event_of(&event) {
                let ruling = composition_ruling(&self.window.composing_in, &here, what);
                self.trace_ime_ruling(&event, &here, ruling);
                let held = std::mem::take(&mut self.window.composing_in);
                self.window.composing_in = held.after(ruling.next, &here);
                if !ruling.deliver {
                    return Ok(());
                }
            }
            match ime_owner(self.keyboard_owner()) {
                // The name editor, through the same two doors every other field
                // in this window uses: a pre-edit is **drawn at its caret and is
                // not in the text**, and a commit is an ordinary insert that
                // replaces the selection — so "typing over the opening selection
                // replaces it" is one rule rather than one rule per input method.
                //
                // **The pre-edit is drawn now** (0.3). It was thrown away for as
                // long as this editor had nowhere to put one: the composition
                // lived in no field and the candidate window was placed from the
                // terminal's caret, so a name typed with an IME appeared one
                // committed syllable at a time with nothing on the glass in
                // between. `TextField` holds it out of the buffer, which is what
                // makes drawing it safe — an Escape that cancels a composition
                // leaves the name exactly as it was, with nothing to un-type.
                ImeOwner::Rename => {
                    let mut editor = self.window.rename.take().expect("the editor is open");
                    match &event {
                        Ime::Preedit(text, _) => editor.field.set_preedit(text),
                        Ime::Commit(text) => editor.insert(text),
                        Ime::Enabled | Ime::Disabled => {}
                    }
                    self.window.rename = Some(editor);
                    self.window
                        .rename_blink
                        .reset(Instant::now(), self.app.motion);
                    self.refresh_chrome();
                    self.present_chrome_change()?;
                    return Ok(());
                }
                // With a modal or a popup up the terminal is not who is being
                // typed at, and a commit is a keystroke that took a longer road.
                ImeOwner::Modal => return Ok(()),
                // **Swallowed, and that is the point.** A column has nothing to
                // type into — but the reason this rung exists is not what it
                // gains, it is what it stops: without it a composition made over
                // a file tree lands in whatever shell is behind the tree.
                ImeOwner::FilesTree => return Ok(()),
                // The search field, through the same two doors the editor uses:
                // a pre-edit is drawn at its caret and is not in the text, and a
                // commit is an ordinary insert that replaces the selection.
                ImeOwner::GraphSearch => {
                    self.graph_search_ime(&event)?;
                    return Ok(());
                }
                // The branch prompt inside a git context menu, through the same
                // two doors: a pre-edit is drawn at its caret and is not in the
                // text, and a commit is an ordinary insert.
                ImeOwner::GitPrompt => {
                    self.git_prompt_ime(&event)?;
                    return Ok(());
                }
                ImeOwner::Preview => {
                    self.preview_ime(event)?;
                    return Ok(());
                }
                // The search capsule, through the same two doors: a pre-edit is
                // drawn at its caret and is not in the text, a commit is an
                // ordinary insert that replaces the selection — and a committed
                // character re-asks the search, exactly as a typed one does.
                ImeOwner::Search => {
                    self.search_ime(event)?;
                    return Ok(());
                }
                // The palette's field, through the same two doors: a pre-edit
                // is drawn at its caret and is **not** in the query, and a
                // commit is an ordinary insert that asks the list again.
                ImeOwner::Palette => {
                    self.palette_ime(&event)?;
                    return Ok(());
                }
                ImeOwner::Shell => {}
            }
            self.reset_cursor_blink(Instant::now());
        }
        match event {
            Ime::Enabled => {
                self.window.ime_active = true;
                // **`composing_in` is deliberately untouched** (review
                // 2026-09-17 P2, round 3). A composition context opening is a
                // notice, not a composition ending — and the order that makes
                // that load-bearing is a real one: a refused cancel, then
                // `Disabled`, then `Enabled` (winit re-enables results on
                // `WM_IME_STARTCOMPOSITION`), then the method's own clearing
                // pre-edit, then its commit. Clearing here would hand that
                // commit to whichever field the keyboard had moved to.
                self.window.ime_cursor_throttle.reset();
                self.publish_frame(FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::Expose,
                })
            }
            Ime::Preedit(text, cursor_range) => {
                // A collapsed range is the caret; an open one is the IME's
                // target clause and not a caret at all — see
                // [`preedit_caret_byte`]. Target-clause styling is still
                // outside scope; the caret simply stops standing on it.
                self.window.preedit = (!text.is_empty()).then_some(Preedit {
                    text,
                    cursor_byte: preedit_caret_byte(cursor_range),
                });
                self.publish_frame(FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::Keyboard,
                })
            }
            Ime::Commit(text) => {
                self.window.preedit = None;
                self.return_to_live_for_input();
                self.pending_keyboard_at = Some(Instant::now());
                // A commit is what you typed, so it answers (`attention` plan §10.3.2 row 2). A
                // *preedit* is not: it puts no byte in the pipe and never reaches this arm at all,
                // which is what keeps "IME counts" from being read as "mid-composition counts".
                self.answer_attention(self.focused_leaf, UserInputKind::Ime);
                // Review row R1-24 — a commit is what the reader typed.
                self.note_user_typing(self.focused_leaf);
                // IMM32 also emits this commit when focus/layout changes mid-composition. M0-beta
                // deliberately accepts it exactly like Windows Terminal: every commit reaches PTY.
                write_pty_input(
                    self.focused().and_then(|leaf| leaf.pty.as_ref()),
                    &ime_commit_bytes(&text),
                    "write IME UTF-8 commit to PTY",
                )?;
                self.publish_frame(FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::Keyboard,
                })
            }
            Ime::Disabled => {
                let drawn_in_the_preview =
                    self.window.preedit.is_some() && self.preview_edit_focus().is_some();
                self.window.preedit = None;
                self.window.composing = None;
                // And `composing_in` is not cleared here either, for the reason
                // `Enabled` gives one arm up: this is the method telling us it
                // has ended a composition, which is precisely what a method that
                // refused the cancel says before sending the commit anyway.
                self.window.ime_active = false;
                self.window.ime_cursor_throttle.reset();
                hang_watch::during(hang_watch::Station::ImeCaretDestroy, || {
                    self.destroy_ime_caret("ime_disabled")
                });
                // A composition taken away must stop being *drawn* where it was
                // drawn. The grid is repainted by the frame below, but the
                // preview paints from the same field on its own pass and would
                // otherwise keep the letters on screen until something else
                // moved. Asked, rather than done unconditionally, because this
                // arrives on every blur and a chrome rebuild is not free.
                if drawn_in_the_preview {
                    self.repaint_preview()?;
                }
                self.publish_frame(FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::Expose,
                })
            }
        }
    }
}
