//! `math` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{
    AnimationEntry, AnimationFillOutcome, DecorationWorkerCompletion, DocumentMath, Layered,
    MathHoverExit, MathWorkerRequest, MathWorkerResult, ModalBand, Motion, OverlayStack,
    PasteTarget, Popup, PreviewDocument, PreviewMathArtifact, PreviewMathKey, PreviewMathPicture,
    PreviewSurface, PreviewTextCommand, Runtime, TabState, adopt_animation_fill,
    answer_one_formula, answers_for, dispatch_tab_decoration_tasks, document_formulas,
    dump_overlay_frame, first_run, float_trigger_tip, formula_tools, ground_overlay_layers,
    hang_watch, i18n, leaf_session_mut, marks, math_copy_window, math_em_milli, new_tab_tip,
    nonzero_u32, preview, preview_select, preview_text_command, preview_trace, profiles, quit,
    rail_overlay_layer, recoverable_clipboard_write, restore, retire_spent_math_copy, search,
    seats, settings, tooltip, trace_sink, window_layout_key, write_terminal_clipboard_text,
};
use anyhow::Context;
use anyhow::{Result, anyhow};
use bt_layout::SeatId;
use bt_render::{FrameSource, FrameTrigger, MathHit, MathHitTarget, Travel};
use bt_term::{SessionMathTask, normalized_local_image_path_key};
use bt_viewport::{MathBlockAnchor, ViewportFrame};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Instant, SystemTime};
use winit::dpi::PhysicalPosition;
use winit::event::KeyEvent;

