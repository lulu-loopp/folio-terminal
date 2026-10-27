//! `palette` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::input::Keystroke;
use crate::window_seam::prelude::*;
use crate::{
    ApplicationChange, MenuPaint, Popup, RecallFlash, Runtime, i18n, icons, input,
    install_page_ground_color, install_theme_class_background, palette, pane_folder, profiles,
    settings, shortcuts,
};
use anyhow::Result;
use bt_render::{FrameSource, FrameTrigger, Travel};
use std::time::Instant;
use winit::dpi::PhysicalPosition;
use winit::event::{Ime, MouseScrollDelta};
use winit::keyboard::{Key, NamedKey};

impl<B: Backend> Runtime<'_, B> {
    /// **Everything a new palette costs this window**, whichever door it came
    /// through.
    ///
    /// One function and not a block inside `apply_theme`, because there are now
    /// two doors: the theme flipping, and one of the two schemes being replaced
    /// under a theme that did not move. The second is the one that would have
    /// been missed — its symptom is a terminal in the new colours inside chrome
    /// still wearing the old ones, which reads as a redraw bug rather than as a
    /// step nobody ran.
    pub(crate) fn adopt_new_palette(&mut self) -> Result<()> {
        // Every other window draws out of the same `THEME`, so every other window
        // owes itself this call (multiwindow slice C). Recorded rather than
        // performed here: the sibling windows are not reachable from a `Runtime`,
        // which is one window by construction.
        self.note_application_change(ApplicationChange {
            font: false,
            look: true,
            caret: false,
            option: false,
            paid_by: Some(self.window.window.id()),
        });
        // The window's own colours moved, so what the window has told DWM about
        // them may have gone stale — and DWM's acrylic plate and border are
        // drawn from that statement, not from anything in this process's frame.
        self.apply_window_dark_mode()?;
        install_theme_class_background(&self.window.window);
        // **And the floor under every page in this window** (§7.14). Beside
        // the class brush and not somewhere else, because the two answer the
        // same question about the same ground for two different surfaces —
        // what a resize band shows, and what a web pane shows where the browser
        // has not caught up. Splitting them is how one of the pair gets left
        // behind on a theme flip, which is the whole reason the brush's own
        // hot-swap was written here in §7.1.6c-4b.
        install_page_ground_color(&self.window.compositor);
        // **And what every page in it is told to prefer** (0.4.4 ticket 09): a page's
        // `prefers-color-scheme` follows the same ground, so a theme flip that left it behind
        // would be a light site in a dark window — the picture the ruling of 2026-09-21 was about.
        self.tell_web_pages_their_color_scheme();
        self.sync_math_layout_key();
        // The one thing a rail's key cannot see. A palette is not a fact about a
        // pane, so [`cmdrail::RailKey`] does not carry one — which means a rail
        // built under the old ink would be handed back unchanged, and a light
        // window would keep a dark window's ticks.
        for cache in self.window.command_rails.values_mut() {
            cache.clear();
        }
        // **And every markdown page, for the rail's reason one surface over**
        // (2026-08-28; §7.1.3k ②).
        //
        // A page's *text* recolours for free — the palette is read fresh on
        // every frame and a glyph is drawn in whatever colour it is handed. Two
        // things on a page do not: a formula's raster came out of the engine
        // already inked, and a `<picture>` names **one file for a dark page and
        // another for a light one**. Both are in `PreviewDocumentKey`'s
        // [`PageArtKey`] precisely so that a theme flip is a different layout
        // question — and nothing was asking the question. Without this a light
        // window kept a dark window's hero, exactly as it kept a dark window's
        // ticks until the loop above was written.
        self.refresh_preview_for_layout();
        self.refresh_chrome();
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        Ok(())
    }

    /// **Raise the palette.**
    ///
    /// It closes every other popup on the way up, which is what every opener in
    /// this window does (E61), and it takes the focus with it — see
    /// [`palette::PaletteState::opening`] for why the focus is a photograph
    /// rather than a live reading.
    pub(in crate::runtime) fn open_command_palette(&mut self) -> Result<()> {
        // A second press of the chord puts it away. The mock-up has no such
        // gesture because a web page's palette is dismissed by the click that
        // raised it landing outside; a chord has no outside, so the chord is
        // the way back out as well as in — `toggle` is what every other
        // one-key surface in this product does.
        if self.window.palette.is_some() {
            self.close_command_palette()?;
            return Ok(());
        }
        self.close_popups_except(Popup::Palette);
        let focus = self.shortcut_focus();
        self.window.palette = Some(palette::PaletteState::opening(focus));
        self.ask_for_file_indexes_on_open();
        self.requery_palette()
    }

    /// Put it away, and report whether there was anything to put away — which
    /// is what tells Esc and a press elsewhere whether they consumed anything.
    pub(crate) fn close_command_palette(&mut self) -> Result<bool> {
        if self.window.palette.is_none() {
            return Ok(false);
        }
        self.close_popup(Popup::Palette);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// Every row the five suppliers are offering, before a query is asked.
    fn palette_candidates(&self) -> Vec<palette::Candidate> {
        let mut candidates = Vec::new();
        self.push_action_candidates(&mut candidates);
        self.push_place_candidates(&mut candidates);
        self.push_command_candidates(&mut candidates);
        self.push_file_candidates(&mut candidates);
        self.push_settings_candidates(&mut candidates);
        candidates
    }

    /// Ask the query again and put the answer in.
    ///
    /// `requeried` decides where the selection lands — see
    /// [`palette::PaletteState::refill`].
    fn arrange_palette(&mut self, requeried: bool) -> Result<()> {
        if self.window.palette.is_none() {
            return Ok(());
        }
        self.ask_for_file_indexes();
        let candidates = self.palette_candidates();
        let files_note = self.palette_files_note();
        // **The composed reading and not the buffer** (user ruling 2026-09-05):
        // a composition in progress is part of what the box is asking, so a
        // Chinese query narrows the list while it is being typed rather than
        // only at the moment it commits. See [`palette::PaletteState::query`].
        let query = self
            .window
            .palette
            .as_ref()
            .map(|state| state.query().into_owned())
            .unwrap_or_default();
        let listing = palette::arrange(&candidates, &query, files_note);
        if let Some(state) = self.window.palette.as_mut() {
            state.refill(listing, requeried);
        }
        // A kept row may have moved, and the list may be shorter than the scroll held: the scroll
        // follows the row in the layout the new list makes (T-FRESH-FACTS round 2).
        if !requeried
            && let Some(layout) = self.palette_layout()
            && let Some(state) = self.window.palette.as_mut()
        {
            state.show_selected(&layout);
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The query changed: a new answer, and the selection back at the top.
    fn requery_palette(&mut self) -> Result<()> {
        self.arrange_palette(true)
    }

    /// A supplier changed under a query that did not: a new answer, and the
    /// reader's own row kept.
    pub(crate) fn refresh_palette_listing(&mut self) -> Result<()> {
        self.arrange_palette(false)
    }

    /// What the input is showing, and where its caret is.
    ///
    /// [`Runtime::search_field_look`]'s own shape and its reason: splicing a
    /// preedit into the buffer for display is the owner's job, and the caret is
    /// measured against the very string that will be drawn.
    pub(in crate::runtime) fn palette_field_look(&self) -> (String, String, bool) {
        let Some(state) = self.window.palette.as_ref() else {
            return (String::new(), String::new(), false);
        };
        let field = state.field();
        let before = format!("{}{}", field.before_caret(), field.preedit());
        if field.is_empty() && field.preedit().is_empty() {
            return (
                i18n::Text::PaletteFieldPlaceholder.text().to_owned(),
                String::new(),
                false,
            );
        }
        // The same splice the query is made of, so the string that is measured
        // and the string that is filtered cannot drift apart.
        (field.composed().into_owned(), before, true)
    }

    /// The palette as one band of the overlay.
    pub(in crate::runtime) fn palette_layer(&mut self) -> MenuPaint {
        let Some(layout) = self.palette_layout() else {
            self.window.palette_layout = None;
            return MenuPaint::none();
        };
        let selected = self
            .window
            .palette
            .as_ref()
            .map_or(0, palette::PaletteState::selected);
        let layers = palette::build(&layout, &bt_render::chrome_palette(), selected);
        // The very box that was drawn, kept for the `&self` press router.
        self.window.palette_layout = Some(layout);
        // It travels down, because it arrives from above the reader's line of
        // sight and there is nothing it grew out of to travel away from.
        MenuPaint::plain(layers, Travel::Down)
    }

    /// **The pointer over the palette.**
    ///
    /// Three lines, where the menus with children need thirty: hovering a row
    /// *selects* it (see [`palette::PaletteState::point_at`]), and a pointer
    /// inside the box but on no row leaves the selection where it was rather
    /// than clearing it — a reader whose hand drifted into the gap between two
    /// rows has not stopped aiming at the row they were aiming at.
    ///
    /// Returns whether the pointer was over the box at all, which is what tells
    /// the caller the move has been answered.
    pub(in crate::runtime) fn drive_palette_hover(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let Some(layout) = self.window.palette_layout.clone() else {
            return Ok(false);
        };
        let Some(found) = palette::hit(&layout, position.x, position.y) else {
            return Ok(false);
        };
        if let Some(index) = found
            && self
                .window
                .palette
                .as_mut()
                .is_some_and(|state| state.point_at(index))
            && self.refresh_chrome()
        {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// **A wheel notch on the list** (DESIGN.md §7.55 ⑧).
    ///
    /// Through [`Runtime::vertical_wheel_travel`], which is the one unit every
    /// chrome scroller in this window converts a notch into: the system's own
    /// lines-per-notch measured against this product's line, or a screenful of
    /// *this* box when the reader's setting says a page. So a notch means the
    /// same distance on the palette's list as it does on the rail, the settings
    /// sheet and a files column, and the box does not have to own a number.
    ///
    /// **One scroll and one selection, shared with the keyboard.** The offset
    /// written here is the very field `settle_palette_scroll` writes when `↑`
    /// and `↓` walk the list, so the two never disagree about where the list is;
    /// and because the rows moved under a pointer that did not move, what the
    /// pointer is over changed without the pointer having done anything — the
    /// rail's and the settings sheet's own sentence, and the reason the hover is
    /// re-seated against a layout measured *after* the scroll rather than the
    /// one that was handed in.
    pub(in crate::runtime) fn scroll_palette_list(
        &mut self,
        layout: &palette::PaletteLayout,
        delta: MouseScrollDelta,
    ) -> Result<()> {
        let travel = self.vertical_wheel_travel(delta, layout.list_height());
        let Some(state) = self.window.palette.as_ref() else {
            return Ok(());
        };
        // Wheel-up reveals what lies above, which is a smaller offset.
        let scrolled = (state.scroll() - travel).clamp(0.0, layout.max_scroll());
        if scrolled == state.scroll() {
            return Ok(());
        }
        if let Some(state) = self.window.palette.as_mut() {
            state.set_scroll(scrolled);
        }
        if let Some(position) = self.window.pointer_position
            && let Some(moved) = self.palette_layout()
            && let Some(Some(index)) = palette::hit(&moved, position.x, position.y)
            && let Some(state) = self.window.palette.as_mut()
        {
            state.point_at(index);
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Bring the selected row into the box.
    fn settle_palette_scroll(&mut self) {
        let Some(layout) = self.window.palette_layout.as_ref() else {
            return;
        };
        let Some(state) = self.window.palette.as_ref() else {
            return;
        };
        let scrolled = layout.scroll_to_show(state.selected(), state.scroll());
        if let Some(state) = self.window.palette.as_mut() {
            state.set_scroll(scrolled);
        }
    }

    /// **Every key the palette is up for.**
    ///
    /// Four rules, which are [`Runtime::term_menu_key`]'s four with a field
    /// added: Esc closes, the arrows walk, Enter runs, and everything else goes
    /// into the box. Nothing falls through — a palette that let a keystroke
    /// reach the shell behind it would be a box you cannot type a `p` into.
    pub(in crate::runtime) fn palette_key(&mut self, event: &Keystroke) -> Result<()> {
        if !event.pressed {
            return Ok(());
        }
        // **The chord that raised it puts it away**, and which chord that is
        // is asked of the table rather than written here — so a reader who has
        // rebound the palette closes it with the key they bound, and there is
        // one answer to "what opens the palette" rather than two.
        //
        // Every *other* chord is swallowed with the rest: this is a popup, and
        // a popup owns the keyboard entirely (`popup_takes_the_key`). A window
        // verb firing under a box the reader is typing into would be the box
        // answering a question nobody asked it.
        if let Some(focus) = self
            .window
            .palette
            .as_ref()
            .map(palette::PaletteState::focus)
            && self.app.shortcuts.lookup(
                &event.logical_key,
                &event.key_without_modifiers(),
                self.window.modifiers,
                focus,
            ) == Some(shortcuts::Action::CommandPalette)
        {
            self.close_command_palette()?;
            return Ok(());
        }
        // The application's modifier — see `search_field_key`'s note (M1-7).
        let ctrl = input::is_command_chord(self.window.modifiers);
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                self.close_command_palette()?;
                return Ok(());
            }
            Key::Named(NamedKey::Enter) => {
                self.run_palette_row()?;
                return Ok(());
            }
            Key::Named(NamedKey::ArrowDown) | Key::Named(NamedKey::ArrowUp) => {
                let forwards = matches!(&event.logical_key, Key::Named(NamedKey::ArrowDown));
                if let Some(state) = self.window.palette.as_mut() {
                    state.step(forwards);
                }
                self.settle_palette_scroll();
                if self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
                return Ok(());
            }
            _ => {}
        }

        // The field's own keys. `Tab` is deliberately absent: the mock-up
        // gives it nothing to do, and a completion key in a box whose whole
        // gesture is "type a few letters and press Enter" would be a second
        // way to do the one thing Enter already does.
        //
        // **The clipboard's chord lands in the box, not in the shell behind it**
        // (user ruling 2026-09-05). `Ctrl+V` and `Ctrl+Shift+V` are one verb here:
        // the terminal tells them apart because `Ctrl+V` is a control code a
        // program may want, and a text field has no such reading to protect. It is
        // asked of the clipboard lane, and only for the chord that wants it —
        // reading it is a trip to the platform, and doing that on every keystroke
        // would be a box that opens the clipboard to find out whether you pressed
        // `k`; the line lands through the box's own insert door
        // ([`Self::palette_ime`]) on the turn the clipboard answers. Above the
        // `Ctrl+A` arm and the ordinary character arm, both of which would
        // otherwise answer for `v`.
        if input::is_paste_shortcut(&event.logical_key, self.window.modifiers) {
            return self.paste_into_field(crate::TextFieldSeat::Palette);
        }
        let mut typed = false;
        let mut moved = false;
        if let Some(state) = self.window.palette.as_mut() {
            let field = state.field_mut();
            match &event.logical_key {
                // **An `Alt`-modified character is not typed here**: the palette
                // reads it as a chord somebody meant for something else, and
                // typing its letter would be the box answering a question it was
                // not asked. The one difference from the other fields' reading.
                Key::Character(_) | Key::Named(NamedKey::Space)
                    if self.window.modifiers.alt_key() && !ctrl => {}
                // The editing keys every one-line field shares (AUDIT-hardcode
                // RT-2): the word verbs, select-all, and a chord never typed as
                // its letter (M1-7, X-3 §4 ③).
                key => match input::one_line_field_key(field, key, self.window.modifiers) {
                    Some(input::FieldEdit::Typed) => typed = true,
                    Some(input::FieldEdit::Moved) => moved = true,
                    None => {}
                },
            }
        }
        if typed {
            self.requery_palette()?;
        } else if moved && self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Composition in the palette's field.
    ///
    /// [`Runtime::git_prompt_ime`]'s three arms, with **one difference, and it
    /// is the ruling of 2026-09-05**: a pre-edit still goes to `set_preedit`
    /// and never to `insert` — the buffer must not hold text an Escape would
    /// have to un-type — but the list is asked again on the pre-edit as well as
    /// on the commit, because [`palette::PaletteState::query`] reads the
    /// composition as part of the query. A half-composed `ni'hao` narrows the
    /// box while it is being typed, which is what a filter box does.
    ///
    /// **Cancelling restores by construction and not by a branch**: the IME
    /// announces a cancel as an empty pre-edit, so the same `set_preedit` puts
    /// the reading back to the committed text and the same re-query answers it.
    pub(in crate::runtime) fn palette_ime(&mut self, event: &Ime) -> Result<()> {
        let Some(state) = self.window.palette.as_mut() else {
            return Ok(());
        };
        match event {
            Ime::Preedit(text, _) => state.field_mut().set_preedit(text),
            Ime::Commit(text) => state.field_mut().insert(text),
            Ime::Enabled | Ime::Disabled => return Ok(()),
        }
        self.requery_palette()
    }

    /// **Carry out the selected row.**
    ///
    /// The box closes first, on the mock-up's own note — "close first: the
    /// action may open its own surface" — and because several of these verbs
    /// raise something the palette would otherwise be standing on top of.
    pub(in crate::runtime) fn run_palette_row(&mut self) -> Result<()> {
        let Some(row) = self
            .window
            .palette
            .as_ref()
            .and_then(palette::PaletteState::chosen)
            .map(|row| row.what.verb.clone())
        else {
            return Ok(());
        };
        self.close_command_palette()?;
        match row {
            // The shortcut table's own dispatch, and not a copy of it: a
            // palette row and a chord are two doors onto one room.
            palette::Verb::Run(action) => self.run_shortcut(action),
            palette::Verb::ResetTerminalModes { tab, seat } => {
                let Some(index) = self.tab_index_of(tab) else {
                    return Ok(());
                };
                self.activate_tab(index, false)?;
                if !self.window.tabs[index].sessions.contains_key(&seat) {
                    return Ok(());
                }
                self.focus_seat(seat)?;
                self.reset_terminal_modes(seat)
            }
            palette::Verb::Go { tab, seat } => {
                let Some(index) = self.tab_index_of(tab) else {
                    return Ok(());
                };
                self.activate_tab(index, false)?;
                if self.window.tabs[index].sessions.contains_key(&seat) {
                    self.focus_seat(seat)?;
                }
                if self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
                Ok(())
            }
            palette::Verb::Recall { tab, seat, mark } => {
                let Some(index) = self.tab_index_of(tab) else {
                    return Ok(());
                };
                self.activate_tab(index, false)?;
                if !self.window.tabs[index].sessions.contains_key(&seat) {
                    return Ok(());
                }
                self.focus_seat(seat)?;
                // **What there is to point at is asked when the row runs, not
                // when it was listed** (§7.55 ⑨). The box stays up for as long
                // as somebody is typing into it, and a command that was running
                // when the list was built may have ended by the time Enter is
                // pressed — the same argument that resolves the `TabId` two
                // lines up rather than carrying an ordinal.
                //
                // A mark that has gone in the meantime has no row to light, so
                // it takes the pane's ring: the reader was still moved to a
                // pane, and saying nothing at all after a keypress is the one
                // answer this whole slice exists to stop giving.
                let Some(leaf) = self.window.tabs[index].sessions.get(&seat) else {
                    return Ok(());
                };
                let running = leaf
                    .session
                    .command_mark(mark)
                    .is_none_or(bt_term::CommandMark::is_running);
                let alternate_screen = leaf.session.terminal_modes().alternate_screen;
                match recall_flash(running, alternate_screen) {
                    // The palette is not the rail, so it writes no rail line.
                    RecallFlash::Row => self.jump_to_command_mark(seat, mark).map(|_| ()),
                    RecallFlash::Pane => self.flash_pane(seat),
                }
            }
            // Both halves, because a file the reader asked for should be both
            // shown and findable: the preview opens it, and the column it came
            // out of unfolds to it so the next one is a row away.
            palette::Verb::Open(path) => {
                if let Some(surface) = self.preview_landing_surface() {
                    self.open_preview_onto(surface, path.clone())?;
                }
                self.locate_path_in_files_columns(&path)
            }
            palette::Verb::Adjust(row) => self.open_settings_on_row(row),
        }
    }

    /// **The shortcut table, filtered to what this focus can reach.**
    ///
    /// Read from [`shortcuts::Shortcuts::rows`] rather than from
    /// `editor_rows`, and the difference is the one thing worth writing down:
    /// the editor's projection folds a contiguous family into a single line
    /// (`Ctrl Shift [1 – 9]`) and carries no [`shortcuts::Action`], because it
    /// exists to be *read* on a page. A palette row has to be *run*, and
    /// neither half of that projection survives the trip — a folded family is
    /// nine verbs printed as one, and a line with no action has nothing for
    /// `run_shortcut` to be given.
    fn push_action_candidates(&self, into: &mut Vec<palette::Candidate>) {
        let Some(state) = self.window.palette.as_ref() else {
            return;
        };
        let focus = state.focus();
        for binding in self.app.shortcuts.rows() {
            // A row whose scope does not hold is a verb that would do nothing
            // if it were pressed, and a row whose machine has not arrived is
            // the same thing said a different way. Neither belongs in a list of
            // what this window can do.
            if !binding.scope.holds(focus) || binding.action.is_pending() {
                continue;
            }
            into.push(palette::Candidate {
                section: palette::Section::Actions,
                label: binding.title.text().to_owned(),
                hint: binding
                    .chord
                    .as_ref()
                    .map(|chord| shortcuts::chord_caps(chord).join("+")),
                mark: None,
                awaiting: false,
                verb: palette::Verb::Run(binding.action),
            });
        }
        // The pane menu's recovery verb, which is not a row of the shortcut
        // table: see `palette::reset_terminal_modes_candidate` for when it is
        // offered and how it is addressed.
        let tab = &self.window.tabs[self.window.active_tab];
        let session = tab
            .sessions
            .contains_key(&tab.focused_leaf)
            .then_some(tab.focused_leaf);
        into.extend(palette::reset_terminal_modes_candidate(
            focus, tab.id, session,
        ));
    }

    /// **Every pane of this window**, tab by tab.
    ///
    /// The walk is `jump_to_attention`'s — `tabs × sessions` — because it is
    /// the same question asked with a different filter, and this window has one
    /// answer to "where are the panes".
    ///
    /// **A tab in another window is not on this list** (DESIGN.md §7.55 ②). A
    /// window has no name of its own yet, so a row naming a pane in one would
    /// have to say *which* window with nothing to say it with; the row would be
    /// a place the reader could not tell from the identically-named place in
    /// front of them.
    fn push_place_candidates(&self, into: &mut Vec<palette::Candidate>) {
        for tab in &self.window.tabs {
            for (seat, leaf) in &tab.sessions {
                let label = tab
                    .terminal_name(*seat)
                    .unwrap_or_else(|| tab.display_title());
                let hint = leaf
                    .folder(&pane_folder::SystemProcessFolders)
                    .folder()
                    .map(|cwd| cwd.display().to_string())
                    .or_else(|| {
                        leaf.launch
                            .program
                            .as_ref()
                            .and_then(|path| path.file_name())
                            .map(|name| name.to_string_lossy().into_owned())
                    });
                into.push(palette::Candidate {
                    section: palette::Section::Places,
                    label,
                    hint,
                    mark: Some(profiles::identity_mark(&tab.leaf_profile(*seat))),
                    awaiting: leaf.attention.ledger.ticket().is_some(),
                    verb: palette::Verb::Go {
                        tab: tab.id,
                        seat: *seat,
                    },
                });
            }
        }
    }

    /// **Every command this window has run, newest first.**
    ///
    /// The first aggregation of command marks across panes this product has
    /// had: the rail is one pane's ledger drawn beside that pane, and reaching
    /// for its `Stack` here would be a second aggregation of the same facts.
    /// So the ledgers are read directly (§7.1.6b′'s rule about not writing a
    /// second fold), and the only thing invented is the order.
    ///
    /// The order is the newest end of each ledger interleaved by clock. A
    /// [`std::time::Instant`] is monotonic and process-wide, so marks from two
    /// panes are genuinely comparable; a mark that never began has no clock at
    /// all and sorts last, which is where a command nobody has run belongs.
    fn push_command_candidates(&self, into: &mut Vec<palette::Candidate>) {
        let mut found: Vec<(Option<Instant>, palette::Candidate)> = Vec::new();
        for tab in &self.window.tabs {
            for (seat, leaf) in &tab.sessions {
                let pane = tab
                    .terminal_name(*seat)
                    .unwrap_or_else(|| tab.display_title());
                for mark in leaf.session.command_marks() {
                    // A draft nobody has typed into is not a command that ran.
                    if mark.command_text.is_empty() {
                        continue;
                    }
                    let when = mark.finished_at.or(mark.executed_at);
                    found.push((
                        when,
                        palette::Candidate {
                            section: palette::Section::Commands,
                            label: mark.command_text.clone(),
                            hint: Some(i18n::palette_command_hint(
                                mark.is_running(),
                                mark.exit_code,
                                mark.duration(),
                                &pane,
                            )),
                            mark: None,
                            awaiting: false,
                            verb: palette::Verb::Recall {
                                tab: tab.id,
                                seat: *seat,
                                mark: mark.id,
                            },
                        },
                    ));
                }
            }
        }
        // Newest first, and a mark with no clock behind every mark that has
        // one — `None` is smaller than every `Some`, so reversing the key puts
        // it last without a second comparison to get backwards.
        found.sort_by(|left, right| right.0.cmp(&left.0));
        into.extend(found.into_iter().map(|(_, candidate)| candidate));
    }

    /// **The files under this tab's columns**, out of the bounded index.
    fn push_file_candidates(&self, into: &mut Vec<palette::Candidate>) {
        for root in self.palette_files_roots() {
            let Some(index) = self.app.file_indexes.get(&root) else {
                continue;
            };
            if index.is_empty() {
                continue;
            }
            into.reserve(index.len());
            for file in index.files() {
                into.push(palette::Candidate {
                    section: palette::Section::Files,
                    label: file.name.clone(),
                    hint: Some(file.relative.clone()),
                    mark: Some(icons::ActionIcon::FileObject.mark()),
                    awaiting: false,
                    verb: palette::Verb::Open(file.path.clone()),
                });
            }
        }
    }

    /// **Every row the settings dialog would draw**, on the same list the
    /// dialog itself is built from — so a row this window cannot show is a row
    /// the palette cannot offer either.
    fn push_settings_candidates(&self, into: &mut Vec<palette::Candidate>) {
        for row in settings::visible_rows(self.window.rail.layout) {
            into.push(palette::Candidate {
                section: palette::Section::Settings,
                label: row.title().to_owned(),
                hint: Some(row.category().nav_label().to_owned()),
                mark: Some(icons::ActionIcon::OpenSettings.mark()),
                awaiting: false,
                verb: palette::Verb::Adjust(row),
            });
        }
    }
}

/// The rule itself, as a function of the two facts and nothing else — so it can
/// be read, tested and mutated without a window, a GPU and a shell.
pub(crate) const fn recall_flash(running: bool, alternate_screen: bool) -> RecallFlash {
    if running || alternate_screen {
        RecallFlash::Pane
    } else {
        RecallFlash::Row
    }
}