impl Runtime<'_> {
    /// What one of those boxes says.
    ///
    /// Split from the boxes because the *geometry* of the tab surface is a pure
    /// function of a layout and two geometries — which is what
    /// [`tab_surface_tip_boxes`] can therefore be tested as — while the words are
    /// a fact about which tab is running what, and only a live window has that.
    ///
    /// An id naming a tab this window does not have says nothing, and
    /// `TooltipAnchors::push` drops an empty string: the two lists are built from
    /// one `self.tabs` on one frame, so the case is unreachable rather than
    /// handled, and answering with nothing is what keeps it from becoming a panic
    /// if that ever stops being true.
    pub(in crate::runtime) fn tab_surface_tip_text(&self, id: tooltip::TooltipAnchorId) -> String {
        match id {
            tooltip::TooltipAnchorId::TabPin(index) => self
                .window
                .tabs
                .get(index)
                .map(|tab| {
                    if tab.pinned {
                        // Solid pin = "it is pinned", and the tip names the verb
                        // *and* the reason, because "Unpin" alone does not
                        // explain why the close button went away (mock-up 4204).
                        i18n::Text::Unpin.text()
                    } else {
                        i18n::Text::Pin.text()
                    }
                })
                .unwrap_or_default()
                .to_owned(),
            // What the `×` closes, which the cross itself cannot say (§7.37).
            // The tab menu's own `Close tab` row spells this verb already, so the
            // tip borrows its words rather than minting a second copy of them —
            // one action, one name, whether it is read off a row or a glyph.
            tooltip::TooltipAnchorId::TabClose(_) => i18n::Text::TabMenuClose.text().to_owned(),
            // H108: both surfaces say the same five words, because it is one
            // action wearing one glyph in two places.
            tooltip::TooltipAnchorId::TabFiles(_) => float_trigger_tip().to_owned(),
            tooltip::TooltipAnchorId::TabIcon(index) => self
                .window
                .tabs
                .get(index)
                // **The fleet's reading, which is the one the mark itself draws**
                // (D34). This asked the focused leaf's `status()` through the
                // deref, which was already half a bug — the mark beside it is
                // built from [`TabState::fleet_progress`] and
                // [`TabState::fleet_working`], so a tab whose *sibling* pane was
                // downloading wore a ring and offered a tip saying nothing. Both
                // aggregates fold over the whole fleet and both have an honest
                // answer for an empty one, so a tab with no shell reports no
                // progress and no work, which is the empty tip — no ledger, and
                // therefore nothing said (§7.1.6h).
                .map(|tab| tooltip::mark_tip(tab.fleet_progress(), tab.fleet_working()))
                .unwrap_or_default(),
            tooltip::TooltipAnchorId::Tab(index) => self
                .window
                .tabs
                .get(index)
                .map(TabState::tooltip_text)
                .unwrap_or_default(),
            tooltip::TooltipAnchorId::NewTab => new_tab_tip(self.default_profile()),
            tooltip::TooltipAnchorId::NewTabMenu => i18n::Text::ChooseProfile.text().to_owned(),
            _ => String::new(),
        }
    }

    /// The text of the line one hit stands on, for the glance card over its tick
    /// (B36).
    ///
    /// Read from the plane the hit names rather than from the frame, because a
    /// tick can point at a line that is nowhere near the viewport — which is the
    /// whole reason a results rail is worth having. History is a binary search
    /// (the frozen deque is ordered by id and holds up to a hundred thousand
    /// lines); the two volatile planes are tens of rows and are walked.
    ///
    /// The volatile planes are re-joined through [`search::live_row`], the same
    /// function that produced the text the match was *found* in, so the card
    /// cannot quote a line the scan never saw.
    pub(crate) fn search_hit_line_text(&self, hit: usize) -> Option<String> {
        let seat = self.window.search.seat()?;
        let line = self.window.search.hits().get(hit)?.line;
        let leaf = self.sessions.get(&seat)?;
        match line {
            bt_viewport::SearchLine::History(id) => {
                let frozen = leaf.session.transcript().frozen();
                let at = frozen.binary_search_by_key(&id, |line| line.id).ok()?;
                Some(frozen[at].text.clone())
            }
            bt_viewport::SearchLine::Staging(id) => leaf
                .session
                .transcript()
                .staged_rows()
                .find(|staged| staged.id == id)
                .map(|staged| search::staged_line(&staged.row.cells).text),
            bt_viewport::SearchLine::Live { row } => leaf
                .session
                .live_rows()
                .get(row as usize)
                .map(|captured| search::live_row(row, &captured.cells).text),
        }
    }

    /// Rebuild the overlay with formula lanes already derived from the frame
    /// this present draws. Keeping the handed lanes as an argument makes the
    /// flattened order — and therefore every `WebHole::above` index — pass
    /// through the same builder as every other overlay rebuild.
    pub(in crate::runtime) fn refresh_overlay_with_formula(
        &mut self,
        now: Instant,
        formula_tools: Vec<marks::OverlayLayer>,
    ) -> bool {
        // Before anything is built, because every layer below is a function of
        // state and this is the state that the pointer, the tree and the window
        // all move.
        self.sync_drop_preview(now);
        // Before the rails are asked for, and for the same reason: this is the
        // frame's own reading of which seats exist, and a cache for a seat that
        // left the tree is a cache nobody may be handed.
        self.sweep_command_rails();
        // And before any bar is laid out: what a recording's bar says about its
        // file is text, and only the face can say how wide it is drawn.
        self.measure_video_meta();
        // The two lowest things the overlay carries, in their own order: P177's
        // veil under the dock drawing's `z-index` 24 and 25, both under a menu's
        // 30. Neither is a surface floating over the window — one is a drawing on
        // the layout and the other is a drawing on one pane — so a dialog that is
        // somehow up while either runs must cover it. See [`ground_overlay_layers`].
        // Lowest of all, because it is the lowest `z-index` the overlay carries:
        // `.rail { z-index: 15 }` against the dock veil's 24. It is not a
        // surface floating over the *window* — it is chrome, and every popup
        // below is entitled to cover it — but it does float over the panes, and
        // one level up from them is all it ever asked for.
        let mut stack = OverlayStack {
            // Lower still than the rail, because a pane's own scroll bar is not
            // a surface floating over the window at all — see
            // [`OverlayStack::preview_bars`].
            preview_bars: self.preview_seat_bar_layers().into(),
            // Directly above them and for the same argument: it belongs to a
            // pane. See [`OverlayStack::video_bars`] for why it cannot be drawn
            // in any earlier lane.
            video_bars: self.preview_seat_video_bars(),
            terminal_bars: self.terminal_bar_layers().into(),
            command_rail: self.command_rail_layers().into(),
            // **The hovered formula band's two marks**, above the command rail
            // and below everything a menu can drop over a pane (owner's ruling
            // 2026-09-14 ②). Empty on every frame no pointer is on a formula,
            // which is almost all of them.
            //
            // **And, under them, the other face of a band that is changing into
            // it** (§7.1.5p ⑪): one block, one lane, and the marks stand on the
            // source text exactly as they stand on the picture it is replacing.
            formula_tools: formula_tools.into(),
            rail: self.rail_overlay_layers(),
            // **Directly above the list it came out of** (§7.1.6b″). At full
            // opacity and never at the rail's fold: the fold is what a panel
            // *leaving* looks like, and a card in flight is a card that is very
            // much here — a column fading out while one of its cards travelled
            // at full strength would be the fold saying one thing and the flight
            // saying another. The two cannot be up together anyway; passing
            // `1.0` states which one would win.
            flight: rail_overlay_layer(&self.window.flight_chrome, 1.0),
            ground: ground_overlay_layers(self.pane_fade_veils(now), self.dock_overlay_layers(now)),
            // `.srchbar { z-index: 30 }` — above the ground drawings, below the
            // schematic. It also stores the capsule's rectangle for the press
            // router, so the box you can press is the box you can see.
            search: self.search_layers().into(),
            // Beside the capsule, and above it in the list for the reason it
            // is above it on the glass: the capsule floats over the pane's
            // own text and this stands in a row the text was moved out of, so
            // the two can never overlap and the order between them says which
            // would win if the arithmetic were ever broken.
            pane_notices: self.notice_layers().into(),
            // **Above the capsule and the strip, below every menu.** It is a
            // notice standing on one pane's own body, and §7.7 ④ puts its Escape
            // 「排在 pane 菜单之上」 — so the paint and the ladder agree by being
            // written in the same order.
            web_sheet: self.web_sheet_layers().into(),
            ..OverlayStack::default()
        };
        // **The notice joins the popup family** (the animation slice, 2026-08-26).
        // It arrives downward because it does: the strip takes a row off the top
        // of a pane's body and the body yields, so what it comes out of is the
        // head above it. One passage for the band and not one per pane — two
        // shells reporting at once is one piece of news arriving, and a second
        // clock would fade the first strip out from under a reader mid-sentence.
        let notices = std::mem::take(&mut stack.pane_notices);
        stack.pane_notices = self.stage(Layered::Notice, notices, Some(Travel::Down), now);
        // **The gate is above the settings dialog**, and that is the one ordering
        // it could have: it is the only surface in this window that stands in
        // front of something already happening, so nothing may cover it. Every
        // other member of the chain below floats over a window that still works.
        // **The quit card is above the gate**, which is the one ordering *it*
        // could have: the gate stands in front of one window's shut and this
        // stands in front of the whole process leaving. It is also the only
        // surface in this window that is drawn from a fact none of the others
        // can see — the application's — so a window whose own gate happened to
        // be up must still show what is being asked of all of them.
        //
        // **Which of them it was is carried out of the chain** (the animation
        // slice, 2026-08-26), because four of these arms are popups that arrive
        // and leave on the window's rhythm, one is a dialog that passes as two
        // bands, and four are surfaces this slice does not touch. The chain is
        // the only place that knows which — a second `if` outside it would be a
        // second opinion about which surface is up.
        let (modal, band) = if let Some(layout) = self.quit_card_layout() {
            let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
            (
                restore::quit_build(
                    &layout,
                    (width as f32, height as f32),
                    self.app.quit.as_ref().and_then(quit::Quit::hover),
                ),
                ModalBand::Fixed,
            )
        } else if let Some(layout) = self.dirty_gate_layout() {
            let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
            (
                restore::gate_build(
                    &layout,
                    (width as f32, height as f32),
                    self.window.dirty_gate.hover(),
                ),
                ModalBand::Fixed,
            )
        } else if let Some(layout) = self.first_run_layout() {
            // **Under the gate and over everything else this window can raise**
            // (§7.56). It is the first thing a machine ever shows, and while it
            // is up it is the top of the Escape ladder; what stands above it is
            // only the two surfaces that stand in front of something already
            // under way, which on a first launch cannot be up at all.
            let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
            (
                first_run::build(
                    &layout,
                    (width as f32, height as f32),
                    self.window.first_run.hover(),
                    self.window.first_run.focus_ring(),
                ),
                ModalBand::Fixed,
            )
        } else if let Some(layout) = self.psreadline_invite_layout() {
            // **Under the gate and over the settings dialog.** The gate stands in
            // front of an action already under way and nothing may cover it; this
            // stands in front of a shell that has just started, which outranks a
            // dialog the user opened on purpose — and the dialog is where the
            // same question keeps a row, so nothing is lost by being covered.
            let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
            (
                restore::invite_build(
                    &layout,
                    (width as f32, height as f32),
                    self.window.psreadline_invite.hover(),
                ),
                ModalBand::Fixed,
            )
        } else if let Some(layout) = self.paste_card_layout() {
            // **The multi-line paste card** (0.4.4 ticket 02), under the invitation and over the
            // settings dialog. It is raised by the reader's own `Ctrl+V` into a shell, which no
            // dialog above it lets through, so the order below it is a formality; what can rise
            // over it is only a surface raised by something other than a key.
            let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
            (
                restore::paste_card_build(
                    &layout,
                    (width as f32, height as f32),
                    self.window.paste_card_hover,
                ),
                ModalBand::Fixed,
            )
        } else if let Some(layout) = self.settings_layout() {
            // The hover and the readings first, then the renderer: a combo whose
            // value outgrows its 118px button is ellipsised, and only the font
            // knows where the cut falls. Same division, and same hoist, as the
            // profile menu's measured hints below.
            let hover = self.window.settings.hover();
            // The ring, not the focus: `focus_ring` is `None` while the keyboard
            // arrived at this control by way of somebody's finger.
            let focus = self.window.settings.focus_ring();
            let values = &self.settings_values();
            let shortcuts = self.app.shortcuts.editor_rows();
            let profile_lines = profiles::page_lines(
                &self.app.profile_programs,
                self.default_profile(),
                self.default_profile_is_automatic(),
            );
            let background_image = self.background_image_name();
            let recording = self.window.settings.recording_state();
            let recording =
                recording.map(|(row, caps, hint)| (row, caps.to_vec(), hint.map(str::to_owned)));
            let recording = recording
                .as_ref()
                .map(|(row, caps, hint)| (*row, caps.as_slice(), hint.as_deref()));
            // **The caret follows the focus and not the ring**: a field
            // somebody clicked into has the keyboard, whether or not the ring is
            // showing.
            let focus_for_caret = self.window.settings.focus();
            // The fields' own text, and the selection the caret is holding in
            // whichever one has the focus. Hoisted with the recorder's caps and
            // for its reason: they are `String`s owned by the panel, and the
            // renderer is borrowed mutably below them.
            let editor = self.window.settings.editor();
            let editor_env: Vec<(String, String)> = editor.map_or_else(Vec::new, |editor| {
                editor
                    .env
                    .iter()
                    .map(|(name, value)| (name.text().to_owned(), value.text().to_owned()))
                    .collect()
            });
            let editor_caret = editor.and_then(|editor| editor.caret_of(focus_for_caret));
            let editor = editor.map(|editor| settings::EditorInk {
                name: editor.name.text(),
                program: editor.program.text(),
                args: editor.args.text(),
                env: &editor_env,
                caret: editor_caret,
            });
            // **The summoned terminal's two strings, joined where both halves are
            // in hand** (§7.54e ⑤ — §7.54b's own arrangement for the sentence
            // about a chord another program is holding, said again about the caps
            // themselves). The panel holds a capture indexed into the shortcut
            // table and the table holds the bound chord; neither knows on its own
            // whether *this* row is the one listening, and this is the one place
            // that does.
            let summon_line = self.summon_shortcut_line();
            let summon_listening =
                summon_line.is_some() && self.window.settings.recording_row() == summon_line;
            let summon_caps: Vec<String> = match (summon_listening, recording) {
                (true, Some((_, caps, _))) => caps.to_vec(),
                _ => summon_line
                    .and_then(|index| shortcuts.get(index))
                    .map(|line| line.caps.clone())
                    .unwrap_or_default(),
            };
            let summon_command = self.window.settings.quake_command().to_owned();
            let summon_command_caret = self
                .window
                .settings
                .caret_of(focus_for_caret)
                .filter(|(held, _, _)| {
                    *held == settings::SettingsTarget::Field(settings::SettingsRow::QuakeCommand)
                })
                .map(|(_, from, to)| (from, to));
            let summon = settings::SummonInk {
                caps: &summon_caps,
                listening: summon_listening,
                command: &summon_command,
                command_caret: summon_command_caret,
            };
            let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
            let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
            (
                settings::build(
                    &layout,
                    hover,
                    focus,
                    values,
                    &shortcuts,
                    &profile_lines,
                    &background_image,
                    recording,
                    editor,
                    summon,
                    &mut measure,
                ),
                ModalBand::Settings,
            )
        } else if let Some(layout) = self.restore_layout() {
            // Above the strip but under no scrim: the prompt floats over a
            // window that already works, which is the whole reason it is
            // allowed to exist (mock-up 2219-2221).
            (
                restore::build(&layout, self.window.restore_prompt.hover()),
                ModalBand::Fixed,
            )
        } else if let Some(layout) = self.profile_menu_layout() {
            let travel = layout.travel();
            (
                {
                    let default = self.default_profile();
                    // Taken before the device is borrowed mutably, and handed over as
                    // a borrow of the store rather than of `self.app`: the measure
                    // closure below holds the GPU for the whole call.
                    let favicons = Rc::clone(&self.app.favicons);
                    let favicons = favicons.borrow();
                    let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
                    let mut measure =
                        |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
                    profiles::build(
                        &layout,
                        &self.app.profile_programs,
                        default,
                        self.window.profile_menu.hover(),
                        self.app.recent.entries(),
                        SystemTime::now(),
                        &favicons,
                        &mut measure,
                    )
                },
                ModalBand::Menu(Popup::Profile, travel),
            )
        } else if let Some(layout) = self.root_menu_layout() {
            // Beside the picker in the same `else if` chain, which is what makes
            // the two mutually exclusive in the *picture* as well as in the
            // state: E61's rule is that one popup is up at a time, and a chain
            // cannot draw both however the flags are set.
            let travel = layout.travel();
            let choices = self
                .window
                .root_menu
                .seat()
                .map(|seat| self.root_choices(seat))
                .unwrap_or_default();
            let current = self
                .window
                .root_menu
                .seat()
                .map(|seat| self.files_state(seat).root)
                .unwrap_or_default();
            let hover = self.window.root_menu.hover();
            let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
            let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
            (
                profiles::root_menu_build(&layout, &choices, &current, hover, &mut measure),
                ModalBand::Menu(Popup::Root, travel),
            )
        } else if self.window.graph_filter_menu.is_some()
            && let Some(layout) = self.graph_filter_menu_layout()
        {
            // The fifth arm of the same chain, and in it for E61's reason: one
            // popup is up at a time, and a chain cannot draw two however the
            // flags are set.
            let travel = layout.travel();
            let surface = self
                .window
                .graph_filter_menu
                .as_ref()
                .map(|menu| menu.surface);
            let hover = self
                .window
                .graph_filter_menu
                .as_ref()
                .and_then(|m| m.hover.clone());
            let filter = surface
                .and_then(|surface| {
                    self.window.tabs[self.preview_tab_index(surface)]
                        .git_graph_view
                        .get(&surface)
                })
                .map(|view| view.filter.clone())
                .unwrap_or_default();
            (
                profiles::git_filter_menu_build(&layout, &filter, hover.as_ref()),
                ModalBand::Menu(Popup::GraphFilter, travel),
            )
        } else if let Some(seat) = self.preview_menu_seat()
            && let Some(layout) = self.preview_menu_layout()
        {
            // The fourth arm of the same chain, and it is in the chain for E61's
            // reason: one popup is up at a time, and a chain cannot draw two
            // however the flags are set.
            let travel = layout.travel();
            let items = self.preview_menu_items(seat);
            let hover = self.window.preview_menu.hover();
            let favicons = Rc::clone(&self.app.favicons);
            let favicons = favicons.borrow();
            (
                profiles::preview_menu_build(&layout, &items, hover, &favicons),
                ModalBand::Menu(Popup::Preview, travel),
            )
        } else {
            (Vec::new(), ModalBand::Fixed)
        };
        // **The band, put through whatever passage it is in** — and the bands
        // that are *not* on it asked for the pictures they left behind, which is
        // the frame a departure begins on.
        let holding = band.holding();
        let live = match band {
            // Nothing this slice animates: the quit card, the dirty gate, the
            // PowerShell invite and the restore prompt are drawn exactly as they
            // were, and an empty band is empty.
            ModalBand::Fixed => modal.into(),
            ModalBand::Menu(popup, travel) => {
                self.stage(Layered::Popup(popup), modal.into(), Some(travel), now)
            }
            ModalBand::Settings => {
                // `settings::build` puts the scrim on the first layer and the
                // dialog on the ones after it (its own red gate), which is what
                // lets the dimming stand still while the dialog travels four
                // pixels out of nowhere.
                let mut layers = modal;
                let dialog = layers.split_off(1);
                // **The scrim does not travel** — `None` and not a direction,
                // because it did not come from anywhere: it is the window
                // itself going dark, and a dimming that slid four pixels would
                // be the whole window sliding under the dialog on it.
                let mut painted = self.stage(Layered::SettingsScrim, layers.into(), None, now);
                let dialog = self.stage(
                    Layered::SettingsDialog,
                    dialog.into(),
                    Some(Travel::Down),
                    now,
                );
                painted.append(dialog);
                painted
            }
        };
        // **Under whatever is live**, and that ordering is the whole of it: a
        // menu that is going and a menu that is coming are one gesture seen from
        // both ends (E61 — the opener closes the others), and what a hand is
        // reaching for is the one that is arriving.
        stack.modal = marks::Band::default();
        for leaving in Layered::MODAL_BANDS {
            if !holding.contains(&leaving) {
                let ghost = self.stage(leaving, marks::Band::default(), None, now);
                stack.modal.append(ghost);
            }
        }
        stack.modal.append(live);
        stack.layout_peek = self.layout_peek_layer().into();
        // **Read after the last group under the floats and before the floats
        // themselves** (§7.14c): this is the offset every floated page's hole is
        // spelled against, and the one place the stack's order and that index
        // are the same statement.
        let below_floats = stack.below_the_floats();
        stack.float = self.float_layer(now, below_floats);
        // The five menus with bands of their own, each through its own passage —
        // and each with the child it may be holding out through a second, for
        // [`Layered::Submenu`]'s reason. A band handed in empty is a menu that
        // has closed, which is the whole of what starts a departure.
        stack.file_menu = self.stage_menu(Popup::File, now);
        stack.pane_menu = self.stage_menu(Popup::Pane, now);
        stack.git_menu = self.stage_menu(Popup::GitMenu, now);
        stack.term_menu = self.stage_menu(Popup::TermMenu, now);
        stack.tab_menu = self.stage_menu(Popup::Tab, now);
        stack.palette = self.stage_menu(Popup::Palette, now);
        stack.toast = self.toast_layer();
        // The card and the tip keep the entrances they were written with — 90ms
        // of their own — and gain only the way out, which neither had.
        let key_hint = self.key_hint_layer();
        stack.key_hint = self.stage_departure(Layered::KeyHint, key_hint, now);
        // And the Cards bubble takes **both** halves from the register, because
        // it was written without either: it grows out of the card its tail bites,
        // travelling from that direction, and leaves as a picture over the fast
        // span like every other layer this window puts down.
        let (card_hint, card_hint_travel) = self.card_hint_layer(now);
        stack.card_hint = self.stage(Layered::CardHint, card_hint.into(), card_hint_travel, now);
        let tooltip = self.tooltip_layer();
        stack.tooltip = self.stage_departure(Layered::Tip, tooltip, now);
        // **Read before the card's own layers go in, and for
        // [`OverlayStack::below_the_floats`]'s reason** (§7.44 ③): a video
        // playing on the card is drawn into a slot of the card's own group, and
        // this is the one place the stack's order and that index are the same
        // statement. Handed in exactly as `below_the_floats` is handed to
        // `float_layer`, and the group writes its own index because only the
        // group knows where inside itself the slot went.
        let below_peek = stack.below_the_file_peek();
        stack.file_peek = self.file_peek_layer(below_peek, now);
        stack.drag_ghost = self.drag_ghost_layer();
        stack.window_ring = self.window_ring_layer().into();
        let flattened = stack.flattened();
        dump_overlay_frame(&flattened);
        let marks::Band { layers, groups } = flattened;
        let layers = self
            .window
            .settings_marks
            .resolve_overlay(layers, &bt_render::chrome_palette());
        // **Where each layer stands, kept for the pages under them** (M4-3).
        // Read here because this is the one place the stack's order is settled,
        // which is the order `WebHole::above` is an index into - the same
        // sentence `below_the_floats` is read for a few lines up. Read through
        // the groups (ticket 46): a surface's fade and travel are on its span
        // now, not in its layers, so a menu leaving at nothing covers no page
        // and one arriving stands where it is drawn.
        self.window.overlay_bounds = bt_render::overlay_layer_bounds(&layers, &groups);
        self.window.renderer.set_modal_overlay(layers, groups)
    }

    /// Point the "Display formulas" switch at `enabled` (user ruling 2026-08-10).
    ///
    /// Every shell in every tab, for the same reason a DPI change reaches them
    /// all: this is a fact about how the user wants a terminal to look, not about
    /// one pane. The sessions keep every detection record they hold — the switch
    /// only decides whether a proven raster is allowed to become a band — so a
    /// flip costs one frame and nothing is re-scanned or re-rasterized.
    ///
    /// The write is immediate rather than debounced (§1.1): the file exists apart
    /// from `session.json` precisely so a click in this dialog is on disk before
    /// the user can close the window on it.
    pub(crate) fn apply_display_formulas(&mut self, enabled: bool) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.display_formulas = enabled;
        if !self.app.settings_store.store(settings) {
            return Ok(false);
        }
        for tab in &mut self.window.tabs {
            for (_, leaf) in tab.leaves_mut() {
                leaf.session.set_display_math_bands(enabled);
            }
        }
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        Ok(true)
    }

    /// Point the "Inline formulas" switch at `enabled` (user ruling 2026-08-10).
    ///
    /// Every shell in every tab and the same immediate write to disk, for the
    /// same reasons [`Self::apply_display_formulas`] gives. What differs is the
    /// cost, and it is worth being honest about: this switch gates *detection*,
    /// so each session re-scans rather than merely re-projecting. That is not an
    /// oversight — an inline run that is never detected leaves no record behind
    /// to re-arm, so the alternative to re-scanning is answering with a verdict
    /// the user just revoked.
    pub(crate) fn apply_inline_formulas(&mut self, enabled: bool) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.inline_formulas = enabled;
        if !self.app.settings_store.store(settings) {
            return Ok(false);
        }
        for tab in &mut self.window.tabs {
            for (_, leaf) in tab.leaves_mut() {
                leaf.session.set_inline_math_bands(enabled);
            }
        }
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        Ok(true)
    }

    /// **What a rendered page's selection would put on the clipboard**, or
    /// `None` when that is nothing.
    ///
    /// The same question the terminal's menu asks before it offers `Copy`, asked
    /// of the same thing: not "is there a pair of places" but "are there bytes
    /// between them". A bare click has places and no bytes.
    pub(in crate::runtime) fn preview_selected_text(
        &self,
        surface: PreviewSurface,
    ) -> Option<String> {
        // **One selection model, so one answer** (research §10 Q3, ruled). While
        // a caret is standing in this page the range is the caret's, and what
        // goes on the clipboard is the file's own bytes between its two ends.
        //
        // **That is a departure from §7.31 ⑥ and it is a narrow one.** "Copy
        // what you read" still decides the *range* — the run of the document the
        // two ends name is the run the reader dragged across — and what changes
        // is that the marks inside it come with it: the `#` of the heading, the
        // `>` of the quote, the `**` around the bold word. They come because
        // they are the characters the caret was dragged over. That is what an
        // editor's copy is, it is what a paste of the result puts back, and it
        // is the only answer that lets `Ctrl+X` be the inverse of `Ctrl+V` on a
        // surface where both are now possible. A page with no caret in it — one
        // being read rather than edited — copies what it draws, exactly as it
        // always has, through the piece walk below.
        if let Some(caret) = self.preview_live_caret(surface)
            && !caret.is_empty()
        {
            let content = self.preview_buffer_on(surface)?.content.as_deref()?;
            let text = caret.selected(content);
            return (!text.is_empty()).then(|| text.to_owned());
        }
        let pane = self.preview_pane(surface)?;
        let selection = pane.md_select?;
        let PreviewDocument::Markdown { blocks, .. } = &pane.doc else {
            return None;
        };
        let pieces = preview_select::pieces(blocks);
        let (start, end) = selection.range(&pieces);
        let text = preview_select::copy_text(&pieces, start, end);
        (!text.is_empty()).then_some(text)
    }

    /// Put a rendered page's selection on the clipboard, through the door the
    /// terminal's own copy already uses.
    pub(in crate::runtime) fn copy_preview_text_selection(
        &mut self,
        surface: PreviewSurface,
    ) -> bool {
        let Some(text) = self.preview_selected_text(surface) else {
            return false;
        };
        if let Err(error) = write_terminal_clipboard_text(&text) {
            // Recoverable, on `recoverable_clipboard_write`'s own terms: the
            // selection stays standing so the reader can try again.
            eprintln!("recoverable preview copy failure: {error:#}");
            return false;
        }
        true
    }

    /// **One key aimed at a rendered page's selection**, or `false` if it was
    /// not one of the three.
    pub(in crate::runtime) fn preview_text_key(
        &mut self,
        surface: PreviewSurface,
        event: &KeyEvent,
    ) -> Result<bool> {
        if !matches!(
            self.preview_pane(surface).map(|pane| &pane.doc),
            Some(PreviewDocument::Markdown { .. })
        ) {
            return Ok(false);
        }
        let Some(command) = preview_text_command(&event.logical_key, self.window.modifiers) else {
            return Ok(false);
        };
        match command {
            PreviewTextCommand::Copy => {
                self.copy_preview_text_selection(surface);
            }
            PreviewTextCommand::SelectAll => {
                let selection = match &self.preview_pane(surface).map(|pane| &pane.doc) {
                    Some(PreviewDocument::Markdown { blocks, .. }) => {
                        preview_select::select_all(&preview_select::pieces(blocks))
                    }
                    _ => None,
                };
                if selection.is_none() {
                    // A page with no words in it: the key is still the page's,
                    // and there is simply nothing to take.
                    return Ok(true);
                }
                self.preview_pane_mut(surface).md_select = selection;
                self.repaint_preview()?;
            }
            PreviewTextCommand::Clear => {
                if self
                    .preview_pane(surface)
                    .is_none_or(|pane| pane.md_select.is_none())
                {
                    // Nothing to let go of — `Esc` falls through to whatever
                    // else on this surface wants it.
                    return Ok(false);
                }
                self.preview_pane_mut(surface).md_select = None;
                self.repaint_preview()?;
            }
        }
        Ok(true)
    }

    /// Everything about a document that a pane's width cannot change.
    ///
    /// **The expensive pass, run once per content change.** Every shaping call in
    /// here is a fact about the document — a table column is its own widest cell,
    /// a fence is its longest line — and running it per width is what the resize
    /// report was about.
    /// Ask the engine for every formula this document needs, and hand back the
    /// ones that have already arrived.
    ///
    /// **Both halves in one pass, and neither is a side effect of the other.**
    /// The question "what does this page need" is answered by walking the page,
    /// and every answer is either in hand or has to be asked for — so the walk
    /// that collects the pictures is the same walk that discovers the gaps. A
    /// separate "request" pass run at some other moment would need a rule for
    /// when to run it, and there is no such rule that is right: a formula's
    /// picture depends on the pane's scale and the theme's ink as well as on the
    /// file, and any of those three can change with nothing else changing.
    ///
    /// Asking is idempotent: a gap becomes [`PreviewMathArtifact::Pending`] the
    /// moment it is sent, so the second page showing the same formula finds it
    /// already in flight and the hundredth frame does not send it again.
    ///
    /// **And the answers this page already has are part of the question**
    /// (§7.1.3u ③) — see [`answer_one_formula`], which is what reads them.
    /// [`Self::resolve_document_pictures`]' own sentence, said one lane over:
    /// the cache is bounded and the page's appetite is not, so whether the cache
    /// still holds a picture must not decide whether the engine is asked for one.
    pub(in crate::runtime) fn resolve_document_math(
        &mut self,
        blocks: &[preview::MarkdownBlock],
        metrics: seats::PreviewMarkdownMetrics,
        palette: &bt_render::ChromePalette,
        standing: &DocumentMath,
    ) -> DocumentMath {
        // **The prose's own ink, which on this page is `files_row_text` and not
        // `preview_body_text`** — the second is the heavier ink a heading and a
        // bold run are set in. A formula standing in a sentence is set in the
        // sentence's colour; one standing on its own is set in the page's.
        let foreground_rgb = palette.files_row_text;
        self.window.preview_math.tick = self.window.preview_math.tick.saturating_add(1);
        let mut document = DocumentMath::default();
        let (mut formulas, mut drawn, mut asked) = (0_usize, 0_usize, 0_usize);
        for (source, mode, em_px) in document_formulas(blocks, metrics) {
            let key = PreviewMathKey {
                source,
                mode,
                em_milli_px: math_em_milli(em_px),
                foreground_rgb,
            };
            let mut needs_typesetting = false;
            let answer = answer_one_formula(
                &mut self.window.preview_math,
                standing,
                &key,
                &mut needs_typesetting,
            );
            formulas += 1;
            if let Some(picture) = answer {
                drawn += 1;
                document.insert(&key, picture);
            }
            // Spent the moment the borrow above ends: the door wants the whole
            // runtime and that wants one of its caches — [`answer_one_picture`]'s
            // arrangement, for its reason.
            if needs_typesetting {
                asked += 1;
                self.request_preview_math(key);
            }
        }
        // **Why a page is standing on its source text** (M2-7, §13.40). A
        // formula that has not been typeset yet and one the engine refused draw
        // the identical thing — the author's own LaTeX — and from outside the
        // window the two are one picture. This is the line that tells them
        // apart, and it is silent unless a page has a formula in it at all.
        if formulas > 0 {
            let worker = u8::from(self.app.math_worker_running);
            preview_trace::emit(preview_trace::global(), || {
                format!("math formulas={formulas} drawn={drawn} asked={asked} worker={worker}")
            });
        }
        document
    }

    /// Send one formula to the engine, and write down that it is in flight.
    ///
    /// The two happen together or not at all: a key marked pending against a
    /// request that was never sent is a formula that stands on its source text
    /// for the life of the window with nobody left to ask.
    fn request_preview_math(&mut self, key: PreviewMathKey) {
        if !self.app.math_worker_running {
            return;
        }
        let leaf = self.focused_shell_address();
        if self
            .app
            .math_worker
            .tasks
            .send(MathWorkerRequest::PreviewMath {
                leaf,
                key: Box::new(key.clone()),
            })
            .is_ok()
        {
            self.window.preview_math.mark_pending(key);
        }
    }

    pub(in crate::runtime) fn preview_doc_text_geometry(
        &self,
        surface: PreviewSurface,
        body: [f32; 4],
        scale: f32,
        rows_height: f32,
        columns: usize,
        advance: f32,
    ) -> seats::PreviewMonoGeometry {
        seats::preview_mono_geometry(
            body,
            seats::preview_text_metrics(scale),
            rows_height,
            columns,
            advance,
            self.preview_pane(surface)
                .map_or([0.0, 0.0], |pane| pane.scroll),
        )
    }

    pub(crate) fn apply_math_results(
        &mut self,
        batch: &mut Vec<MathWorkerResult>,
        lane_gone: bool,
    ) -> Result<()> {
        let mut changed = lane_gone;
        // **A formula that lands owes a rebuild to whatever was standing on its
        // source text** (user report, 2026-08-26; widened by M2-7, §13.40).
        //
        // Neither surface is rebuilt by a publish. The card is an overlay layer
        // and `publish_frame` presents what the last `refresh_overlay` composed;
        // a docked pane's body is built by `refresh_preview_body`, which a
        // resize, a scroll, an edit, an open and a palette change reach and a
        // picture landing did not. Either way a picture that arrived after its
        // reader had settled was filed in the window's cache, ticked the
        // generation nothing was going to read, and left the block standing on
        // its LaTeX until some unrelated gesture happened along.
        // `complete_peek_page` says the same sentence about a page.
        //
        // Gathered across the drain and asked once at the end rather than per
        // completion: a page of forty formulas answers forty times, and forty
        // rebuilds of every layer in the window would be the cost of one card.
        let mut picture_landed = false;
        for completion in answers_for(batch, |result| self.owns(result.owner())) {
            let leaf = completion.leaf.leaf;
            let target_index = self.window.tabs.iter().position(|tab| tab.id == leaf.tab);
            let target_active = target_index == Some(self.window.active_tab);
            // The answer goes back to the shell that asked for it. Since U12 that is a
            // seat and not merely a tab: routing through the tab's `Deref` handed every
            // pane's work to whichever pane happened to hold the keyboard when it landed.
            // A pane closed while its work was in flight simply has no session to take it.
            changed |= match completion.completion {
                DecorationWorkerCompletion::Math { task, result } => match *task {
                    // **A table's picture is made here and not on the worker thread.** The
                    // worker did the half that is genuinely off-thread — proving the block
                    // — and stopped, because the other half needs the shaper that lives on
                    // this one. What comes back for a table is therefore an empty raster
                    // and the real extent is measured now, before the session is told
                    // anything, so the record it stores is the size the block will draw at.
                    SessionMathTask::Frozen(task) => {
                        let result = if task.span.kind == bt_detect::BlockKind::Table {
                            // Measured at the asking pane's own size (ticket 37).
                            let font_size_px = target_index
                                .and_then(|index| self.window.tabs[index].sessions.get(&leaf.seat))
                                .map_or_else(
                                    || self.window.renderer.base_metrics().font_size_px,
                                    |asking| asking.metrics.font_size_px,
                                );
                            self.table_raster(&task.span.render_source, font_size_px)
                        } else {
                            result
                        };
                        target_index.is_some_and(|index| {
                            let applied = leaf_session_mut(&mut self.window.tabs, index, leaf.seat)
                                .is_some_and(|session| {
                                    session.complete_worker_result(task, result)
                                });
                            target_active && applied
                        })
                    }
                    SessionMathTask::Live(task) => {
                        let result = if task.span.kind == bt_detect::BlockKind::Table {
                            // Measured at the asking pane's own size (ticket 37).
                            let font_size_px = target_index
                                .and_then(|index| self.window.tabs[index].sessions.get(&leaf.seat))
                                .map_or_else(
                                    || self.window.renderer.base_metrics().font_size_px,
                                    |asking| asking.metrics.font_size_px,
                                );
                            self.table_raster(&task.span.render_source, font_size_px)
                        } else {
                            result
                        };
                        target_index.is_some_and(|index| {
                            let applied = leaf_session_mut(&mut self.window.tabs, index, leaf.seat)
                                .is_some_and(|session| {
                                    session.complete_live_worker_result(task, result)
                                });
                            target_active && applied
                        })
                    }
                },
                DecorationWorkerCompletion::InlineImage { task, result } => {
                    if target_active {
                        self.remember_decode_for_peek(&task, result.as_ref().ok());
                    }
                    target_index.is_some_and(|index| {
                        let applied = leaf_session_mut(&mut self.window.tabs, index, leaf.seat)
                            .is_some_and(|session| {
                                session.complete_inline_image_result(task, result)
                            });
                        target_active && applied
                    })
                }
                DecorationWorkerCompletion::ScaleInlineImage { scaled } => target_index
                    .is_some_and(|index| {
                        let applied = leaf_session_mut(&mut self.window.tabs, index, leaf.seat)
                            .is_some_and(|session| session.complete_inline_image_scale(scaled));
                        target_active && applied
                    }),
                // **Not gated on the asking tab still being the one on
                // screen.** The two completions below are the window's,
                // not a seat's: the decode lands in `peek_cache`, which
                // is one map per window and keyed by path, and the
                // resample is claimed by whichever picture is holding
                // that exact target. Dropping either because the tab
                // changed left the ledger entry it answers — a
                // `PeekCacheEntry::Pending`, a `PreviewImageState`'s
                // `pending` — set for good, and a picture whose question
                // is permanently outstanding is a picture that never
                // arrives.
                DecorationWorkerCompletion::PeekImage { path, result } => {
                    self.complete_peek_image(path, result)?;
                    // Peek state never enters frames, so no republish is needed.
                    false
                }
                DecorationWorkerCompletion::PeekAnimation { path, frames } => {
                    // **Filed whether or not it decoded**, which is what stops a
                    // `.gif` that is one still frame — or one too large to hold
                    // — being asked for again on every pointer move for as long
                    // as it is on the glass. A refusal is an answer.
                    let key = normalized_local_image_path_key(&path);
                    // **Where a playback is named** (adversarial review
                    // 2026-09-11, B3). Every arrival here is a file opened —
                    // the first time this window looked inside it, or a surface
                    // that has just begun showing it — so every arrival is a new
                    // playback and gets a number no other playback in this
                    // process has.
                    let serial = self.app.next_animation_serial();
                    self.window.animations.insert(
                        key,
                        match frames {
                            Ok(animation) => AnimationEntry::Ready {
                                serial,
                                animation: Box::new(animation),
                            },
                            // **Said where a reader is standing**, and not only
                            // to a console nobody has open: the refusal is filed
                            // with its reason and the foot of the pane prints
                            // the two that are worth printing — see
                            // [`AnimationEntry::Refused`].
                            Err(refusal) => AnimationEntry::Refused(refusal),
                        },
                    );
                    self.refresh_video_layers();
                    self.present_chrome_change()?;
                    false
                }
                // **An animation's next frames, and its decoder** (user report
                // 2026-09-10).
                //
                // Taken out and put back rather than reached into, for the one
                // reason `BoundedCache` states in its own note: an entry is
                // weighed when it goes in, so a ring that grew under the map
                // would be pixels the ceiling never counted. Going through the
                // door recounts them.
                //
                // **An animation the map has since let go of is let go of
                // again.** A fill that arrives for a key that is no longer there
                // is frames of a file no surface is showing, and re-inserting it
                // would be an eviction undone by its own answer.
                //
                // **And a fill that answers a playback this window has moved on
                // from is let go of the same way** (adversarial review
                // 2026-09-11, B10). The key names the *file*; since B8 a surface
                // that begins showing a file re-opens it, so the entry under
                // that key may be a second playback standing on frame zero.
                // Parking the old cursor into it would hand a fresh animation a
                // decoder halfway through the file and a ring of frames from the
                // middle of it — the picture jumping to wherever the last
                // playback had got to, on the frame the reader expected it to
                // start. The serial is what makes those two answers
                // distinguishable at all.
                DecorationWorkerCompletion::AnimationFill {
                    path,
                    serial,
                    cursor,
                    frames,
                } => {
                    let key = normalized_local_image_path_key(&path);
                    if adopt_animation_fill(
                        &mut self.window.animations,
                        key,
                        serial,
                        cursor,
                        frames,
                    ) == AnimationFillOutcome::FileChanged
                    {
                        // **The file was rewritten under the loop.** The
                        // playback is gone and nothing else would notice: the
                        // pass that opens animations runs off the video layers,
                        // and with no entry under this key there is no animation
                        // to keep the loop awake. So it is asked here, once, and
                        // what comes back is the new file standing on frame zero.
                        self.refresh_video_layers();
                        self.present_chrome_change()?;
                    }
                    false
                }
                // **A video's frame**, on exactly the terms above: it lands in the same cache, is
                // claimed by the same file rather than by the same asker, and moves no frame of
                // its own — see [`Self::complete_peek_video_frame`].
                DecorationWorkerCompletion::PeekVideoFrame { path, glance } => {
                    self.complete_peek_video_frame(path, glance)?;
                    false
                }
                // **The glance card's page**, on the same reasoning and with one difference: unlike
                // the flyout's decode it *is* drawn by the chrome, so a page that has arrived owes
                // the overlay a rebuild. That is asked below rather than here.
                DecorationWorkerCompletion::PeekPage {
                    path,
                    page,
                    fit,
                    outcome,
                } => {
                    self.complete_peek_page(&path, page, fit, outcome)?;
                    false
                }
                // **Not gated on the asking tab either, and for the stronger
                // reason**: the answer is not addressed to a tab at all. It is
                // filed against its own content, and the pages that were waiting
                // for it — however many, in whichever tabs — pick it up the next
                // time they resolve. `true`, because a page that has been
                // standing on its source text now has a picture to draw and
                // nothing else will ask for a frame.
                DecorationWorkerCompletion::PreviewMath { key, result } => {
                    // The other half of the `math` station (§13.40): one line
                    // per answer, so "the picture is late" and "the engine
                    // refused it" stop being the same picture on the glass.
                    let (set, mode, em_milli, chars) = (
                        u8::from(result.is_ok()),
                        key.mode,
                        key.em_milli_px,
                        key.source.chars().count(),
                    );
                    preview_trace::emit(preview_trace::global(), || {
                        format!(
                            "math answered set={set} mode={mode:?} em_milli={em_milli} chars={chars}"
                        )
                    });
                    let artifact = match result {
                        Ok(raster) => PreviewMathArtifact::Ready(PreviewMathPicture {
                            key: key.texture_key(),
                            rgba: Arc::from(raster.rgba.into_boxed_slice()),
                            width_px: raster.width_px,
                            height_px: raster.height_px,
                            baseline_px: raster.baseline_px,
                        }),
                        Err(_) => PreviewMathArtifact::Refused,
                    };
                    // Only a picture moves anything. A refusal leaves the block
                    // drawing exactly what it was drawing — the source the author
                    // wrote — so it owes nobody a frame, which is the same reason
                    // it does not tick the generation.
                    picture_landed |= matches!(artifact, PreviewMathArtifact::Ready(_));
                    self.window.preview_math.land(*key, artifact);
                    true
                }
                DecorationWorkerCompletion::PeekScaledImage { scaled } => {
                    if target_active {
                        self.complete_peek_scale(scaled)?;
                    }
                    false
                }
                DecorationWorkerCompletion::PreviewScaledImage { scaled } => {
                    self.complete_preview_scale(scaled)?;
                    false
                }
                // A page's picture just got sharp. Nothing about the document
                // changed and nothing about its heights did either — the block
                // was already reserved at the size this raster fills — but the
                // page has to be built again to hand the new pixels over, which
                // is what the generation this landing ticks arranges.
                DecorationWorkerCompletion::MarkdownScaledImage { scaled } => {
                    self.complete_markdown_raster(scaled);
                    true
                }
                // A verdict belongs to the seat that asked: the ledger is per pane, because
                // the directory relative text is measured from is per pane. Unlike the two
                // above it *is* gated on the tab still being on screen — a pane nobody can
                // see has no frame to redraw, and the answer is kept either way.
                DecorationWorkerCompletion::VerifiedPath { path, verdict } => target_index
                    .is_some_and(|index| {
                        let drew = leaf_session_mut(&mut self.window.tabs, index, leaf.seat)
                            .is_some_and(|session| {
                                session.complete_path_verification(path, verdict)
                            });
                        target_active && drew
                    }),
            };
        }
        let active = self.window.active_tab;
        let tasks = self.app.math_worker.tasks.clone();
        let scale_tasks = self.app.math_worker.scale_tasks.clone();
        let path_tasks = self.app.math_worker.path_tasks.clone();
        let window = self.window_id();
        dispatch_tab_decoration_tasks(
            window,
            &mut self.window.tabs[active],
            &tasks,
            &scale_tasks,
            &path_tasks,
            &mut self.app.math_worker_running,
            &mut self.app.math_worker_notice_pending,
        );
        // The card re-lays its document out of the same cache the picture just
        // landed in, so this is the whole of "the card first, the picture when it
        // comes": nothing is held back waiting for the engine, and the block that
        // was standing on its LaTeX is drawn as a picture on the next frame.
        // Asked only while a card is actually up — `file_peek_subject` is the one
        // gate that says so, and asking it is what keeps a document nobody is
        // hovering from paying for an overlay rebuild per formula.
        if picture_landed && self.file_peek_subject().is_some() && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        // **And the page in the pane, which is the same sentence one surface
        // over** (M2-7; `docs/DESIGN.md` §13.40). The block above says "the card
        // first, the picture when it comes" and arranges it for a *card*; a
        // docked preview pane was left with only the publish below, and a
        // publish is not a rebuild — it asks the window to draw again out of the
        // bodies it is already holding, and the body that was built while the
        // formula was still pending holds the author's LaTeX.
        //
        // `PreviewMathCache::generation` exists precisely to make the next
        // rebuild find the picture, and nothing on this road was asking for that
        // rebuild: `refresh_preview_body` is reached from a resize, a scroll, an
        // edit, an open and a palette change, and a formula landing is none of
        // those. So a page that had **settled** before its picture arrived stood
        // on its source text for as long as the reader left it alone.
        //
        // It has always been a race and the port is what lost it: the Mac's
        // first typesetting of a session takes about 1.7 s (the engine's font
        // book is built on the worker thread before the first answer), while
        // the startup's own layout passes are done inside 200 ms — so on that
        // machine the answer *always* lands after the page has settled, and
        // §M2's "the integral is typeset" row failed on every run. A slow first
        // formula on any machine is the same defect.
        //
        // Gated on a picture rather than on `changed`, and on the same flag the
        // card is: a refusal leaves the block drawing exactly what it was
        // drawing, so it owes nobody a rebuild. Before the publish, so the frame
        // that goes out carries the new body rather than the one after it.
        if picture_landed {
            self.refresh_preview_body();
        }
        if changed {
            self.publish_frame(FrameTrigger {
                occurred_at: Instant::now(),
                source: FrameSource::Expose,
            })?;
        }
        Ok(())
    }

    /// Settle the rows of every pane on screen whose stability window has run
    /// out, then hand whatever that produced to the engine.
    ///
    /// The walk is the whole of the fix: settling only the focused leaf left a
    /// visible sibling showing its own LaTeX until somebody clicked into it.
    /// `dispatch_tab_decoration_tasks` already visits every leaf of the tab, so
    /// once the settle does too there is nothing else to route.
    pub(in crate::runtime) fn advance_live_math_if_due(&mut self, now: Instant) -> Result<()> {
        let active = self.window.active_tab;
        let mut settled = false;
        for (_, leaf) in self.window.tabs[active].leaves_mut() {
            if leaf
                .session
                .live_stability_deadline()
                .is_some_and(|deadline| now >= deadline)
            {
                leaf.session.advance_live_stability(now);
                settled = true;
            }
        }
        if !settled {
            return Ok(());
        }
        let tasks = self.app.math_worker.tasks.clone();
        let scale_tasks = self.app.math_worker.scale_tasks.clone();
        let path_tasks = self.app.math_worker.path_tasks.clone();
        let window = self.window_id();
        let disabled = dispatch_tab_decoration_tasks(
            window,
            &mut self.window.tabs[active],
            &tasks,
            &scale_tasks,
            &path_tasks,
            &mut self.app.math_worker_running,
            &mut self.app.math_worker_notice_pending,
        );
        if disabled {
            self.publish_frame(FrameTrigger {
                occurred_at: now,
                source: FrameSource::Expose,
            })?;
        }
        Ok(())
    }

    /// **The band's travelling height and the picture's strength, written where
    /// the projection reads them** (review 2026-09-18, P1).
    ///
    /// Split out from [`Self::present_math_toggle`] so that the two questions
    /// are two functions: *what does this frame draw* is asked by every compose,
    /// and *is a frame worth asking for* is asked by the flight's own turn. They
    /// were one function, which is why a frame composed for any other reason
    /// projected whatever the last tick had cached.
    pub(crate) fn sample_math_toggle(&mut self, now: Instant) {
        let Some(flight) = self.window.math_toggle.as_ref() else {
            return;
        };
        let motion = self.app.motion;
        let target = flight.target();
        let presentation = bt_term::MathTogglePresentation {
            anchor: flight.anchor().clone(),
            height_subpixels: flight.height_subpixels(now, motion),
            picture_opacity_milli: flight.picture_opacity_milli(now, motion),
            face_milli: flight.face_milli(now, motion),
            source_width_cells: flight.source_width_cells(),
        };
        let Some(index) = self.live_paste_target(target) else {
            return;
        };
        if let Some(leaf) = self.window.tabs[index].sessions.get_mut(&target.seat) {
            leaf.session
                .set_math_toggle_presentation(Some(presentation));
        }
    }

    /// The terminal pane the pointer is inside, its seat-local position, and the
    /// frame it last drew.
    ///
    /// The whole of per-seat hit routing. Every pointer question below used to
    /// be asked of one frame in one rectangle, because there was one of each;
    /// with a fleet, "which cell is under the pointer" has no answer until you
    /// have said *which pane*, and the pointer's own coordinates are what say
    /// it. Deliberately not the focused leaf: hovering a link in the pane you
    /// are not typing in must underline that pane's link, and answering from the
    /// focused pane's cells would underline a cell the pointer is nowhere near.
    ///
    /// `None` when the pointer is over chrome, over a non-terminal pane, or over
    /// a pane that has not drawn yet — all three being "there is no cell here",
    /// which is exactly what the callers already do nothing about.
    pub(in crate::runtime) fn pane_hit_context(
        &self,
    ) -> Option<(bt_layout::SeatId, PhysicalPosition<f64>, &ViewportFrame)> {
        let position = self.window.pointer_position?;
        // **A float over the pointer is a float over the pane** (§7.39, user
        // report 2026-08-28). A press here would be taken by the window before it
        // reached a pane — `press_float` stands above the layout in the router —
        // and a resting pointer owes the same Z-order: a reference the window
        // covers is under the window, not under the pointer, and a card raised
        // for it would stand on top of the very window hiding it. This is the one
        // root every hover consumer draws from, so the gate is here and nowhere
        // else.
        if self.pointer_over_a_float(position) {
            return None;
        }
        let seat = seats::pane_at(&self.seat_layout, position.x, position.y)?;
        let leaf = self.sessions.get(&seat)?;
        let frame = leaf.last_presented_frame.as_ref()?;
        // The pane's *body*, not its seat: a pane with a head draws its grid
        // below that head, and a pointer measured from the seat's corner would
        // be off by the head's height on every pane that wears one.
        let scale = self.window.renderer.scale_factor() as f32;
        let body = seats::pane_body_viewport(&self.seats, &self.seat_layout, seat, scale)?;
        if position.x < f64::from(body.x)
            || position.y < f64::from(body.y)
            || position.x >= f64::from(body.x + body.width)
            || position.y >= f64::from(body.y + body.height)
        {
            return None;
        }
        Some((
            seat,
            PhysicalPosition::new(
                position.x - f64::from(body.x),
                position.y - f64::from(body.y),
            ),
            frame,
        ))
    }

    /// **The band under the pointer, cut to the pointer's own pane** (audit
    /// 2026-09-15, RC-2).
    ///
    /// The seat comes back from [`Self::pane_hit_context`] and is handed on
    /// rather than dropped: the renderer clamps a band's box against a
    /// `SeatViewport`, and the one it used to read off itself names the
    /// **focused** pane outside a compose. In an unfocused pane narrower than
    /// the focused one the two marks were hit-tested past that pane's own right
    /// edge — the press missed the mark you could see. The body and not the
    /// seat, for [`Self::pane_hit_context`]'s own reason: `position` has already
    /// been measured from the body's corner, and the boxes must be cut to the
    /// same rectangle they are measured in.
    /// **The seat comes back with the hit**, because a block anchor does not name a pane and two
    /// panes can hold anchors that compare equal — a `Live` anchor is a screen, two grid points and
    /// a generation counter, and every one of those is a per-session number. Asking "which session
    /// answers for this anchor" can therefore be answered by the wrong one; asking the pane the
    /// pointer was in cannot.
    pub(in crate::runtime) fn math_hit(&self) -> Option<(SeatId, MathHit)> {
        let (seat, position, frame) = self.pane_hit_context()?;
        let scale = self.window.renderer.scale_factor() as f32;
        let body = seats::pane_body_viewport(&self.seats, &self.seat_layout, seat, scale)?;
        let metrics = self.pane_frame_metrics(seat)?;
        self.window
            .renderer
            .math_hit_test(metrics, body, frame, position.x, position.y)
            .map(|hit| (seat, hit))
    }

    /// The hovered block belongs to the pane the pointer is in, and so does the state that lights
    /// it: `math_hit` already answers from that pane's frame, and the hover it sets is written to
    /// that pane's own shell. Writing it to the focused shell instead would darken a block in the
    /// pane holding the keyboard because the pointer was somewhere else entirely.
    ///
    /// **The band is held while the pointer is on it or on either mark, and let
    /// go the instant it is on neither** (owner's ruling 2026-09-18: 「为什么不能
    /// 和其它 hover 保持一致」). There is one question here and it is
    /// [`Self::math_hit`]'s, which is what makes that sentence structural rather
    /// than a pair of tests: since the ruling of 2026-09-15 ② (§7.1.5p ⑨ ii) the
    /// two marks stand *inside* the band's own rectangle, in the inset it
    /// reserves for them, so `math_hit`'s three answers — the `‹›`, the `⧉` and
    /// the block — are three regions of one box with no gap between them and
    /// nothing outside it. Reaching for a mark cannot drop the band, because the
    /// pointer never leaves the band to get there.
    ///
    /// That is what the half-second this used to arm was for. The mock-up's
    /// `transition-delay: .5s` forgave a pointer crossing from a band to marks
    /// that stood *beside* it, which was ③'s geometry and has not been this
    /// window's since ⑨ overturned it; what the delay bought after that was a
    /// highlight that stayed lit half a second after the hand had gone, which is
    /// what the owner reported, and no other hover in this window does it — a
    /// pane head's run, a tab's `×`, a link's underline and the tip all go on
    /// the pointer's own event.
    pub(in crate::runtime) fn update_math_hover(
        &mut self,
        now: Instant,
    ) -> Result<Option<MathHit>> {
        let Some(hit) = self.math_hit().map(|(_, hit)| hit) else {
            // On neither the band nor either mark: the hover is over, on this
            // event and not half a second after it. `leave_hovered_math` costs
            // one `Option` read for a window that was not hovering a formula,
            // which is what lets this door be unconditional.
            self.leave_hovered_math(now, MathHoverExit::PointerLeft)?;
            return Ok(None);
        };
        if self.window.math_hover_anchor.as_ref() != Some(&hit.anchor) {
            self.window.math_hover_anchor = Some(hit.anchor.clone());
            // **And no clock is set here** (owner's report 2026-09-14
            // evening). Until that report the marks' fade began on this
            // line, from the gesture — and a fade begun by the gesture is a
            // fade whose first frames are drawn from the picture that was in
            // hand *before* it, which is the stale-overlay half of the very
            // defect §7.1.5p ⑥ was written about. The marks arrive when the
            // picture that lights the band arrives, which is
            // [`Self::sync_math_tools`]'s answer and nobody else's. Crossing
            // straight from one formula to the next is a change of anchor,
            // so that door still sees a second arrival rather than a move.
            if self.set_hovered_math(Some(hit.anchor.clone())) {
                self.repaint_hovered_pane()?;
            }
        }
        Ok(Some(hit))
    }

    /// **Apply placement read from the frame this present draws to the marks**,
    /// and answer whether the overlay owes a rebuild.
    ///
    /// The one door between the two halves of this surface: the renderer says
    /// where a named band's boxes are *on this picture*
    /// ([`Self::math_tool_placement`]), the pointer says which mark it is on
    /// ([`Self::hovered_math_tool`]), and
    /// [`formula_tools::FormulaToolFollow::follow`] decides what that means for
    /// two marks already standing somewhere — nothing, a new target to travel
    /// to, or a different ink.
    ///
    /// **It is asked at each present while a band is lit or travelling, not only
    /// while a clock is running** (owner's reports 2026-09-14 and 2026-09-20).
    /// A press on `‹›` republishes the block taller, a resize re-wraps its rows,
    /// and a scale change resizes every box. The present door owns the frame for
    /// all of those changes, so it derives placement there and hands it in.
    ///
    /// **A band the pointer has left is never re-read from a picture.** The
    /// grace running out clears [`WindowRuntime::math_hover_anchor`] a whole
    /// picture before the shell stops lighting the block, so a frame in that gap
    /// still carries a lit placement; following it would turn the exit fade
    /// round and light the marks again over a band nobody is pointing at.
    fn sync_math_tools(
        &mut self,
        now: Instant,
        placement: Option<bt_render::MathToolBoxes>,
    ) -> Option<bool> {
        if self.window.math_tools.is_none() && self.window.math_hover_anchor.is_none() {
            return None;
        }
        let motion = self.app.motion;
        let mut changed = if self.window.math_hover_anchor.is_none() {
            self.window
                .math_tools
                .as_mut()
                .is_some_and(|follow| follow.leave(now, motion))
        } else if let Some(boxes) = placement {
            let hovered = self.hovered_math_tool();
            // **A band whose own height is travelling carries its marks** rather than having them
            // travel to it (§7.1.5p ⑪): two journeys over one distance would leave the marks
            // trailing the edge they ride and settling ninety milliseconds after it stopped.
            let riding = self
                .window
                .math_toggle
                .as_ref()
                .is_some_and(|flight| flight.anchor().same_block(&boxes.anchor));
            if let Some(follow) = self.window.math_tools.as_mut() {
                if follow.is_riding(&boxes.anchor, riding) {
                    follow.ride(&boxes, hovered, now, motion)
                } else {
                    follow.follow(&boxes, hovered, now, motion)
                }
            } else {
                self.window.math_tools = Some(formula_tools::FormulaToolFollow::arriving(
                    &boxes, hovered, now,
                ));
                true
            }
        } else {
            // This present's picture does not carry the named band. §7.1.5p
            // ⑥'s re-ruled answer is still silence rather than a neighbouring
            // block: naming the band and handing this frame are one lookup.
            false
        };
        if self.window.math_toggle.is_none()
            && let Some(follow) = self.window.math_tools.as_mut()
        {
            changed |= follow.finish_landing_frame();
        }
        // And a fade that has finished leaving is a surface that is gone: the
        // follow is dropped whole, so "no marks" is one fact and not a struct
        // holding a zero.
        if self
            .window
            .math_tools
            .as_ref()
            .is_some_and(|follow| follow.gone(now, motion))
        {
            self.window.math_tools = None;
            changed = true;
        }
        Some(changed)
    }

    /// **Which mark the pointer is on**, of the two the hovered band put up.
    ///
    /// [`Self::math_hit`] and never a second hit test: the box you can press and
    /// the box that lights up are one box by construction, which is the same
    /// arrangement [`Self::math_tool_placement`] keeps with the drawing. The two
    /// arms that are not verbs answer `None` — a pointer on the formula itself
    /// is not on a control.
    fn hovered_math_tool(&self) -> Option<formula_tools::FormulaTool> {
        match self.math_hit()?.1.target {
            MathHitTarget::ToggleSource => Some(formula_tools::FormulaTool::ToggleSource),
            MathHitTarget::CopyLatex => Some(formula_tools::FormulaTool::CopyLatex),
            MathHitTarget::Block | MathHitTarget::Failure => None,
        }
    }

    /// **Where the hovered band's marks stand on the frame this present draws**,
    /// in the surface's own pixels.
    ///
    /// **The band is named, and it is named by the same anchor that lit it**
    /// (owner's report 2026-09-14, T-MATH-TOOLS-SEAT).
    /// [`WindowRuntime::math_hover_anchor`] is the block the pointer resolved and
    /// the block [`Self::set_hovered_math`] wrote into the shell, so it is the
    /// block wearing the ground; handing it to the renderer is what makes the
    /// floor and the marks one answer.
    ///
    /// Naming alone was enough while marks appeared on hover and stood still.
    /// It stopped being enough once they travelled with a band whose height
    /// changes: the old implementation named the right block in the last
    /// presented frame while `redraw` drew the newly projected frame. The caller
    /// now hands this function the exact frame set it is about to present. A
    /// frame without the named block still answers `None`; it never substitutes
    /// whichever neighbouring block happens to be lit.
    ///
    /// **Still the shells and not the pointer's pane**: the marks outlive the
    /// pointer by the ninety milliseconds they leave over (owner's ruling
    /// 2026-09-18 took the half-second that used to come first), so a pointer
    /// that has left the *pane* leaves marks that are still fading, and marks
    /// that went looking for the pointer's own pane would have vanished on the
    /// frame the hand crossed the edge instead of fading where they stand. The
    /// pointer still decides which mark is *lit*, which is the only question it
    /// is the authority on.
    ///
    /// **The pane's own viewport goes in, and the same one moves the answer
    /// out** (audit 2026-09-15, RC-2). The renderer cuts a band's boxes to the
    /// `SeatViewport` it is handed; reading that off the renderer answered with
    /// the *focused* pane's width and height, so a band hovered in a narrower or
    /// shorter pane had its marks clamped against somebody else's rectangle and
    /// then translated by its own. One `body`, resolved before the boxes are
    /// asked for and spent on both, is what makes that pair impossible — and it
    /// is the same value [`Self::math_hit`] hands the hit test, so drawing and
    /// pressing cannot disagree either.
    ///
    /// A seat the solver is not showing as a pane is skipped rather than ending
    /// the search: a folded seat is not the pane the band is in, and the band's
    /// own pane may be the next one in the map.
    pub(in crate::runtime) fn math_tool_placement<'a>(
        &self,
        frame_for: impl Fn(
            SeatId,
        ) -> Option<(
            bt_render::SeatViewport,
            &'a ViewportFrame,
            bt_render::CellMetrics,
        )>,
    ) -> Option<bt_render::MathToolBoxes> {
        let hovered = self.window.math_hover_anchor.as_ref()?;
        let (body, mut boxes) = self.sessions.keys().find_map(|seat| {
            let (body, frame, metrics) = frame_for(*seat)?;
            Some((
                body,
                self.window
                    .renderer
                    .math_tool_boxes(metrics, body, frame, hovered)?,
            ))
        })?;
        let (dx, dy) = (body.x as f32, body.y as f32);
        for rect in [&mut boxes.block, &mut boxes.source, &mut boxes.copy] {
            *rect = [rect[0] + dx, rect[1] + dy, rect[2] + dx, rect[3] + dy];
        }
        Some(boxes)
    }

    /// **The frames the band's marks still owe the glass**, and none once both
    /// their journeys have landed.
    ///
    /// The arrival, the exit and a move to new geometry are one question here
    /// because they are one question in
    /// [`formula_tools::FormulaToolFollow::owes_frames`]: a journey owes frames
    /// exactly while it is travelling, and under `Motion::Reduced` no journey
    /// ever is. **No span is spelled here** — the ninety milliseconds belongs to
    /// [`tooltip::TOOLTIP_FADE`] and is read where the journey is, which is what
    /// keeps a second copy of it from appearing on the day it changes.
    ///
    /// A band standing still under a still pointer costs no wake-ups at all,
    /// which is the silence §7.29 promises every other hover in this window.
    pub(crate) fn math_tools_owe_frames(&self, now: Instant) -> bool {
        self.window
            .math_tools
            .as_ref()
            .is_some_and(|follow| follow.owes_frames(now, self.app.motion))
    }

    /// **The next frame a running journey is owed**, and nothing at all once
    /// both have landed.
    ///
    /// The tip's own arrangement ([`Self::tooltip_deadline`]): a surface in
    /// motion asks for the next frame rather than for the end of its span, so
    /// the ninety milliseconds is *drawn* rather than merely begun and finished.
    /// Until the owner's report of 2026-09-14 evening this asked for the end of
    /// the fade, which on a still window is two frames of a fade and no middle.
    pub(in crate::runtime) fn math_tools_deadline(&self, now: Instant) -> Option<Instant> {
        self.animating_deadline(self.math_tools_owe_frames(now), now)
    }

    /// **Draw the band's marks from the picture in hand**, and keep paying their
    /// journeys' frames until both land.
    ///
    /// The tip's own advancer, on the tip's own clock
    /// ([`Self::advance_tooltip_if_due`]) — and it is here for the reason that
    /// one is there: an overlay layer whose content is a function of a clock and
    /// of a picture that arrives on its own schedule is redrawn by something
    /// that keeps looking, or it is drawn once, from whatever was in hand, and
    /// left there. `refresh_overlay` answers whether the marks actually moved,
    /// so a span in which nothing changed costs a rebuild and no present.
    ///
    /// **Two reasons to pay and only one of them is a clock.** Geometry is read
    /// at the present door, where the frame being drawn is in hand. This turn
    /// still observes pointer ink and copy acknowledgement, which start no
    /// geometry clock of their own (owner's report 2026-09-14 evening).
    /// **And the copy tick comes down here too** (audit 2026-09-15, RB-1).
    ///
    /// [`WindowRuntime::math_copied`] had a writer and no reader that ever
    /// cleared it, so from [`FOOT_REVEAL_FEEDBACK`] after a copy the window's
    /// turn returned a deadline permanently in the past, `ControlFlow::WaitUntil`
    /// took it, and the loop span at one core for the life of the window — the
    /// exact failure `Runtime::about_to_wait`'s empty-registry branch names in
    /// its own comment. This is the reader: the acknowledgement is retired on the
    /// turn its window runs out, the overlay is rebuilt once so the tick becomes
    /// the pair of sheets again, and `math_copy_window` then answers `None` for
    /// it for ever after. **Both halves are needed**: filtering the deadline
    /// alone would leave the tick drawn until something else repainted the band.
    pub(in crate::runtime) fn advance_math_tools_if_due(&mut self, now: Instant) -> Result<()> {
        // **On the window's own display frame** (owner's report 2026-09-18).
        // Geometry is deliberately absent here: the present this turn requests
        // will derive it from the picture that present actually draws. See
        // [`Self::animation_frame_is_due`].
        if !self.animation_frame_is_due() {
            return Ok(());
        }
        let spent = hang_watch::during(hang_watch::Station::ClockMathCopy, || {
            retire_spent_math_copy(&mut self.window.math_copied, now)
        });
        // A window with no marks asks the picture nothing: the hit test is
        // behind the same refusal `sync_math_tools` makes.
        let hover_moved = if self.window.math_tools.is_some() {
            let hovered = self.hovered_math_tool();
            self.window
                .math_tools
                .as_mut()
                .is_some_and(|follow| follow.observe_hovered(hovered))
        } else {
            false
        };
        let owes_frame = self.math_tools_owe_frames(now);
        // While a formula journey is moving, the present door rebuilds this
        // lane from its handed frame. At rest, a hover-ink change or a spent copy
        // acknowledgement needs no geometry look and can rebuild immediately.
        if owes_frame || ((hover_moved || spent) && self.refresh_overlay()) {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// **Whether this band's copy is still inside its acknowledgement window.**
    ///
    /// Keyed on the anchor as well as the clock: a tick left standing on the
    /// next formula the pointer walked onto would be this window confirming
    /// something about a block nobody copied.
    ///
    /// The clock half is [`math_copy_window`]'s and not a second reading of the
    /// same instant (RB-1): the drawing, the retirement and the deadline are
    /// three consumers of one answer, and three copies of `at +
    /// FOOT_REVEAL_FEEDBACK` is how the field came to be drawn after it had
    /// stopped being owed.
    fn math_copy_is_fresh(&self, anchor: &MathBlockAnchor, now: Instant) -> bool {
        self.window
            .math_copied
            .as_ref()
            .is_some_and(|(said, at)| said == anchor && math_copy_window(Some(at), now).is_some())
    }

    /// **The band's two marks, wherever they have got to**, as one overlay
    /// layer.
    ///
    /// Nothing is decided here any more (owner's report 2026-09-14 evening).
    /// Where the marks stand, how solid they are and which of them is lit are
    /// three answers [`WindowRuntime::math_tools`] is already holding, because
    /// all three can be *between* two values on the frame this is asked for: a
    /// mark on its way to the geometry a toggle just published, a pair coming up
    /// or going out, an ink that changed when the pointer crossed onto one of
    /// them. This lane draws what that says and does no looking of its own —
    /// which is also what makes the whole of the ruling's motion clause testable
    /// without a GPU.
    ///
    /// The boxes it is handed are still the renderer's own, through exactly the
    /// arithmetic [`Self::math_hit`] is answered from ([`Self::math_tool_placement`]),
    /// so the box you can press and the box you can see are one box by
    /// construction rather than by two call sites agreeing.
    ///
    /// One layer or none: there is one pointer, so at most one band.
    pub(in crate::runtime) fn formula_tool_layers(&self, now: Instant) -> Vec<marks::OverlayLayer> {
        let Some(follow) = self.window.math_tools.as_ref() else {
            return Vec::new();
        };
        let motion = self.app.motion;
        let opacity = follow.opacity(now, motion);
        if opacity <= 0.0 {
            return Vec::new();
        }
        let boxes = follow.placed(now, motion);
        let scale = self.window.renderer.scale_factor() as f32;
        let state = formula_tools::FormulaToolState {
            hovered: follow.hovered(),
            pressed: self
                .window
                .math_tool_pressed
                .as_ref()
                .filter(|(anchor, _)| anchor == &boxes.anchor)
                .map(|(_, verb)| *verb),
            copied: self.math_copy_is_fresh(&boxes.anchor, now),
        };
        let sprites =
            formula_tools::sprites(&boxes, state, &bt_render::chrome_palette(), scale, opacity);
        if sprites.is_empty() {
            return Vec::new();
        }
        vec![marks::OverlayLayer {
            sprites,
            ..marks::OverlayLayer::default()
        }]
    }

    /// Sample diagnostics from the handed frame, before its ride receipt is consumed.
    pub(in crate::runtime) fn math_band_trace_for_present<'a>(
        &self,
        frame_for: impl Fn(
            SeatId,
        ) -> Option<(
            bt_render::SeatViewport,
            &'a ViewportFrame,
            bt_render::CellMetrics,
        )>,
    ) -> Option<(SeatId, bt_render::MathBandTrace, bool)> {
        if !self.app.trace_perf {
            return None;
        }
        let named = self.window.math_hover_anchor.as_ref()?;
        self.sessions.keys().find_map(|seat| {
            let (body, frame, metrics) = frame_for(*seat)?;
            let mut trace = self
                .window
                .renderer
                .math_band_trace(metrics, body, frame, named)?;
            let in_flight = self
                .window
                .math_toggle
                .as_ref()
                .is_some_and(|flight| flight.anchor().same_block(named));
            let riding = self
                .window
                .math_tools
                .as_ref()
                .map_or(in_flight, |follow| follow.is_riding(named, in_flight));
            for rect in [
                &mut trace.seat.block,
                &mut trace.seat.source,
                &mut trace.seat.copy,
            ] {
                rect[0] += body.x as f32;
                rect[2] += body.x as f32;
                rect[1] += body.y as f32;
                rect[3] += body.y as f32;
            }
            trace.ink_right += body.x as f32;
            Some((*seat, trace, riding))
        })
    }

    pub(in crate::runtime) fn math_band_trace_line(
        &self,
        now: Instant,
        trace: Option<(SeatId, bt_render::MathBandTrace, bool)>,
    ) -> String {
        let placed = self
            .window
            .math_tools
            .as_ref()
            .map(|follow| follow.placed(now, self.app.motion).source);
        let eye = |rect: Option<[f32; 4]>| {
            rect.map_or_else(
                || "none".to_owned(),
                |r| format!("{:.3},{:.3},{:.3},{:.3}", r[0], r[1], r[2], r[3]),
            )
        };
        let owes = self.math_tools_owe_frames(now)
            || self
                .window
                .math_toggle
                .as_ref()
                .is_some_and(|flight| flight.owes_frames(now, self.app.motion));
        let Some((seat, trace, riding)) = trace else {
            // The guard also covers departing marks. Say that the band is
            // absent instead of silently losing those very post-landing frames.
            return format!(
                "seat=none display=none band_l=none band_r=none ink_r=none height_sub=none seat_eye=none placed_eye={} face_opacity_milli=none riding=0 owes={}",
                eye(placed),
                u8::from(owes),
            );
        };
        let display = match trace.seat.display {
            bt_viewport::MathBlockDisplay::Rendered => "rendered",
            bt_viewport::MathBlockDisplay::Source => "source",
        };
        format!(
            "seat={} display={display} band_l={:.3} band_r={:.3} ink_r={:.3} height_sub={} seat_eye={} placed_eye={} face_opacity_milli={} riding={} owes={}",
            seat.0,
            trace.seat.block[0],
            trace.seat.block[2],
            trace.ink_right,
            trace.height_subpixels,
            eye(Some(trace.seat.source)),
            eye(placed),
            trace.picture_opacity_milli,
            u8::from(riding),
            u8::from(owes),
        )
    }

    pub(in crate::runtime) fn trace_math_band(&self, line: Option<String>) {
        if let Some(line) = line {
            trace_sink::stderr_line(format!(
                "BT_PERF_TRACE math_band frame={} {line}",
                self.window.renderer.perf_frame(),
            ));
        }
    }

    /// Whether a present can carry either formula overlay lane. These are the
    /// two `Option` reads paid by a present with no lit band and no flight; a
    /// flight begins from the marks and therefore keeps `math_tools` alive for
    /// its whole lifetime.
    pub(crate) fn formula_overlay_is_active(&self) -> bool {
        self.window.math_hover_anchor.is_some() || self.window.math_tools.is_some()
    }

    /// Put both formula overlay lanes on the frame this present is about to
    /// draw. `placement` and `toggle_layers` were derived from the same handed
    /// frame set immediately before this call, after every visible pane was
    /// projected. A stationary band still pays its one geometry scan, but no
    /// allocation or overlay rebuild when the answer did not move.
    pub(in crate::runtime) fn refresh_formula_overlay_for_present(
        &mut self,
        now: Instant,
        placement: Option<bt_render::MathToolBoxes>,
        toggle_layers: Vec<marks::OverlayLayer>,
    ) -> bool {
        let carried = std::mem::take(&mut self.window.formula_overlay_owed);
        let Some(moved) = self.sync_math_tools(now, placement) else {
            return false;
        };
        let in_flight = self.window.math_toggle.is_some();
        if !carried && !moved && !in_flight && !self.math_tools_owe_frames(now) {
            return false;
        }
        let formula_tools = toggle_layers
            .into_iter()
            .chain(self.formula_tool_layers(now))
            .collect();
        self.refresh_overlay_with_formula(now, formula_tools)
    }

    /// Set the hovered formula on the pane the pointer is in, and clear it everywhere else.
    ///
    /// One pointer, so at most one shell in the window may believe a block of its is under it. The
    /// sweep is what makes a pointer *leaving* a pane put that pane's block back, including when it
    /// left by crossing straight into another pane's block and no clear ever ran.
    fn set_hovered_math(&mut self, anchor: Option<MathBlockAnchor>) -> bool {
        let hovered = if anchor.is_some() {
            self.window.hover_pane
        } else {
            None
        };
        let active = self.window.active_tab;
        let mut changed = false;
        for (seat, leaf) in self.window.tabs[active].leaves_mut() {
            let wanted = anchor.as_ref().filter(|_| hovered == Some(*seat));
            changed |= leaf.session.set_math_hover(wanted);
        }
        changed
    }

    /// **Ask again what the pointer is on, because the picture moved under it**
    /// (owner's report 2026-09-18).
    ///
    /// A formula's hover is a fact about two things — where the pointer is, and
    /// what the pane is drawing there — and until this door existed only one of
    /// them was ever asked about. [`Self::update_math_hover`] had exactly one
    /// caller, the pointer-moved handler, so the window re-read the band under
    /// the hand when *the hand* moved and never when *the picture* did. A wheel
    /// notch (which on a trackpad produces no pointer event at all), a
    /// full-screen program repainting its own screen, a pane's own output
    /// scrolling, a resize, a block changing shape: every one of those takes the
    /// band out from under a resting pointer, and the band went on wearing its
    /// ground and its two marks until the pointer happened to twitch.
    ///
    /// **One rule and one door.** Nothing here decides anything: it re-asks the
    /// very question the pointer asks, through the very function the pointer
    /// goes through, so a band lit by a move and a band lit by a scroll cannot
    /// come to two different answers. A band that has gone out from under the
    /// pointer ends the hover exactly as a pointer leaving a band does — the
    /// same door, on the same terms, since the owner's ruling of 2026-09-18 made
    /// that door immediate for both. The marks do not
    /// flicker through any of it: [`Self::set_hovered_math`] answers `false`
    /// when the shells already believe what they are being told, and
    /// [`formula_tools::FormulaToolFollow`] keys on
    /// [`MathBlockAnchor::same_block`], so a band that merely stands on
    /// different rows this frame is followed rather than re-arrived at.
    ///
    /// **It is the presented picture's revision and not the composed one**, and
    /// that is the whole of what makes this honest: [`Self::math_hit`] reads
    /// `last_presented_frame`, which is written in the same breath as
    /// [`WindowRuntime::presented_picture_revision`], so the number compared
    /// here names the exact picture the hit test is about to be answered from.
    ///
    /// **A window with no pointer in it pays one `Option` read.** A window with
    /// one pays that plus a `u64` comparison on every turn, and the hit test
    /// only on the turns a new picture actually reached the glass — which is the
    /// same bargain [`Self::sync_math_tools`] already makes one clock over.
    pub(in crate::runtime) fn refresh_math_hover_against_the_picture(
        &mut self,
        now: Instant,
    ) -> Result<()> {
        if self.window.pointer_position.is_none() {
            // The hand is not in this window, so there is nothing to ask on its
            // behalf — and the pictures that went by while it was away are not a
            // debt to pay when it comes back: `pointer_moved` asks for itself on
            // the very first move. Squaring the number here is what keeps a
            // window nobody is pointing at from answering a stale comparison on
            // the turn the pointer returns.
            self.window.math_hover_revision = self.window.presented_picture_revision;
            return Ok(());
        }
        if self.window.math_hover_revision == self.window.presented_picture_revision {
            return Ok(());
        }
        self.window.math_hover_revision = self.window.presented_picture_revision;
        self.update_math_hover(now)?;
        Ok(())
    }

    /// **The band under the pointer stops being under the pointer** — the one
    /// door that ends a formula's hover, whatever ended it.
    ///
    /// The pointer's own door since the owner's ruling of 2026-09-18, and the
    /// session's since the audit of 2026-09-15 (RB-3) — because leaving the band
    /// was never the only way this fact can stop being true. A hover is a fact
    /// about **a session**: the anchor names a block in one shell's transcript,
    /// `math_tool_placement` looks for it in the active tab's leaves, and
    /// `sync_math_tools`' last arm deliberately *keeps* the marks where they are
    /// when no picture knows the band. Switch tabs by keyboard and all three
    /// hold: the anchor is a block in a transcript nobody can see, no frame in
    /// the new tab knows it, and the two marks therefore stood on the glass over
    /// the new tab's content until the pointer happened to move. Every door that
    /// takes that session off the screen comes through here.
    ///
    /// **The marks leave with the band, and they leave over the ninety
    /// milliseconds they arrived on** (owner's report 2026-09-14 evening):
    /// §7.1.5p ② spent the glance card's asymmetry here — a fade in and no fade
    /// out — and the owner asked for the pair, so the ground goes with the
    /// pane's next picture and the two marks fade where they stand. `math_tools`
    /// therefore outlives `math_hover_anchor` by exactly that span, and
    /// `sync_math_tools` — which reads the anchor, not this door — is what
    /// finally drops it.
    ///
    /// **And they begin leaving on the pointer's own event** (owner's ruling
    /// 2026-09-18: 「为什么不能和其它 hover 保持一致」). Until that ruling the
    /// pointer's door armed a half-second first — the mock-up's own
    /// `transition-delay: .5s`, which forgave a hand crossing from a band to
    /// marks that stood *beside* it. That was ③'s geometry and has not been this
    /// window's since ⑨ ii moved the marks inside the band's own rectangle, so
    /// what the delay still bought was a highlight left lit half a second after
    /// the hand had gone, which no other hover in this window does. See
    /// [`Self::update_math_hover`] for why the property the grace protected is
    /// now geometric.
    ///
    /// `why` is the one thing the doors do not agree on. A pointer moving off a
    /// band leaves the block exactly where it is, possibly in the middle of
    /// changing face; a tab switch, a pane close or a focus change takes the
    /// whole session off the screen, and a journey carried across one of those
    /// would go on presenting a height for a document nobody can see.
    pub(crate) fn leave_hovered_math(&mut self, now: Instant, why: MathHoverExit) -> Result<()> {
        // **A window with no band under the pointer leaves nothing**, and that
        // matters now that this door is on every pointer move that misses a
        // band as well as on every tab switch: everything below requests a
        // present, and a window that has never hovered a formula must not pay
        // for that.
        if self.window.math_hover_anchor.is_none()
            && self.window.math_tools.is_none()
            && self.window.math_tool_pressed.is_none()
            && self.window.math_toggle.is_none()
        {
            return Ok(());
        }
        // **And a block still changing face lands here — when the change is what
        // is leaving.** A tab switch, a pane close and a focus change (audit
        // 2026-09-15, RB-3) all take the band off the screen, and the end state
        // is what such a change is carried across as (§7.1.5p ⑪). A hand simply
        // moving off the band is not one of those: the block is still on the
        // glass, still travelling, and landing it early because nobody is
        // pointing at it would be this window cutting a motion short for the one
        // reader who looked away (owner's ruling 2026-09-18).
        if why == MathHoverExit::BandLeftTheScreen {
            self.settle_math_toggle()?;
        }
        self.window.math_hover_anchor = None;
        let motion = self.app.motion;
        if let Some(follow) = self.window.math_tools.as_mut() {
            follow.leave(now, motion);
        }
        self.window.math_tool_pressed = None;
        if self.set_hovered_math(None) {
            self.repaint_hovered_pane()?;
        }
        // And the first frame of that fade goes up in the same breath: the marks
        // are an overlay layer, so nothing is on the glass until the present
        // door rebuilds it, and under `Motion::Reduced` this frame is the whole exit.
        // `hide_tooltip`'s own idiom, for a surface that goes down the same way.
        self.present_chrome_change()?;
        Ok(())
    }

    /// **The pane of this tab whose session knows this block, and what its two faces measure**
    /// (owner's ruling 2026-09-15, T-MATH-TOGGLE-MOTION).
    ///
    /// The seat comes back with the answer for the reason RC-2 made a band's boxes take one
    /// (§7.1.5p ⑩ iii): a block belongs to a pane, and every step of a change — the height it is
    /// presented at, the document being told, the frame that carries it to the glass — has to be
    /// spent on **that** pane and not on whichever one happens to hold the keyboard when the clock
    /// next ticks. The press arrives on the focused pane, but a flight outlives the press.
    ///
    /// `None` for a block no session in this tab can measure between two faces: a live block (for
    /// which §7.1.5p ⑪ gives the reason), an inline run, a block whose record went away, and every
    /// block in a tab with no shell at all (§7.1.6h).
    /// **Asked of the pane the block is in, never of whichever pane answers first.** Two panes can
    /// hold anchors that compare equal — see [`Self::math_hit`] — so a search over the sessions is
    /// a coin toss between them, and all three of this window's block verbs take the seat for that
    /// reason rather than each deciding for itself.
    fn math_toggle_faces(
        &self,
        target: PasteTarget,
        anchor: &MathBlockAnchor,
    ) -> Option<bt_term::MathToggleFaces> {
        let index = self.live_paste_target(target)?;
        let leaf = self.window.tabs[index].sessions.get(&target.seat)?;
        leaf.session.math_toggle_faces(&leaf.projection, anchor)
    }

    /// **[`Self::math_toggle_faces`] without the other face's rows** — the pane
    /// and the two heights, and nothing laid out.
    ///
    /// The advancer's door, and the reason it is a second one: §7.1.5p ⑪ iv
    /// re-reads the far end on every turn, and a turn is not a frame. Building
    /// the `$$…$$` rows there to read one integer off them is the per-turn cost
    /// the owner's stutter report of 2026-09-15 names
    /// (`bt_term::DualPlaneSession::math_toggle_heights`). The rows belong to
    /// the press, which asks for them once.
    fn math_toggle_heights(
        &self,
        target: PasteTarget,
        anchor: &MathBlockAnchor,
    ) -> Option<[i64; 2]> {
        let index = self.live_paste_target(target)?;
        let leaf = self.window.tabs[index].sessions.get(&target.seat)?;
        leaf.session.math_toggle_heights(&leaf.projection, anchor)
    }

    /// **The `‹›` mark was pressed** — see [`formula_tools::FormulaToggleMotion`] for what the
    /// ninety milliseconds after it are made of.
    pub(in crate::runtime) fn press_math_toggle(
        &mut self,
        target: PasteTarget,
        anchor: &MathBlockAnchor,
    ) -> Result<()> {
        let Some(index) = self.live_paste_target(target) else {
            return Ok(());
        };
        let seat = target.seat;
        let now = Instant::now();
        let motion = self.app.motion;
        if let Some(mut flight) = self.window.math_toggle.take() {
            if flight.target() == target && flight.anchor().same_block(anchor) {
                // **A second press turns the journey round where it stands.** The document is not
                // touched and cannot need to be: the block has been one entry with an artifact
                // height for the whole of the flight, in *both* directions, so changing where it
                // is heading only moves which end the telling happens at.
                let Some(faces) = self.math_toggle_faces(target, anchor) else {
                    // The session stopped being able to measure this block between the two
                    // presses. Landing it is the only answer that leaves the picture and the
                    // document agreeing.
                    self.window.math_toggle = Some(flight);
                    return self.settle_math_toggle();
                };
                flight.reverse(faces.heights(), faces.source, now, motion);
                self.window.math_toggle = Some(flight);
                return self.present_math_toggle(target, now);
            }
            // A press on a **different** block lands the one in flight first: two bands are two
            // surfaces (§7.1.5p ⑦ iii's own reading of crossing from one to the next), and this
            // window tells one story about its document at a time.
            self.window.math_toggle = Some(flight);
            self.settle_math_toggle()?;
        }
        let faces = self.math_toggle_faces(target, anchor);
        // **Whether this press began a journey, said out loud** (review 2026-09-18, question ①).
        //
        // The instrument is the reason this had to be dug for. The owner's two recordings of
        // 2026-09-18 were read for evidence that the band's height travels, and the honest answer
        // turned out to be that no press in either of them turned a block over at all —
        // `band_moved` never rose once in 14,033 projections, and the count of rendered blocks
        // never changed across the clicked stretch — so the recordings said nothing about this
        // clause either way. A press that takes the one-frame road looks, in every other line of
        // the trace, exactly like a press that takes the journey and is then paced badly: both are
        // a burst of overlay presents over one composed frame. One line at the door tells them
        // apart. §7.10 ④‴'s lesson, on the gesture §7.1.5p ⑪ is about.
        if self.app.trace_perf {
            let plane = match anchor {
                MathBlockAnchor::History { .. } => "history",
                MathBlockAnchor::Live { .. } => "live",
            };
            let (road, from, to) = match (faces.as_ref(), motion) {
                (Some(faces), Motion::Full) => {
                    let [rendered, source] = faces.heights();
                    ("journey", rendered, source)
                }
                // The two roads that end in a one-frame switch, under their own names: a block the
                // session cannot measure two faces for (which is every live band today), and a
                // reader who asked this window for stillness.
                (Some(_), Motion::Reduced) => ("one_frame_reduced_motion", 0, 0),
                (None, _) => ("one_frame_unmeasurable", 0, 0),
            };
            trace_sink::stderr_line(format!(
                "BT_PERF_TRACE toggle plane={plane} road={road} rendered_subpixels={from} source_subpixels={to}"
            ));
        }
        let Some(faces) = faces else {
            // Not a history display band. It changes in the one frame it always did.
            return self.switch_math_source_now(target, anchor);
        };
        if motion == Motion::Reduced {
            // **Stillness lands the change on the frame it is asked for** and wakes the loop for
            // none of it — the answer every other surface in this window gives, given here by
            // never starting a journey rather than by a second reading of the setting.
            return self.switch_math_source_now(target, anchor);
        }
        let to_source = !faces.showing_source;
        if !to_source {
            // **A block going back to its picture is told so now.** The representation that can be
            // presented at any height is the artifact one, so this direction switches at the near
            // end and is then presented at the rows' own height on this very frame — which is the
            // same identity the other direction gets at the far end, read from the other side.
            let Some(leaf) = self.window.tabs[index].sessions.get_mut(&seat) else {
                return Ok(());
            };
            if !leaf.session.toggle_math_source(anchor) {
                return Ok(());
            }
        }
        self.clear_selection();
        self.window.math_toggle = Some(formula_tools::FormulaToggleMotion::begin(
            target,
            anchor.clone(),
            faces.heights(),
            to_source,
            faces.source,
            now,
        ));
        self.present_math_toggle(target, now)
    }

    /// **The change made in a single frame** — what pressing `‹›` was before this clause, and what
    /// it still is under [`Motion::Reduced`], on the live plane, and wherever the two faces cannot
    /// be measured.
    fn switch_math_source_now(
        &mut self,
        target: PasteTarget,
        anchor: &MathBlockAnchor,
    ) -> Result<()> {
        let Some(index) = self.live_paste_target(target) else {
            return Ok(());
        };
        let seat = target.seat;
        // `math_hit()` answered, so a shell drew the block that was clicked, and it drew it in this
        // seat's pane rather than in whichever pane holds the keyboard (§7.1.6h).
        let Some(leaf) = self.window.tabs[index].sessions.get_mut(&seat) else {
            return Ok(());
        };
        if leaf.session.toggle_math_source(anchor) {
            self.clear_selection();
            self.publish_interaction_frame()?;
        }
        Ok(())
    }

    /// **Put the flight's own frame on the glass**: the band at the height it has reached, the
    /// picture at the strength it has reached, and the overlay that carries the other face rebuilt
    /// against the same instant.
    ///
    /// One instant for both halves, for the reason [`Self::refresh_overlay`] takes one: the band
    /// the projection draws and the source text laid over it must not disagree about what time it
    /// is, and two `Instant::now()` calls in one frame can.
    fn present_math_toggle(&mut self, target: PasteTarget, now: Instant) -> Result<()> {
        let Some(index) = self.live_paste_target(target) else {
            return Ok(());
        };
        let seat = target.seat;
        let motion = self.app.motion;
        let presentation =
            self.window
                .math_toggle
                .as_ref()
                .map(|flight| bt_term::MathTogglePresentation {
                    anchor: flight.anchor().clone(),
                    height_subpixels: flight.height_subpixels(now, motion),
                    picture_opacity_milli: flight.picture_opacity_milli(now, motion),
                    face_milli: flight.face_milli(now, motion),
                    source_width_cells: flight.source_width_cells(),
                });
        let Some(leaf) = self.window.tabs[index].sessions.get_mut(&seat) else {
            return Ok(());
        };
        // **A turn that would draw the frame already on the glass asks for nothing.** `turn` runs
        // on every pass of the loop, which under a talkative shell is far more often than the
        // ninety milliseconds has frames in it; what decides whether this one is worth a picture is
        // whether the band would actually stand anywhere new — `Ease::retarget`'s own rule, asked
        // of the session rather than of the clock.
        if leaf.session.math_toggle_presentation() == presentation.as_ref() {
            return Ok(());
        }
        leaf.session.set_math_toggle_presentation(presentation);
        self.repaint_pane_change(seat)?;
        // The other face is an overlay layer. `redraw` rebuilds it only after it
        // has projected this presentation, from the same frame it then presents.
        Ok(())
    }

    /// **End the change now, wherever it had got to**: the far face on the glass, the document told
    /// if the telling was still owed, and the presentation put back.
    ///
    /// **The one door, and every ending comes through it** — the journey landing on its own clock,
    /// a press on another block, a wheel notch, a re-wrap that moved the height it was travelling
    /// to, and the band leaving the screen altogether ([`Self::leave_hovered_math`], which is how a
    /// tab switch, a pane close and a focus change reach here). A half-animated block is not
    /// something to carry into a view that has changed under it; the end state is.
    ///
    /// It refuses before it does anything at all when nothing is in flight, so a window nobody has
    /// pressed a formula in pays one `Option` read for every door it ever passes through.
    pub(in crate::runtime) fn settle_math_toggle(&mut self) -> Result<()> {
        let Some(flight) = self.window.math_toggle.take() else {
            return Ok(());
        };
        // The heights and not the faces: all this door wants is *which pane still answers for the
        // block*, and it is the last thing that runs before the change lands (T-MATH-MARKS-IN-
        // SOURCE-FACE).
        let target = flight.target();
        let Some(index) = self.live_paste_target(target) else {
            return Ok(());
        };
        let seat = target.seat;
        if self.math_toggle_heights(target, flight.anchor()).is_none() {
            // The owner still exists but its block vanished or changed. Release only that
            // owner's presentation; equal anchors in other panes do not belong to this flight.
            if let Some(leaf) = self.window.tabs[index].sessions.get_mut(&seat) {
                leaf.session.set_math_toggle_presentation(None);
            }
            return Ok(());
        }
        let switched = flight.switch_owed();
        if let Some(leaf) = self.window.tabs[index].sessions.get_mut(&seat) {
            leaf.session.set_math_toggle_presentation(None);
            if switched {
                leaf.session.toggle_math_source(flight.anchor());
            }
        }
        if switched {
            // The rows a selection was taken from are about to stop existing, which is why the
            // one-frame switch has always cleared it. The other direction cleared it at the press.
            self.clear_selection();
        }
        // Taking the flight must not hand changing geometry to a new ease.
        // The receipt is consumed by sync_math_tools on exactly this landing
        // frame, including interrupted journeys. It never books a wake-up.
        if let Some(follow) = self.window.math_tools.as_mut() {
            follow.finish_ride(flight.anchor());
        }
        self.repaint_pane_change(seat)?;
        Ok(())
    }

    /// **Pay the frames a change of face owes**, and land it on the frame it is due.
    ///
    /// The marks' own advancer beside it ([`Self::advance_math_tools_if_due`]) and on the same
    /// terms: a surface whose content is a function of a clock is redrawn by something that keeps
    /// looking, or it is drawn once and left there. What is different here is that the picture this
    /// one owes is a **terminal** frame and not an overlay — the band's height is part of the
    /// document's layout — so it republishes the pane rather than rebuilding chrome over it.
    ///
    /// **The endpoints are re-read on every turn.** A pane that re-wrapped, a font that changed
    /// size, a setting that changed the breathing a band keeps: any of them moves the height this
    /// journey is travelling *to*, and a journey that lands somewhere the block does not stand is
    /// the jump this clause exists to remove. Asking is one measurement of one block's own lines,
    /// and only while something is in flight.
    ///
    /// **And it asks for the heights, not for the faces** (owner's report 2026-09-15,
    /// T-MATH-MARKS-IN-SOURCE-FACE). A turn is not a frame: the loop passes here on every wake-up
    /// a talkative shell causes, and `math_toggle_faces` lays the block's `$$…$$` rows out — one
    /// `layout_frozen_line` per line, every cluster materialized, a `String` per row — to have
    /// `heights()` read one integer pair off it and drop the rest. [`Self::math_toggle_heights`]
    /// is that pair, counted rather than laid out. The rows are the press's business and the
    /// press asks for them once.
    pub(in crate::runtime) fn advance_math_toggle_if_due(&mut self, now: Instant) -> Result<()> {
        let motion = self.app.motion;
        let Some((target, anchor, landed)) = self.window.math_toggle.as_ref().map(|flight| {
            (
                flight.target(),
                flight.anchor().clone(),
                flight.landed(now, motion),
            )
        }) else {
            return Ok(());
        };
        let Some(heights) = self.math_toggle_heights(target, &anchor) else {
            return self.settle_math_toggle();
        };
        let still_measures = self
            .window
            .math_toggle
            .as_ref()
            .is_some_and(|flight| flight.still_measures(heights));
        // **The landing is not paced, and stands above the gate for that reason**
        // (review 2026-09-18, P1). What lands is owed to the *document* — the
        // block is told which face it wears, the presentation is put back, the
        // selection the switch invalidates is cleared — and none of that is a
        // picture anybody is waiting for the display to take. Behind the gate it
        // was a change of face that a neighbouring pane printing every five
        // milliseconds could postpone for as long as it kept printing, which is
        // a block frozen half way over for the rest of the flood.
        if landed || !still_measures {
            return self.settle_math_toggle();
        }
        // **And only the frame of its own is paced** (owner's report 2026-09-18:
        // 「两个图标的移动还是一顿一顿的」). The ninety milliseconds is sampled
        // from the clock wherever this lands, so fewer turns draw the same
        // journey in fewer steps rather than a slower one — and the burst of
        // near-identical steps the recording caught, thirteen to twenty of them
        // inside one journey, is what the eye was reading as a jerk. A refusal
        // loses nothing: a frame composed for any other reason carries this
        // flight through [`Self::carry_live_journeys`], and if none is, the
        // refusal books one. See [`Self::animation_frame_is_due`].
        if !self.animation_frame_is_due() {
            return Ok(());
        }
        self.present_math_toggle(target, now)
    }

    /// **The next frame a running change of face is owed**, and nothing at all once it has landed.
    ///
    /// The tip's own arrangement, through the marks': the next frame rather than the end of the
    /// span, so the ninety milliseconds is drawn rather than merely begun and finished. No span is
    /// spelled here either.
    pub(in crate::runtime) fn math_toggle_deadline(&self, now: Instant) -> Option<Instant> {
        let flight = self.window.math_toggle.as_ref()?;
        // **The landing, unpaced, and the next frame of the travel, paced — the
        // earlier of the two** (review 2026-09-18, P1). The travel is a picture
        // and waits for the glass; the landing is the document being told which
        // face the block wears and waits for nothing, so a window whose other
        // pane is printing hard still lands this change on the frame it is due
        // rather than when the printing stops.
        let landing = flight.lands_at();
        let travelling = self.animating_deadline(flight.owes_frames(now, self.app.motion), now);
        Some(travelling.map_or(landing, |frame| frame.min(landing)))
    }

    /// **The other face of a block that is changing, over the band it is changing in.**
    ///
    /// The source text as an overlay, on its own layer under the marks': the two are drawn in one
    /// lane because they belong to one block, and the marks stand *on* the source text exactly as
    /// they stand on the picture (§7.1.5p ⑨ ii).
    ///
    /// Empty whenever nothing is in flight, when the fade has not left the
    /// picture yet, and when the handed frame does not know the named band.
    /// This is §7.1.5p ⑥'s re-ruled answer: the band is named and the picture is
    /// this frame's, so it says nothing rather than drawing over a neighbour.
    pub(in crate::runtime) fn formula_toggle_layers<'a>(
        &self,
        now: Instant,
        frame_for: impl Fn(
            SeatId,
        ) -> Option<(
            bt_render::SeatViewport,
            &'a ViewportFrame,
            bt_render::CellMetrics,
        )>,
    ) -> Vec<marks::OverlayLayer> {
        let Some(flight) = self.window.math_toggle.as_ref() else {
            return Vec::new();
        };
        let opacity = flight.source_opacity(now, self.app.motion);
        if opacity <= 0.0 {
            return Vec::new();
        }
        let target = flight.target();
        if self.live_paste_target(target).is_none() {
            return Vec::new();
        }
        let Some((body, frame, metrics)) = frame_for(target.seat) else {
            return Vec::new();
        };
        let Some(mut face) =
            self.window
                .renderer
                .math_band_face(metrics, body, frame, flight.anchor())
        else {
            return Vec::new();
        };
        if face.display == bt_viewport::MathBlockDisplay::Source {
            // **The frame this present draws is already drawing these very
            // rows.** Laying the overlay over it would strike the same text
            // twice, in the same face, at the same place.
            return Vec::new();
        }
        let (dx, dy) = (body.x as f32, body.y as f32);
        face.block = [
            face.block[0] + dx,
            face.block[1] + dy,
            face.block[2] + dx,
            face.block[3] + dy,
        ];
        face.rows_top += dy;
        face.rows_left += dx;
        face.rows_right += dx;
        let labels = formula_tools::source_face_labels(
            &face,
            flight.source_rows(),
            bt_render::foreground_rgb(),
            // The source overlay is set in its pane's own face (ticket 37).
            metrics.font_size_px,
        );
        if labels.is_empty() {
            return Vec::new();
        }
        vec![marks::OverlayLayer {
            labels,
            opacity,
            ..marks::OverlayLayer::default()
        }]
    }

    /// **Asked of the pane the block is in.** This used to ask the *focused* one, and a right
    /// press does not move the keyboard — the focus move lives inside the left-only route — so
    /// copying from a formula in an unfocused pane asked a session where the anchor names nothing
    /// and copied nothing, or, where that session happened to hold a block of the same shape,
    /// copied the wrong formula. The seat comes from the press, like the other two verbs'.
    pub(in crate::runtime) fn copy_math_latex(
        &mut self,
        target: PasteTarget,
        anchor: &MathBlockAnchor,
    ) {
        // A block anchor names a place in a shell's transcript, so a tab with no
        // shell has no anchor anybody could have clicked and nothing to copy
        // (§7.1.6h) — the same `None` a stale anchor already answers with.
        let Some(index) = self.live_paste_target(target) else {
            return;
        };
        let Some(source) = self.window.tabs[index]
            .sessions
            .get(&target.seat)
            .and_then(|leaf| leaf.session.math_source(anchor))
        else {
            return;
        };
        let result = hang_watch::during(hang_watch::Station::ClipboardWrite, || {
            bt_platform::set_clipboard_text(source)
        })
        .map_err(|error| anyhow!(error))
        .context("copy original LaTeX source to clipboard");
        // **Only a copy that landed says it landed** (owner's ruling 2026-09-14
        // ②). The bool this helper already returned was being thrown away, and
        // a tick on a clipboard the window could not reach would be the one
        // acknowledgement in this product that confirms nothing.
        if recoverable_clipboard_write(result, "formula copy") {
            self.window.math_copied = Some((anchor.clone(), Instant::now()));
        }
    }

    pub(in crate::runtime) fn apply_math_context_menu_result(&mut self) {
        let Some(result) = self.window.math_context_menu.take_result() else {
            return;
        };
        let anchor = self.window.pending_math_context_anchor.take();
        self.window.mouse_route = None;
        match (result, anchor) {
            (Ok(true), Some((target, anchor))) => self.copy_math_latex(target, &anchor),
            (Ok(true), None) => {
                eprintln!("recoverable formula context-menu result had no pending anchor");
            }
            (Ok(false), _) => {}
            (Err(error), _) => {
                eprintln!("recoverable formula context-menu failure: {error}");
            }
        }
    }

    /// **Re-key every pane of every tab** (review row R5-5).
    ///
    /// A theme switch, a scheme swap, a font change and a language switch all
    /// have one required hook: move whatever they move, then call this. See
    /// [`window_layout_key`] for which of the four revisions answers for
    /// which. The session keeps same-source old pixels only while the
    /// replacement is pending.
    ///
    /// Every pane, because every one of those facts is about the *window* or the
    /// *display* and none of them is about the pane holding the keyboard.
    /// `apply_scale_factor` and `adopt_terminal_font` already say so in as many
    /// words, handing every leaf of every tab its new metrics; this is the
    /// sentence that tells each session its rasters were built for the old ones,
    /// and while it was said to the focused leaf alone a sibling went on drawing
    /// pictures measured for a cell that no longer exists.
    ///
    /// **The width is each pane's own.** Two panes of one tab are two widths, so
    /// there is no single key to compute once and hand round: the walk builds one
    /// per leaf out of that leaf's columns. A tab with no shell has no leaves and
    /// therefore no bands to re-key, which is the no-op §7.1.6h asks for.
    pub(in crate::runtime) fn sync_math_layout_key(&mut self) {
        let dpi_milli = self.window.renderer.dpi_milli();
        // The window's own count, not a constant. It was `1` for as long as
        // nothing could change the face; the Terminal font row can, and a frozen
        // revision here would leave every typeset band rastered for the previous
        // cell — correct glyphs at the wrong size, from a cache whose key did not
        // include the thing that moved.
        let font_rev = self.window.renderer.font_revision();
        let line_wrapping = self.app.settings_store.loaded().line_wrapping;
        for tab in self.window.tabs.iter_mut() {
            for (_, leaf) in tab.leaves_mut() {
                leaf.session.set_layout_key(window_layout_key(
                    nonzero_u32(leaf.grid.columns.get()),
                    dpi_milli,
                    // **The size is each pane's own too** (ticket 37): the face this leaf was
                    // last applied at, so a pane at 150 % keys its bands at its own em.
                    leaf.metrics.font_size_subpixels(),
                    font_rev,
                    line_wrapping,
                ));
            }
        }
    }

    /// Put one string on the clipboard, through the door every other copy in
    /// this window uses.
    pub(in crate::runtime) fn copy_text_to_clipboard(&mut self, text: &str) {
        let result = hang_watch::during(hang_watch::Station::ClipboardWrite, || {
            bt_platform::set_clipboard_text(text)
        })
        .map_err(|error| anyhow!(error))
        .context("copy a refused address to the clipboard");
        let _ = recoverable_clipboard_write(result, "web address copy");
    }
}
