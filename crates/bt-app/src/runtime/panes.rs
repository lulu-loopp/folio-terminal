//! `panes` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use super::pointer::{Ending, Gone, Leaving, OwnerGone};
use crate::runtime::attention::expire_leaf_attention;
use crate::runtime::floats::{file_menu_powers, float_dock_label};
use crate::runtime::frame::trace_unchanged_present;
use crate::runtime::preview::picture_channel_owner;
use crate::runtime::preview::preview_image_placement;
use crate::runtime::tabs::pane_can_become_a_tab;
use crate::runtime::windows::dirty_gate_names;
use crate::window_seam::prelude::*;
use crate::{
    BirthAt, BirthDue, BirthWait, EASE, HoverFloat, LeafResizeTarget, LeafSeed, LeafView,
    PaneBirth, PaneMotion, PlanInputs, SuccessorLanding, TextScale, born_in_tab, cardhint,
    conpty_source_of, cubic_bezier, decided_birth, deliver_held_input, diagnostics,
    exit_diagnostics, i18n, icons, land_shell_birth, pane_into_new_tab, resolved_birth_seed, seed,
    split_axis, tabs_a_close_loses, toast,
};
use crate::{
    CardAim, ColumnNotch, CommandFlash, DividerDrag, DividerGrip, Drag, DragCarry, DragHandover,
    DragSource, DropLanding, FilesFocusArrival, FlashBand, FolderPick, FormulaSwitches,
    FrameImageReferences, FrameTraces, HandoverInto, LeafId, LeafOnStage, LeafSession,
    MathHoverExit, MenuPaint, Motion, NewWindowPlan, NoticeHost, PaneDraw, PaneMenuState,
    PaneTransform, PeekCache, PeekCacheEntry, PopoverTrigger, Popup, PopupOwner, PresentIntent,
    PreviewSurface, RAIL_TEXT_FADE, RAIL_TEXT_FADE_OPEN_DELAY, RAIL_TRANSITION, RevealTween,
    RowHost, RowPayload, RowPayloadKind, RowVerb, Runtime, SplitSeed, TabId, TabState, TabSurface,
    TearOut, attention_trace, auto_split_axis, card_trace, closing_this_pane_closes_the_tab,
    cmdrail, create_leaf_session, files, float, focus_thumb, glyph_trace, hang_watch, marks,
    mouse_trace, notice, notify, palette, present_diagnostics, presentation_physical_size, preview,
    preview_trace, profiles, restore, restore_row_seed, row_verb, schedule_leaf_grid_change,
    scrollback_quota, seats, solve_seats, solve_tree, trace, video_seat, web_thumb, webhost,
    wheel_zoom_notches,
};
use anyhow::Context;
use anyhow::{Result, anyhow};
use bt_layout::{Axis, SeatId, SeatKind, SeatLayout, SeatMetrics, SizePolicy};
use bt_persist::LayoutNodeV1;
use bt_render::{
    ChromePalette, FrameSource, FrameTrigger, GridSize, PresentOutcome, PresentPhase, Travel,
};
use bt_term::normalized_local_image_path_key;
use bt_viewport::horizontal::ContentColumn;
use bt_viewport::{ViewSelection, ViewportFrame};
use bt_workbench::attention;
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Instant;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::MouseScrollDelta;
use winit::keyboard::ModifiersState;
use winit::window::WindowId;

impl<B: Backend> Runtime<'_, B> {
    /// **Land every birth of this window that is due** — one pass over both places a birth can
    /// sit (a pane's own, and a *Restart shell* successor's beside it) in both of its waits
    /// ([`births_due`]): T-PROGRAMS-REFRESH's rows and T-BIRTH-OFF-WINDOW's shell. Called when the
    /// program walk's answer reaches this window and at the head of every drain (the birth worker
    /// wakes the pane's window through the pane's own wake).
    ///
    /// A birth whose rows are now answered is decided exactly as [`create_leaf_session`] decides
    /// one, and made again through it at its text size and its seat's current rectangle, from its
    /// seed with the default resolved and its folder crossed ([`resolved_birth_seed`]); the new
    /// pane is itself in birth for its shell, and what was typed meanwhile moves to it. A birth
    /// whose shell has answered lands ([`land_shell_birth`]): finished from the answer, a resize it
    /// missed told to it, what was typed written first. A successor that lands takes its pane's
    /// place; one that fails is said as an error toast over the pane it would have replaced, which
    /// is left as it was. A pane's own failed birth is said over a pane with no shell.
    pub(crate) fn land_births(&mut self) -> Result<()> {
        let stored = self.app.settings_store.loaded().default_profile.clone();
        let mut landed = false;
        for tab_index in 0..self.window.tabs.len() {
            let birth_death_due: Vec<SeatId> = self.window.tabs[tab_index]
                .sessions
                .iter()
                .filter_map(|(seat, leaf)| {
                    leaf.birth_death_fallback
                        .as_ref()
                        .is_some_and(|pending| pending.landing.shell.answered())
                        .then_some(*seat)
                })
                .collect();
            for seat in birth_death_due {
                landed = true;
                self.land_birth_death_fallback(tab_index, seat)?;
            }
            let due: Vec<(SeatId, BirthAt, BirthDue)> = {
                let programs = &self.app.profile_programs;
                let rows_decide = |seed: &LeafSeed| decided_birth(seed, &stored, programs).is_ok();
                let locale_settled = crate::locale_declaration::settled();
                self.window.tabs[tab_index]
                    .sessions
                    .iter()
                    .flat_map(|(seat, leaf)| {
                        births_due(leaf, &rows_decide, locale_settled)
                            .into_iter()
                            .map(|(at, due)| (*seat, at, due))
                    })
                    .collect()
            };
            for (seat, at, due) in due {
                landed = true;
                match (at, due) {
                    (at, BirthDue::Rows) => self.land_pane_birth(tab_index, seat, &stored, at)?,
                    (BirthAt::Pane, BirthDue::Shell) => self.land_own_shell(tab_index, seat)?,
                    (BirthAt::Successor, BirthDue::Shell) => self.land_restart(tab_index, seat)?,
                }
            }
        }
        if landed {
            self.refresh_chrome();
            self.publish_frame(FrameTrigger {
                occurred_at: Instant::now(),
                source: FrameSource::Expose,
            })?;
        }
        Ok(())
    }

    /// Land the last-resort shell requested in place after a silent birth death, and say what
    /// became of the pane (`runtime::tabs::land_birth_death_fallback`).
    fn land_birth_death_fallback(&mut self, tab_index: usize, seat: SeatId) -> Result<()> {
        let Some(leaf) = self.window.tabs[tab_index].sessions.get_mut(&seat) else {
            return Ok(());
        };
        match crate::runtime::tabs::land_birth_death_fallback(leaf)? {
            crate::runtime::tabs::BirthDeathLanding::Waiting => {}
            crate::runtime::tabs::BirthDeathLanding::FellBack(exit) => {
                exit_diagnostics::say_shell_exits([&exit], diagnostics::note);
            }
            crate::runtime::tabs::BirthDeathLanding::Kept(exit, error) => {
                exit_diagnostics::say_shell_exits([&exit], diagnostics::note);
                exit_diagnostics::say_spawn_failure(
                    seat.0,
                    Some(Path::new(bt_pty::LAST_RESORT_SHELL)),
                    &error,
                    diagnostics::note,
                );
            }
        }
        Ok(())
    }

    /// A pane's own shell has answered: land it, record its source for the startup trace, and
    /// say a failed birth.
    fn land_own_shell(&mut self, tab_index: usize, seat: SeatId) -> Result<()> {
        let tab_id = self.window.tabs[tab_index].id;
        let Some(leaf) = self.window.tabs[tab_index].sessions.get_mut(&seat) else {
            return Ok(());
        };
        let refusal = land_shell_birth(leaf, diagnostics::note)?;
        let source = landed_source(leaf, refusal.is_some());
        if let (Some(shells), Some(source)) = (self.app.startup_shells.as_mut(), source) {
            shells.born(tab_id, seat, source);
            if let Some(lines) = shells.lines_if_born() {
                for line in lines {
                    crate::diagnostics::stderr_line(line);
                }
                self.app.startup_shells = None;
            }
        }
        if let Some(fallback) = leaf.spawn_fallback.take() {
            exit_diagnostics::say_spawn_fallback(seat.0, &fallback, diagnostics::note);
        } else if let Some(refusal) = refusal.as_ref() {
            exit_diagnostics::say_spawn_failure(
                seat.0,
                leaf.program.as_deref(),
                &format!("{refusal:#}"),
                diagnostics::note,
            );
        }
        if let Some(refusal) = refusal {
            self.say_shell_refusal(&refusal)?;
        }
        Ok(())
    }

    /// A *Restart shell* successor's shell has answered: it takes the pane's place, or its failure
    /// is said and the pane keeps the shell it has.
    fn land_restart(&mut self, tab_index: usize, seat: SeatId) -> Result<()> {
        let Some(leaf) = self.window.tabs[tab_index].sessions.get_mut(&seat) else {
            return Ok(());
        };
        let requested = leaf
            .successor
            .as_deref()
            .and_then(|successor: &LeafSession| successor.program.clone());
        match land_successor(leaf, diagnostics::note)? {
            SuccessorLanding::Waiting => Ok(()),
            SuccessorLanding::Refused(refusal) => {
                exit_diagnostics::say_spawn_failure(
                    seat.0,
                    requested.as_deref(),
                    &format!("{refusal:#}"),
                    diagnostics::note,
                );
                self.say_shell_refusal(&refusal)
            }
            SuccessorLanding::Landed(mut successor) if tab_index == self.window.active_tab => {
                if let Some(fallback) = successor.spawn_fallback.take() {
                    exit_diagnostics::say_spawn_fallback(seat.0, &fallback, diagnostics::note);
                }
                self.replace_restarted_shell(seat, *successor)
            }
            SuccessorLanding::Landed(mut successor) => {
                if let Some(fallback) = successor.spawn_fallback.take() {
                    exit_diagnostics::say_spawn_fallback(seat.0, &fallback, diagnostics::note);
                }
                self.window.tabs[tab_index]
                    .sessions
                    .insert(seat, *successor);
                Ok(())
            }
        }
    }

    /// A shell that could not be born, said where B2's refusals are said: an error toast on this
    /// window, its title naming what failed and its body the reason.
    fn say_shell_refusal(&mut self, refusal: &anyhow::Error) -> Result<()> {
        self.toast(
            toast::ToastKind::Error,
            toast::ToastAnchor::Window,
            Some(i18n::Text::ShellDidNotStart.text().to_owned()),
            format!("{refusal:#}"),
        )
    }

    fn land_pane_birth(
        &mut self,
        tab_index: usize,
        seat: SeatId,
        stored: &str,
        at: BirthAt,
    ) -> Result<()> {
        let tab_id = self.window.tabs[tab_index].id;
        let Some(leaf) = self.window.tabs[tab_index].sessions.get(&seat) else {
            return Ok(());
        };
        let Some(birth) = leaf.birth_at(at) else {
            return Ok(());
        };
        let held_seed = birth.seed.clone();
        let probe_input = birth.probe_input.clone();
        let text_scale = match at {
            BirthAt::Pane => leaf.text_scale,
            BirthAt::Successor => leaf
                .successor
                .as_deref()
                .map_or(leaf.text_scale, |successor: &LeafSession| {
                    successor.text_scale
                }),
        };
        let Ok((identity, _)) = decided_birth(&held_seed, stored, &self.app.profile_programs)
        else {
            return Ok(());
        };
        let (seed, refusal) = resolved_birth_seed(&held_seed, &identity);
        let render_physical =
            presentation_physical_size(self.window.renderer.presentation_geometry());
        let (layout, _, _, _) = solve_seats(
            &self.window.tabs[tab_index].seats,
            &self.window.renderer,
            render_physical,
            self.window.size_policy,
            self.rail_posture(),
            self.platform_chrome(),
        );
        let body = seats::birth_body_viewport(
            &self.window.tabs[tab_index].seats,
            &layout,
            seat,
            &seats::seat_metrics(self.window.renderer.dpi_milli().get()),
            self.window.renderer.scale_factor() as f32,
        );
        let view = LeafView::at(&mut self.app.gpu, &self.window.renderer, text_scale)?;
        let born = create_leaf_session(
            view,
            body,
            LeafId { tab: tab_id, seat },
            &self.window.pty_wake,
            probe_input.as_deref(),
            &seed,
            &self.app.profile_programs,
            stored,
            FormulaSwitches::from_settings(self.app.settings_store.loaded()),
            scrollback_quota(self.app.settings_store.loaded().scrollback_lines),
            self.app.settings_store.loaded().line_wrapping,
            &bt_pty::SystemShellEnvironment,
        )?;
        // What was typed meanwhile moves to the new pane — into its own birth's queue while its
        // shell is being made, or to its shell or nowhere when it has none to wait for — ahead of
        // anything typed after; the old queue goes only once that was taken.
        if let Some(birth) = self.window.tabs[tab_index]
            .sessions
            .get(&seat)
            .and_then(|leaf| leaf.birth_at(at))
        {
            deliver_held_input(birth, born.input_target(), diagnostics::note)?;
        }
        match at {
            BirthAt::Pane => {
                let source = conpty_source_of(Some(&born));
                self.window.tabs[tab_index].sessions.insert(seat, born);
                if let (Some(shells), Some(source)) = (self.app.startup_shells.as_mut(), source) {
                    shells.born(tab_id, seat, source);
                    if let Some(lines) = shells.lines_if_born() {
                        for line in lines {
                            crate::diagnostics::stderr_line(line);
                        }
                        self.app.startup_shells = None;
                    }
                }
            }
            // A successor made again stays beside the shell it replaces until its own shell
            // lands; one with no shell to wait for takes the pane's place now, as
            // `restart_shell` does with one.
            BirthAt::Successor if born.birth.is_some() => {
                if let Some(leaf) = self.window.tabs[tab_index].sessions.get_mut(&seat) {
                    leaf.successor = Some(Box::new(born));
                }
            }
            BirthAt::Successor => {
                if tab_index == self.window.active_tab {
                    self.replace_restarted_shell(seat, born)?;
                } else {
                    self.window.tabs[tab_index].sessions.insert(seat, born);
                }
            }
        }
        if let Some(refusal) = refusal {
            self.toast(
                toast::ToastKind::Error,
                toast::ToastAnchor::Window,
                None,
                refusal.notice(),
            )?;
        }
        Ok(())
    }

    /// Re-solve the tree against the current surface and place the terminal
    /// seat.
    ///
    /// It used to also *answer* a grid — the one `seats.identity()`'s rectangle
    /// asks for — and every caller handed that answer straight to the focused
    /// leaf, which is a different leaf as soon as a tab holds two. Deriving
    /// cols/rows is [`Self::resize_leaves_to_layout`]'s job now, once per leaf
    /// from that leaf's own body, so there is no single grid for this method to
    /// return and no way for a caller to give one shell another's width.
    ///
    /// The direction is still one-way (red line L10): what comes back out of the
    /// terminal never re-enters here.
    pub(in crate::runtime) fn resolve_seat_layout(&mut self, render_physical: PhysicalSize<u32>) {
        // Provenance is decided here, at the one place a rectangle becomes a layout, and from the
        // very number the solver is handed (user ruling 2026-08-08).
        (self.window.size_policy, self.window.lawful_client_size) = size_authority_for_rectangle(
            self.window.size_policy,
            self.window.lawful_client_size,
            render_physical,
        );
        let (layout, overflow, terminal_seat, viewport) = solve_seats(
            &self.seats,
            &self.window.renderer,
            render_physical,
            self.window.size_policy,
            // **The posture, never the stored preference** (§7.1.6b′). The card
            // column stands in the panel's own place and costs the stage its own
            // width exactly as an expanded rail does, and that width is
            // `RailState::terminal_inset_logical_px` reading `focus`. The bit
            // lives on the window rather than in `window.rail`, so a solve handed
            // the stored preference is a solve told the mode is off — which is
            // the whole of "the column floated over the panes".
            self.rail_posture(),
            self.platform_chrome(),
        );
        // **The one gate the type system cannot hold: the stage was solved against
        // the panel this window is actually wearing.**
        //
        // Every geometry question in this file is supposed to go through
        // [`Self::rail_posture`], and nothing but a reader's care makes that true —
        // `WindowRuntime::rail` is the same type and compiles just as well, which
        // is how the card column came to be drawn floating over the panes on the
        // day it landed. So the agreement is checked where the answer lands: it
        // re-solves the viewport from
        // the posture, and a solve handed anything else stops the debug build on
        // the first frame of the mode rather than showing it to a reader.
        debug_assert_eq!(
            viewport,
            seats::logical_viewport(
                render_physical.width,
                render_physical.height,
                seats::scale_ppm(self.window.renderer.dpi_milli().get()),
                seats::rail_inset_device_px(
                    self.rail_posture(),
                    seats::scale_ppm(self.window.renderer.dpi_milli().get()),
                ),
                seats::chrome_band_device_px(
                    seats::scale_ppm(self.window.renderer.dpi_milli().get()),
                    self.platform_chrome(),
                ),
            ),
            "the stage was laid out against a panel this window is not wearing"
        );
        self.window.seat_viewport = viewport;
        // T230, and the reason the diff is taken *here*: this is the one place a
        // solved layout becomes the layout, so it is the one place that can tell
        // a real change from a rebuild that landed on the same answer. Every
        // cause §3.5 lists — the ladder, a divider drag, a swap, focus mode —
        // passes through it, and none of them has to remember to say so.
        let events = seats::layout_events(&self.seat_layout, &layout);
        self.seat_layout = layout;
        self.seat_overflow = overflow;
        self.publish_layout_events(events);
        self.window.renderer.set_seat_viewport(terminal_seat);
        self.refresh_preview_for_layout();
        self.refresh_chrome();
    }

    /// Every pane's solved box in physical pixels, in the solver's own order.
    ///
    /// The order is `SeatLayout::rects`', which is the in-order walk of the tree
    /// (D2) — red line L8, and the reason [`PaneMotion`] stores a `Vec` at all:
    /// every number in here is geometry, and geometry that depends on iteration
    /// order is geometry nobody chose.
    ///
    /// A seat the solver did not place has no box and is simply absent, which is
    /// the same answer [`PaneMotion::begin`] gives a seat that left the layout.
    pub(in crate::runtime) fn pane_rects(&self) -> Vec<(SeatId, [f32; 4])> {
        self.seat_layout
            .rects
            .iter()
            .filter_map(|placement| {
                let device = placement.device_rect?;
                Some((
                    placement.id,
                    [
                        device.left as f32,
                        device.top as f32,
                        device.right as f32,
                        device.bottom as f32,
                    ],
                ))
            })
            .collect()
    }

    /// What each pane is drawn through right now, for [`seats::PaneMotionFrame`].
    ///
    /// The clock is read once by the caller and handed down, exactly as
    /// [`Self::tab_trailers`] does for the strip: two seats of one chrome build
    /// disagreeing about what time it is would be two seats sampled from two
    /// different frames of the same animation.
    pub(in crate::runtime) fn pane_transforms(&self, now: Instant) -> Vec<(SeatId, PaneTransform)> {
        self.seat_layout
            .rects
            .iter()
            .map(|placement| {
                (
                    placement.id,
                    self.window
                        .pane_motion
                        .transform_of(placement.id, now, self.app.motion),
                )
            })
            .collect()
    }

    /// Hand one commit's geometry changes to whoever is listening (T230).
    ///
    /// See [`WindowRuntime::last_layout_events`] for who that is, and is not, today.
    fn publish_layout_events(&mut self, events: Vec<seats::LayoutEvent>) {
        self.window.last_layout_events = events;
        if self.app.trace_layout_events && !self.window.last_layout_events.is_empty() {
            crate::diagnostics::stderr_line(format!(
                "BT_LAYOUT_EVENTS {:?}",
                self.window.last_layout_events
            ));
        }
    }

    pub(crate) fn seat_metrics(&self) -> SeatMetrics {
        seats::seat_metrics(self.window.renderer.dpi_milli().get())
    }

    /// One terminal pane's body, or `None` when the rail has nothing to stand on.
    ///
    /// The rail is drawn on the **primary screen only**, and this is where that is
    /// enforced rather than inside [`cmdrail`]: the ledger already refuses to
    /// record an alternate-screen marker (§3.2's isolated namespace), so a pane
    /// running `vim` has a ledger full of the commands that ran *before* `vim`
    /// started — perfectly true marks that would be drawn over somebody else's
    /// canvas. What the alternate screen suspends is not the data, it is the
    /// picture.
    pub(crate) fn command_rail_body(&self, seat: SeatId) -> Option<[f32; 4]> {
        let leaf = self.sessions.get(&seat)?;
        let scale = self.window.renderer.scale_factor() as f32;
        let body = seats::pane_body_viewport(&self.seats, &self.seat_layout, seat, scale)?;
        cmdrail::host_rect(
            [
                body.x as f32,
                body.y as f32,
                (body.x + body.width) as f32,
                (body.y + body.height) as f32,
            ],
            leaf.session.terminal_modes().alternate_screen,
        )
    }

    /// **The rail's ordinal stack for one pane — "one rail, two sources"**
    /// (§7.1.5d, S4).
    ///
    /// The plain command ledger, unless a capsule is open on this very pane with
    /// something typed in it; then the ledger and the matched lines merged into
    /// one stack in document order. The two sides are put in one line space by
    /// [`bt_viewport::search_address`] — the function the highlighter already uses
    /// to place a cell against a hit — so a command's prompt row and a hit on that
    /// same row are recognisably the same line rather than two ticks a pixel
    /// apart.
    ///
    /// **The hits are deduplicated per line here and not in [`cmdrail::merge`]**,
    /// because this is where the hits are: the search keeps them in document order
    /// grouped by line already, so the dedup is `last()` and not a set. B56's own
    /// asymmetry lands in these four lines — *the rail counts lines, the counter
    /// counts hits* — and it is what stops a `grep` output, where every line
    /// matches several times, from drawing a rail nobody can read.
    fn command_rail_stack(&self, seat: SeatId) -> cmdrail::Stack {
        let Some(leaf) = self.sessions.get(&seat) else {
            return cmdrail::Stack::default();
        };
        let marks = leaf.session.command_marks();
        if self.command_rail_search(seat).is_none() {
            return cmdrail::commands(marks);
        }
        let commands: Vec<cmdrail::CommandLine> = marks
            .iter()
            .map(|mark| cmdrail::CommandLine {
                mark: mark.id,
                line: leaf
                    .session
                    .command_mark_anchor(mark.start)
                    .and_then(bt_viewport::search_address)
                    .map(|(line, _)| line),
                failed: mark.failed(),
            })
            .collect();
        let current = self.window.search.current().map(|hit| hit.line);
        let mut matches: Vec<cmdrail::MatchLine> = Vec::new();
        for (index, hit) in self.window.search.hits().iter().enumerate() {
            if matches.last().is_some_and(|last| last.line == hit.line) {
                continue;
            }
            matches.push(cmdrail::MatchLine {
                line: hit.line,
                hit: index,
                current: current == Some(hit.line),
            });
        }
        cmdrail::merge(&commands, &matches)
    }

    /// What the search contributes to this pane's [`cmdrail::RailKey`], or `None`
    /// when the pane is carrying no search.
    ///
    /// **`None` for an empty field**, which is the one place this parts company
    /// with the prototype: `srchRail()` marks the rail the moment a capsule exists,
    /// so its commands recede at `Ctrl+F` before anything has been asked. An empty
    /// query has asked nothing, and a rail that greys out to answer nothing is a
    /// rail that has told the reader their history went away.
    fn command_rail_search(&self, seat: SeatId) -> Option<cmdrail::RailSearch> {
        if !rail_is_searching(self.window.search.seat(), self.window.search.query(), seat) {
            return None;
        }
        Some(cmdrail::RailSearch {
            query: self.window.search_revision,
            hits: self.window.search.revision(),
            current: self.window.search.current_index(),
        })
    }

    /// Fold every rail that is changing mode this frame — see
    /// [`cmdrail::RailCache::fold_for_mode`].
    fn fold_rails_on_mode_change(&mut self) {
        let modes: Vec<(SeatId, bool)> = self
            .window
            .command_rails
            .keys()
            .map(|seat| (*seat, self.command_rail_search(*seat).is_some()))
            .collect();
        for (seat, searching) in modes {
            if let Some(cache) = self.window.command_rails.get_mut(&seat) {
                cache.fold_for_mode(searching);
            }
        }
    }

    /// Every rail on screen, at whatever temperature its own clocks have reached,
    /// with the jump flash under them.
    ///
    /// The rails themselves come out of [`cmdrail::RailCache`] and are usually not
    /// rebuilt at all: a frame that moved neither a ledger nor a pane nor an
    /// unfolded bucket hands back the layer it handed back last time, which is the
    /// whole reason `command_marks_revision` exists. What the cache cannot hand
    /// back is a rail with a hand on it — `hot` recolours every tick and the crest
    /// travels — so [`cmdrail::RailCache::picture`] paints those afresh and caches
    /// only the resting one.
    pub(in crate::runtime) fn command_rail_layers(&mut self) -> Vec<marks::OverlayLayer> {
        let scale = self.window.renderer.scale_factor() as f32;
        let palette = bt_render::chrome_palette();
        let motion = self.app.motion;
        let now = Instant::now();
        let flash = self.command_flash_layer(&palette, scale);
        // Before anything is keyed: a rail that is changing mode folds whatever
        // the fisheye had open, because the stack it was open over is about to be
        // replaced. See [`cmdrail::RailCache::fold_for_mode`].
        self.fold_rails_on_mode_change();
        // Pass one, under a shared borrow: every rail that is drawn this frame,
        // and — for the ones whose key has moved — the geometry to draw it with.
        // See [`cmdrail::RailCache::needs_rebuild`] for why the question and the
        // answer are asked in two passes.
        let seats: Vec<SeatId> = self.sessions.keys().copied().collect();
        let plans: Vec<(SeatId, cmdrail::RailKey, Option<cmdrail::Rail>)> = seats
            .into_iter()
            .filter_map(|seat| {
                let body = self.command_rail_body(seat)?;
                let leaf = self.sessions.get(&seat)?;
                let key = cmdrail::RailKey {
                    revision: leaf.session.command_marks_revision(),
                    body,
                    scale,
                    // The pointer's, and the one thing about the pointer a rail's
                    // *geometry* is a function of: a fisheye genuinely moves ticks.
                    expanded: self
                        .window
                        .command_rails
                        .get(&seat)
                        .and_then(|cache| cache.pointer().expanded()),
                    search: self.command_rail_search(seat),
                };
                let stale = self
                    .window
                    .command_rails
                    .get(&seat)
                    .is_none_or(|cache| cache.needs_rebuild(key));
                Some((
                    seat,
                    key,
                    stale.then(|| {
                        cmdrail::lay_out(body, &self.command_rail_stack(seat), scale, key.expanded)
                    }),
                ))
            })
            .collect();
        // Pass two, exclusive: store what changed and hand out the pictures.
        let mut layers: Vec<marks::OverlayLayer> = flash.into_iter().collect();
        for (seat, key, fresh) in plans {
            let cache = self.window.command_rails.entry(seat).or_default();
            if let Some(rail) = fresh {
                cache.install(key, rail, &palette);
            }
            // An empty ledger draws nothing and reports nothing — `cmd.exe` and a
            // PowerShell without the integration script live here (inventory C13).
            if cache.rail().ticks.is_empty() {
                continue;
            }
            layers.push(cache.picture(&palette, now, motion));
        }
        layers
    }

    /// Drop the rails of seats that are no longer terminals of the tab on screen.
    ///
    /// A cache keyed by seat outlives the seat unless something says otherwise,
    /// and a torn-out pane leaves one behind that would be handed back to whatever
    /// seat id the solver reuses next. The sweep runs where the overlay is built,
    /// so it is a function of the same solve everything else that frame is.
    pub(in crate::runtime) fn sweep_command_rails(&mut self) {
        if self.window.command_rails.is_empty() {
            return;
        }
        let live: BTreeSet<SeatId> = self.sessions.keys().copied().collect();
        self.window
            .command_rails
            .retain(|seat, _| live.contains(seat));
        if self
            .window
            .command_rail_hover
            .is_some_and(|(seat, _)| !live.contains(&seat))
        {
            self.window.command_rail_hover = None;
        }
    }

    pub(in crate::runtime) fn drive_command_rail_hover(
        &mut self,
        position: Option<PhysicalPosition<f64>>,
    ) -> Result<bool> {
        let now = Instant::now();
        let motion = self.app.motion;
        let palette = bt_render::chrome_palette();
        // Pass one, shared: the fisheye and the crest, settled together against
        // the ledger — see [`cmdrail::resolve`].
        let settled = position.and_then(|position| self.settle_command_rail(position));
        let hover = settled
            .as_ref()
            .and_then(|(seat, _, resolved)| resolved.nearest.map(|index| (*seat, index)));
        let mut changed = self.window.command_rail_hover != hover;
        self.window.command_rail_hover = hover;
        // Pass two, exclusive. Every rail the pointer is not on cools and folds
        // whatever it had open — the mock-up's `railReset`, which does exactly
        // these two things in this order.
        let cold: Vec<SeatId> = self
            .window
            .command_rails
            .keys()
            .copied()
            .filter(|seat| hover.map(|(hot, _)| hot) != Some(*seat))
            .collect();
        for seat in cold {
            let Some(cache) = self.window.command_rails.get_mut(&seat) else {
                continue;
            };
            let ticks = cache.rail().ticks.len();
            // Folding is expressed as a key change and paid for by the next
            // [`Self::command_rail_layers`], rather than laid out here: a rail
            // nobody is pointing at can afford to fold on the frame it is drawn.
            changed |= cache.pointer_mut().expand(None);
            cache.pointer_mut().aim(ticks, None, false, now, motion);
        }
        if let Some((seat, key, resolved)) = settled {
            let cache = self.window.command_rails.entry(seat).or_default();
            if cache.needs_rebuild(key) {
                cache.install(key, resolved.rail, &palette);
            }
            let ticks = cache.rail().ticks.len();
            changed |= cache.pointer_mut().expand(resolved.expanded);
            cache
                .pointer_mut()
                .aim(ticks, resolved.nearest, true, now, motion);
        }
        if changed {
            // The finger, and the crest, on the same reading — see
            // [`pointer_cursor`]'s `over_command_tick`.
            self.apply_pointer_cursor();
            // **The chrome and not just the overlay**, because the crest moving
            // changes *what can be tipped* and the tippable list is a product of
            // the chrome build ([`Self::rebuild_tooltip_anchors`], "what the
            // strip draws is what can be tipped, and both are decided here or
            // neither is"). Refreshing only the overlay repainted the rail and
            // left the anchor list as the last chrome build had it — with no
            // `CommandTick` entry in it at all — so `tooltip_layer` settled its
            // 380ms, went looking for the anchor it had been promised, found
            // nothing, and drew no card. Measured on a real window: hovering a
            // tick lit the rail and never produced a glance card unless some
            // unrelated event happened to rebuild the chrome underneath it.
            if self.refresh_chrome() {
                self.present_chrome_change()?;
            }
        }
        Ok(hover.is_some())
    }

    /// Which rail the pointer is on, what its geometry should be, and which tick
    /// is the crest — all three under a shared borrow, so the answer can be stored
    /// under an exclusive one.
    ///
    /// The band is tested **before** the fisheye is asked anything, and it is
    /// tested at the temperature the rail is currently drawn at (see
    /// [`cmdrail::Rail::hot_bounds`] for why that asymmetry is safe). Asking the
    /// other way round would let a pointer that is not on the rail at all open a
    /// bucket on it.
    fn settle_command_rail(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<(SeatId, cmdrail::RailKey, cmdrail::Resolved)> {
        let seat = seats::pane_at(&self.seat_layout, position.x, position.y)?;
        let body = self.command_rail_body(seat)?;
        let cache = self.window.command_rails.get(&seat)?;
        cmdrail::nearest(
            cache.rail(),
            self.window
                .command_rail_hover
                .is_some_and(|(hot, _)| hot == seat),
            position.x as f32,
            position.y as f32,
        )?;
        let leaf = self.sessions.get(&seat)?;
        let scale = self.window.renderer.scale_factor() as f32;
        let resolved = cmdrail::resolve(
            body,
            &self.command_rail_stack(seat),
            scale,
            cache.pointer().expanded(),
            position.y as f32,
        );
        let key = cmdrail::RailKey {
            revision: leaf.session.command_marks_revision(),
            body,
            scale,
            expanded: resolved.expanded,
            search: self.command_rail_search(seat),
        };
        Some((seat, key, resolved))
    }

    /// Which rail and which tick a point means, in the geometry that is on the
    /// glass.
    ///
    /// Asked of the cache rather than laid out afresh: the band the pointer is
    /// tested against must be the band that was drawn, and one derivation is the
    /// only way to say that. A seat whose rail has not been built this session has
    /// no band and answers nothing.
    pub(in crate::runtime) fn command_rail_at(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<(SeatId, usize)> {
        let seat = seats::pane_at(&self.seat_layout, position.x, position.y)?;
        self.command_rail_body(seat)?;
        let cache = self.window.command_rails.get(&seat)?;
        cmdrail::nearest(
            cache.rail(),
            self.window
                .command_rail_hover
                .is_some_and(|(hot, _)| hot == seat),
            position.x as f32,
            position.y as f32,
        )
        .map(|index| (seat, index))
    }

    /// While any rail still owes a frame to one of its four clocks, one frame at
    /// the animation's own rate; nothing at all otherwise.
    pub(in crate::runtime) fn command_rail_deadline(&self, now: Instant) -> Option<Instant> {
        self.animating_deadline(self.command_rails_are_moving(now), now)
    }

    pub(crate) fn command_rails_are_moving(&self, now: Instant) -> bool {
        let motion = self.app.motion;
        self.window
            .command_rails
            .values()
            .any(|cache| cache.pointer().is_animating(now, motion))
    }

    /// Pay the rails' frames. The clocks run themselves out — nothing here has to
    /// stop them, because `is_animating` and the paint read the same instants.
    pub(in crate::runtime) fn advance_command_rails(&mut self, now: Instant) -> Result<()> {
        if !self.command_rails_are_moving(now) {
            return Ok(());
        }
        // On the window's own display frame — see [`Self::animation_frame_is_due`].
        if !self.animation_frame_is_due() {
            return Ok(());
        }
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// A press on a rail — **and the one place the rail's two sources part
    /// company** (B40-B41).
    ///
    /// *"Clicking a match tick SELECTS that match (current advances there, count
    /// follows); a plain command tick keeps its normal jump."* Both verbs are
    /// older than this fork: the jump has been here since S1 and
    /// [`search::SearchState::set_current`] since S3, which is why the takeover
    /// costs one `match` and not a second press path. Which of the two a tick
    /// means was decided when the tick was built — see [`cmdrail::Entry::target`]
    /// — so nothing here asks the search whether it is open.
    pub(in crate::runtime) fn press_command_rail(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let Some((seat, index)) = self.command_rail_at(position) else {
            return Ok(false);
        };
        let Some(target) = self
            .window
            .command_rails
            .get(&seat)
            .and_then(|cache| cache.rail().ticks.get(index))
            .map(|tick| tick.target)
        else {
            return Ok(false);
        };
        match target {
            cmdrail::Target::Command(mark) => {
                let landing = self.jump_to_command_mark(seat, mark)?;
                // **The rail's own station** (`BT_MOUSE_TRACE`). Everything about
                // a press on a tick used to be silent: which tick was hit, which
                // mark it names, and — the question a report of "it jumped to the
                // wrong place" turns on — where the jump could actually land.
                // `window_top == extent` is a jump answered at the ceiling: the
                // mark is inside the last paneful and there is no document below
                // it to scroll, which is not the same sentence as "the rail
                // missed". `relief` is how much of that ceiling the blank rows
                // under the prompt are spending on a formula standing taller than
                // the pane, and it is what moves the ceiling between one press and
                // the next.
                self.mouse_trace(|| {
                    let number = |value: Option<i64>| {
                        value.map_or_else(|| "unresolved".to_owned(), |value| value.to_string())
                    };
                    match landing {
                        Some(landing) => format!(
                            "rail-jump seat={seat:?} tick={index} mark={mark:?} anchor_y={} local_offset={} window_top={} extent={} relief={}",
                            number(landing.anchor_y_subpixels),
                            landing.local_offset_subpixels,
                            number(landing.window_top_subpixels),
                            landing.extent_subpixels,
                            landing.relief_subpixels,
                        ),
                        None => {
                            format!(
                                "rail-jump seat={seat:?} tick={index} mark={mark:?} leave=nothing-to-jump-to"
                            )
                        }
                    }
                });
            }
            cmdrail::Target::Match(hit) => self.select_search_hit(hit)?,
        }
        Ok(true)
    }

    /// **Light the whole pane instead of a row in it** (DESIGN.md §7.55 ⑨).
    ///
    /// The other half of [`Self::jump_to_command_mark`], and deliberately
    /// missing that method's whole first half: **nothing is scrolled**. The two
    /// cases this is drawn for are a command that is still running and a pane
    /// showing an alternate screen, and in both of them a scroll is the wrong
    /// verb — it would take a reader who asked "where is that running" away from
    /// the very output they were being sent to watch, and on an alternate screen
    /// there is no scrollback to move at all.
    ///
    /// The overlay is rebuilt **here**, which is the opposite of the row band's
    /// arrangement one method up and for that arrangement's own reason: the ring
    /// is placed out of the seat layout, which the frame on the glass already
    /// agrees with, so there is no later frame to wait for. The row band waits
    /// because its rectangle is read out of a frame that has not been composed
    /// yet.
    pub(in crate::runtime) fn flash_pane(&mut self, seat: SeatId) -> Result<()> {
        self.window.command_flash = Some(CommandFlash {
            seat,
            band: FlashBand::Pane,
            started: Instant::now(),
        });
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Whether a seat can hold a capsule at all: a terminal, on its primary screen.
    ///
    /// **The alternate screen is the whole of the second half** (D-5). §3.2 keeps the two screens
    /// in isolated anchor namespaces with no ordering between them, so a search over the primary
    /// history could not place its hits against what `vim` is drawing; and a capsule that stayed up
    /// over a full-screen program would sit there reading `0/0` for as long as the program ran,
    /// which is R3's named failure. `Ctrl+F` is not in the table there either, so the key reaches
    /// the program and means whatever the program says it means.
    ///
    /// **A page is the second host** (§7.7 ②, W2 slice ④). The mock-up's own
    /// selector has read `.term, .pv-web-doc` since W1, and the ruling is that
    /// this is a second host and not a second implementation: the same capsule,
    /// in the same corner, opened with the same chord. What differs is who
    /// counts the matches — see [`Self::refresh_search`].
    pub(in crate::runtime) fn seat_can_search(&self, seat: SeatId) -> bool {
        if self.seat_holds_a_page(seat) {
            return true;
        }
        self.sessions
            .get(&seat)
            .is_some_and(|leaf| !leaf.session.terminal_modes().alternate_screen)
    }

    /// Whether the tab in front holds a page on that seat.
    pub(in crate::runtime) fn seat_holds_a_page(&self, seat: SeatId) -> bool {
        self.web_on(seat).is_some()
    }

    /// Decide which panes owe a strip, and give the answer to the tree.
    ///
    /// **The one place the projection is written.** The facts live on the leaves
    /// — which PowerShell this is, what its own `$PROFILE` says, whether it has
    /// spoken, whether a marker has arrived — and `Seats` holds the answer only
    /// because the pane's body is measured from it. Recomputed whole rather than
    /// edited, so a pane that has gone cannot leave a seat behind in the set.
    ///
    /// A change here is a **layout** change: a strip takes a row off the
    /// terminal, so the seats have to be solved again and the shells told what
    /// they now have. That is why this ends in `commit_seat_geometry` and why
    /// nothing else has to remember to.
    pub(crate) fn settle_pane_notices(&mut self) -> Result<()> {
        let presence = std::collections::BTreeSet::new();
        let mut states: std::collections::BTreeMap<NoticeHost, notice::Notice> =
            std::collections::BTreeMap::new();
        // **And the previews, on exactly the same terms** (user ruling
        // 2026-08-29). A document whose file moved under unsaved edits, and one
        // whose file is gone, each owe their reader a sentence — and the band
        // that carries it is the one already here rather than a second one of
        // its own, because a reader who meets both must not meet two heights and
        // two ideas of where the `×` is.
        //
        // Recomputed whole beside the shells' answer and not merged into it: the
        // facts come from a *pool* rather than from `sessions`, and a preview
        // seat can never be a terminal seat, so the two passes cannot disagree
        // about one seat.
        //
        // **No `presence` entry since 2026-09-12** (owner's ruling). A preview
        // pane's news is a pill floating over the bottom edge of its document
        // (`seats::news_pill_box`) and takes no row from the layout at all, so
        // the set that makes a body one bar shorter is the shells' alone. That
        // is the whole of "no reserved foot" in this pass: nothing here can make
        // a preview seat wear a band, so `Seats::seat_wears_notice` is false for
        // every one of them and `preview_body_viewport` subtracts nothing.
        for seat in self.seats.preview_seats() {
            let Some(state) = self.preview_disk_notice_on(self.preview_here(seat)) else {
                continue;
            };
            states.insert(NoticeHost::Seat(seat), state);
        }
        // **And the windows torn off them, on exactly the same terms** (B1,
        // 2026-09-01, §7.39's 总则).
        //
        // The news already arrived here — `refresh_preview_file` walks the
        // *pool*, and a floated document reads its tab's pool through
        // `PreviewSurface::Float` like any other surface — so what was missing
        // was never the fact. It was the row: the two loops above are the tree's
        // two kinds of leaf, a float is in neither, and a window whose file had
        // been replaced under unsaved edits went on showing the old body with
        // nothing said about it.
        //
        // No `presence` entry, and that is not an omission: `Seats::set_notices`
        // is what makes a *pane's* body one bar shorter, and a float's body is
        // derived from its frame every frame by `float_head_tools` →
        // `float_geometry`, which asks this same question directly. One answer,
        // two ways of spending it — the tree is told, and the window simply reads.
        for id in self.preview_float_ids() {
            let Some(state) = self.preview_disk_notice_on(PreviewSurface::Float(id)) else {
                continue;
            };
            states.insert(NoticeHost::Float(id), state);
        }
        // **Two changes, not one, and they gate different work.** Whether a seat
        // *wears* a strip decides the pane's height, so a change to that set is a layout change
        // and re-solves. A content-only change repaints the overlay and moves no rectangle.
        let geometry_moved = self.seats.set_notices(presence);
        let content_changed = self.window.notice_states != states;
        self.window.notice_states = states;
        if geometry_moved {
            self.commit_seat_geometry()?;
            self.refresh_chrome();
            return self.present_chrome_change();
        }
        if content_changed && self.refresh_overlay() {
            return self.present_chrome_change();
        }
        Ok(())
    }

    /// Take one pane's preview-file strip down without deciding anything else.
    pub(in crate::runtime) fn close_pane_notice(&mut self, host: NoticeHost) -> Result<()> {
        // **A preview's `×` is `Keep my edits`** and its own verb is the same
        // door (user ruling 2026-08-29): both of them mean "I have read this",
        // and the edits were never in danger from anything but a press on the
        // other word. A deleted file's strip has no other word, so this is the
        // whole of what can be said back to it.
        //
        // **And a torn-off window's `×` is the same sentence** (B1,
        // 2026-09-01): `notice_surface` is what makes the two hosts spend one
        // verb on one buffer.
        let surface = self.notice_surface(host);
        if !self
            .preview_buffer_on_mut(surface)
            .is_some_and(preview::PreviewBuffer::keep_this_body)
        {
            return Ok(());
        }
        self.settle_pane_notices()
    }

    /// **Whether the "Reopen your other tabs?" card is up**, which is also whether it holds
    /// the keyboard (0.4.5 ticket 57): [`restore::RestorePrompt::is_asking`], read of this
    /// window's prompt and the application's one question. [`Self::restore_layout`] draws the card
    /// on the same reading.
    ///
    /// **Never while a quit has fixed the document** (0.4.6 U-21): an update's quit keeps the
    /// windows up while its session lands, and a card answered then would open windows into a
    /// document that has already been photographed. The question is not spent — it is folded
    /// into the document that lands, as any unanswered question is — so the card comes back only
    /// if the quit is abandoned.
    pub(crate) fn restore_card_is_up(&self) -> bool {
        self.window
            .restore_prompt
            .is_asking(self.app.restore_question.len())
            && !self
                .app
                .quit
                .as_ref()
                .is_some_and(crate::quit::Quit::document_is_frozen)
    }

    /// The restore prompt's placement, or `None` when it is not asking.
    ///
    /// Every string it draws has to be measured with the real font before the
    /// box that holds them can be sized, which is why the content is built here,
    /// where the renderer is, and handed to a module that knows only numbers.
    pub(in crate::runtime) fn restore_layout(&mut self) -> Option<restore::RestoreLayout> {
        if !self.restore_card_is_up() {
            return None;
        }
        let scale = self.window.renderer.scale_factor() as f32;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (width, height) = (width as f32, height as f32);
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        // **The application's list, not this window's** (multiwindow slice D,
        // ruling ①). The card names every tab the launch is asking about — this
        // window's, the other open windows', and the windows that have not opened
        // at all — because there is one question. Which window each row goes back
        // to is not read here; it is where the row already lives.
        let rows = self
            .app
            .restore_question
            .iter()
            .map(|tab| {
                let seed = restore_row_seed(tab);
                let mut row =
                    restore::RestoreRow::from_seed(&seed, persisted_pane_count(&tab.root));
                row.label_text_width = measure(&row.label, restore::ROW_FONT_LOGICAL_PX * scale);
                row.cwd_text_width = measure(&row.cwd, restore::ROW_CWD_FONT_LOGICAL_PX * scale);
                row.badge_text_width = row.badge_text().map_or(0.0, |text| {
                    measure(&text, bt_render::WINDOW_TAB_BADGE_FONT_LOGICAL_PX * scale)
                });
                row
            })
            .collect();
        let content = restore::RestoreContent {
            rows,
            sub_lines: restore::wrap(
                restore::sub_text(),
                restore::content_width(width, scale),
                |text| measure(text, restore::SUB_FONT_LOGICAL_PX * scale),
            ),
            decline_text_width: measure(
                restore::decline_text(),
                restore::BUTTON_FONT_LOGICAL_PX * scale,
            ),
            restore_text_width: measure(
                restore::restore_text(),
                restore::BUTTON_FONT_LOGICAL_PX * scale,
            ),
        };
        Some(restore::layout(&content, width, height, scale))
    }

    /// The rail's own level of the stack — [`rail_overlay_layer`] with this
    /// window's two answers filled in.
    ///
    /// The fold is sampled *here* rather than inside the lift because it is a
    /// reading of a clock, and a pure function that asks the time is a function
    /// nothing can pin. Reading it once at the layer rather than baking it into
    /// each sprite is also what keeps the fade from compounding with the label
    /// fade the rail already runs (Q183): they are two declarations on two
    /// elements, and CSS multiplies them exactly once each.
    pub(in crate::runtime) fn rail_overlay_layers(&self) -> marks::Band {
        // **The fold belongs to the ordinary rail, and to nothing else**
        // (§7.1.6b′: the `Sidebar` row's three rest states govern the *ordinary*
        // panel, and the card column is card-width whatever they say). A window
        // whose sidebar was folded away and then entered focus mode kept the
        // fold's `opacity: 0` on the panel layer: the column was solved at its
        // full width, the stage started after it, the hit test answered its
        // cards — and nothing was drawn there. A card-wide hole with live
        // buttons in it,
        // which is worse than either of the two states it was between.
        //
        // `width_logical_px` already refuses the fold for the same reason; this
        // is that refusal on the paint side, which is where it was missing.
        let fold = self
            .window
            .rail_fold
            .sample(Instant::now(), self.app.motion)
            .0;
        rail_overlay_layer(
            &self.window.rail_chrome,
            panel_opacity(self.rail_posture(), fold),
        )
    }

    /// **The pane whose picture the stand-in wears** (缺陷 #189) — the local
    /// half of [`Runtime::strip_stand_in`], as the leaf rather than as a face.
    ///
    /// The stand-in was a *blank* card until this existed, and on a card column
    /// that is very nearly no card at all: the body is `--termbg` on a panel of
    /// almost the same value, so what the reader saw where their pane was about
    /// to land was an accent hairline around nothing. The head has said the
    /// right name since K124 and it was never enough — the whole of a card in
    /// this mode is its picture, and every other card in the column was showing
    /// one.
    ///
    /// **The picture is the one the column already has** (user ruling
    /// 2026-08-29): the seat's own projection, out of the very cache the pane's
    /// home card is drawn from, so the stand-in and the stage cannot be two
    /// readings of one shell. Nothing is projected a second time — see
    /// [`Self::refresh_focus_thumbnails`] for the one thing that *is* arranged,
    /// which is that the home tab goes on being projected while its card is
    /// scrolled out from under the hand.
    ///
    /// `None` for a visitor's stand-in and that is the honest answer, not a gap:
    /// the pane belongs to another window's process, this window has never drawn
    /// it, and what travels on the broker is a label ([`GhostFace`]) and a tree.
    pub(crate) fn stand_in_pane(&self) -> Option<LeafId> {
        // The same door, read in the same order [`Runtime::strip_stand_in`]
        // reads it: a visitor's slot is not this window's pane's.
        if self.window.foreign.is_some() {
            return None;
        }
        let drag = self.held_drag()?;
        let DropLanding::StripExtract { .. } = drag.landing? else {
            return None;
        };
        let DragSource::Pane(leaf) = drag.source else {
            return None;
        };
        // Through the pane's **own** tab, by id, for `strip_stand_in`'s reason:
        // the spring may have moved the stage, and the seat this card is a
        // picture of is a fact about the tree it is still filed under.
        self.tab_state(leaf.tab)?
            .seats
            .tree()
            .find_seat(leaf.seat)?;
        Some(leaf)
    }

    /// **The pane this window has in its hand, wherever the hand has gone** (B2,
    /// 2026-09-01).
    ///
    /// [`Self::stand_in_pane`] answers about a stand-in *this* window is
    /// drawing, so it goes quiet the moment the pointer crosses onto another
    /// window's glass — which is exactly when the picture is wanted most. This
    /// is the other half: the payload the broker says this window is holding, as
    /// long as it is holding it and whatever is under the pointer.
    ///
    /// Read off the broker rather than off the drag, because the broker is the
    /// one register of "a gesture is in flight and this window owns it" that
    /// survives the pointer leaving — the same reason `strip_guests` reads
    /// `strip_stand_in` instead of re-deriving it.
    pub(crate) fn carried_pane(&self) -> Option<LeafId> {
        let broker = self.app.drag_broker.as_ref()?;
        if broker.source != self.window_id() {
            return None;
        }
        let leaf = broker.cargo.pane()?;
        // Still in the tree it is filed under: a gesture whose seat was reaped
        // mid-flight carries nothing, which is `stand_in_pane`'s own last line.
        self.tab_state(leaf.tab)?
            .seats
            .tree()
            .find_seat(leaf.seat)?;
        Some(leaf)
    }

    /// **P177 — the veil over every pane that is still arriving**, on the panes
    /// this tab is actually drawing.
    ///
    /// The clock is the one [`Runtime::refresh_overlay`] read for the whole
    /// build, and the rectangles are the live solve rather than anything the
    /// tween remembers: an arriving pane's box is whatever the solver says it is
    /// this frame, including after a window resize that happened mid-fade.
    pub(in crate::runtime) fn pane_fade_veils(&self, now: Instant) -> Vec<marks::OverlayLayer> {
        pane_fade_veil_layers(
            &self.window.pane_motion,
            &self.pane_rects(),
            now,
            self.app.motion,
            bt_render::chrome_palette(),
        )
    }

    /// **What kind of pane a landing is aimed at**, or `None` for the rim and
    /// the strip's three, which are aimed at no pane at all.
    ///
    /// One sentence rather than the same three-line chain at each of the places
    /// that asks it, and the places are the ones the 2026-09-16 ruling made ask:
    /// the caption on the box, the commit behind it, and the plan that decides
    /// whether the box is drawn at all. A row's centre now means one of three
    /// things depending on the pane under it, so "which pane is under it" had
    /// better be one reading.
    fn aimed_seat_kind(&self, landing: DropLanding) -> Option<bt_layout::SeatKind> {
        landing
            .aimed_at()
            .and_then(|seat| self.seats.tree().find_seat(seat))
            .map(|seat| seat.kind)
    }

    /// The dock drawing's layers: the destinations, then the box over them.
    ///
    /// Under every menu and every tip, because the mock-up puts them there
    /// (`#dock-shift` 24 and `#dock-preview` 25 against `.combo-menu`'s 30 and
    /// `.tip`'s 60) — this is a drawing *on* the layout, not a surface floating
    /// over the window.
    pub(in crate::runtime) fn dock_overlay_layers(&self, now: Instant) -> marks::Band {
        let Some(shown) = self.window.drop_preview.as_ref() else {
            return marks::Band::default();
        };
        let reveal = shown.reveal.sample(now, self.app.motion).0;
        if reveal <= 0.0 {
            return marks::Band::default();
        }
        let overlay = seats::dock_overlay(
            &shown.plan,
            &self.seat_layout,
            self.layout_host_rect(),
            shown.inputs.landing.aimed_at(),
            // A refused box carries no word: what it says is said by being
            // dashed and empty, and "Swap panes" printed inside an outline that
            // means "this will not happen" is the box arguing with itself.
            if shown.plan.fits() {
                shown.inputs.landing.caption(
                    &shown.inputs.source,
                    self.aimed_seat_kind(shown.inputs.landing),
                )
            } else {
                ""
            },
            self.window.renderer.scale_factor() as f32,
        );
        let Some(overlay) = overlay else {
            return marks::Band::default();
        };
        // **One surface, faded as one** (ticket 46): the box, its ring and its
        // caption arrive together rather than the caption ahead of the
        // translucent box it names — and the fade is the surface's, wrapped
        // round the layers as they were built rather than written over each
        // layer's own opacity (the audit's F6).
        marks::Band::surface(
            seats::build_dock_overlay(
                &overlay,
                self.window.renderer.scale_factor() as f32,
                bt_render::chrome_palette(),
            ),
            reveal,
            [0.0, 0.0],
        )
    }

    /// **Whether one of the popups now up is one the sidebar grew** —
    /// §7.1.6e″'s rule ①, as the question the rail's zone asks.
    ///
    /// A popup owned by the rail is part of the rail, so the pointer being in it
    /// is the pointer being in the rail; and because at most one popup is ever
    /// up (E61) the rail stays out for exactly as long as that popup does. That
    /// covers the corridor between the panel and the menu without a grace of its
    /// own — the hand crossing pixels that belong to neither is crossing them
    /// while the menu is still up, which is the same answer at both ends.
    fn rail_grew_a_popup(&self) -> bool {
        let up = self.popups_up();
        let tabs = self.tab_surface_now();
        Popup::ALL.into_iter().any(|popup| {
            up.holds(popup) && popup_owner(popup, tabs) == PopupOwner::Tabs(TabSurface::Rail)
        })
    }

    /// **Put away every popup the sidebar grew** — §7.1.6e″'s rule ③.
    ///
    /// [`Self::close_popup`]'s own loop, filtered by owner: what leaves with the
    /// panel is what the panel raised, and nothing else. Nothing is repainted
    /// here, for [`Self::close_popups_except`]'s reason — the one caller is
    /// mid-way through a posture change that repaints at its end.
    fn close_rail_popups(&mut self) {
        let tabs = self.tab_surface_now();
        for popup in Popup::ALL {
            if popup_owner(popup, tabs) == PopupOwner::Tabs(TabSurface::Rail) {
                self.close_popup(popup);
            }
        }
    }

    /// Turn the Git page on or off for the whole product (user ruling,
    /// 2026-08-15).
    ///
    /// **Off throws away what was read and stops the reading**, in that order.
    /// Dropping every column's [`git::GitCache`] is the honest half of the
    /// promise: a switch that stopped asking but kept a repository's file list in
    /// memory would still be holding what the user just said they did not want it
    /// to hold. Nothing is *asked* again either, because the gate that starts a
    /// question is in `files_tree_walk` and reads this same setting.
    ///
    /// **What is not touched is each column's own page** (see
    /// [`seats::FilesLeafState::view`]). A column that was on Git falls back to
    /// its tree while the switch is off and returns to Git when it is on again —
    /// the switch decides reachability, the column remembers the choice, and
    /// forcing every column back to Files here would silently spend a decision
    /// the user made about something else.
    /// **Which way a direction-less split cuts**, written down (user ruling,
    /// 2026-08-16).
    ///
    /// The shortest `apply_*` in this dialog, and the reason is worth stating:
    /// nothing on screen depends on it. Every other switch here changes what is
    /// drawn or what is running, so its verb has a second half; this one changes
    /// what the *next* split does, and there is no next split until somebody asks
    /// for one. `Runtime::settings_split_axis` reads the store at the moment of
    /// the split, so there is no copy of this value anywhere to keep in step.
    pub(crate) fn apply_split_direction(
        &mut self,
        direction: bt_persist::SplitDirectionV1,
    ) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.split_direction = direction;
        Ok(self.app.settings_store.store(settings))
    }

    /// **Everything that follows a change to the *set* of seats in a tab**, in
    /// the one place every verb that changes it now goes through.
    ///
    /// A function rather than five copies of four lines, because the four lines
    /// have now been forgotten twice in two different verbs and the symptom is
    /// the same both times and legible from nowhere: the tree changes, the
    /// layout re-solves, the panes are *drawn* in their new rectangles — and the
    /// shells beside the arriving pane are never told they have fewer columns,
    /// so the next prompt reprints itself into a width that no longer exists and
    /// loses its middle. `dock_float` was the second (it re-solved the layout and
    /// stopped there); the audit that found it read every call that mutates the
    /// tree, and this is where they all end now.
    ///
    /// The order is the one [`Self::toggle_files_pane`] established:
    ///
    /// 1. **the pointer's picture first**, because every rectangle it named has
    ///    moved and a `pane_hover` naming a seat that is gone keeps a `×` lit on
    ///    a pane that no longer exists;
    /// 2. **the drag**, because a divider in flight was steering a split that
    ///    may not be in the tree any more;
    /// 3. **the window's own minimum**, which is a floor derived from the seats
    ///    and so moves with them;
    /// 4. **the re-solve**, which is the step that actually carries the new
    ///    columns to every shell — and marks the session dirty on its way out,
    ///    which is why nothing here does that a second time.
    ///
    /// What is deliberately *not* here is anything about a particular kind of
    /// pane: a closing preview drops its image, a closing files column drops its
    /// root, a split moves the keyboard. Those belong to their verbs. This is
    /// only the part that is true whenever the count of seats changes at all.
    ///
    /// The preview sweep **is** true whenever the count changes, which is why it
    /// is here and not in a verb: a leaf can stop being a preview surface by any
    /// of a dozen edits (closed, replaced, torn into a tab, restored over), and a
    /// view left behind is a buffer this tab still believes is on screen — which
    /// is precisely what the pool's dirty gates read.
    pub(crate) fn settle_seat_set_change(&mut self) -> Result<()> {
        self.window.seat_pointer = seats::ChromePointer::default();
        self.drop_divider_drag();
        self.sweep_preview_panes();
        self.apply_window_min_inner_size()?;
        self.commit_seat_geometry()
    }

    /// `closePane` (mock-up 3558-3578, I102/I103/I105).
    ///
    /// One verb for every kind of leaf: the leaf leaves the tree, its sibling is
    /// promoted, and the run it left is re-balanced — all of which `close_seat`
    /// already does, because G79/G80 rule that leaving is balanced exactly as
    /// joining is.
    ///
    /// The branch that is new here is the last one. `detachLeaf` returns null
    /// when the tree holds a single pane, and the mock-up's answer to that is not
    /// "refuse" but `closeTab(w.id)` — **an empty tab is not a state that
    /// exists** (T226/§2.1), so closing the last pane *is* closing the tab. That
    /// falls through to `close_tab`, which keeps its own rule about the last tab
    /// in the strip: the window does not empty either. The question of *when*
    /// that branch is taken is [`closing_this_pane_closes_the_tab`], and it is
    /// not only about the pane count.
    ///
    /// **One confirmation, and only one pane earns it** (I103's second half, now
    /// that there are unsaved buffers for it to be about). A terminal pane's `×`
    /// kills its shell outright; a preview pane's asks — and only when it is the
    /// *last* preview pane in the tab, because the pool outlives any one pane and
    /// only the last one's closing would strand it (P123).
    pub(in crate::runtime) fn close_pane(&mut self, seat: bt_layout::SeatId) -> Result<()> {
        let kind = self
            .seat_layout
            .get(seat)
            .map(|placement| placement.kind)
            .unwrap_or(bt_layout::SeatKind::Terminal);
        // **Gate ① (P123).** Asked before the pane-count branch below, so the
        // question is put once however the close resolves — a preview pane that
        // is also the tab's last pane would otherwise fall through to `close_tab`
        // and be asked by gate ②, which is the same question with a different
        // subject.
        if !self
            .raise_dirty_gate(restore::GateRequest::ClosePane(seat))?
            .proceeds()
        {
            return Ok(());
        }
        if closing_this_pane_closes_the_tab(self.seats.pane_count()) {
            return self.close_tab(self.window.active_tab);
        }
        // **And the gesture held on it** (T-POINTER-CAPTURE R-9, owner gone): a press on its
        // head, a thumb, a selection, a press handed to its program — owed nothing now, and let
        // go of before the pane is.
        self.end_capture(OwnerGone(Gone::Pane(self.leaf_here(seat))));
        // **And the hovered formula goes with the pane** (audit 2026-09-15,
        // RB-3). Placed here on purpose: both gates have passed, so the close is
        // certain, and nothing has moved yet — the sweep can still reach the
        // session that is about to be removed, and the frame this publishes is
        // struck against a layout that is still the one on the glass.
        self.leave_hovered_math(Instant::now(), MathHoverExit::BandLeftTheScreen)?;
        let metrics = self.seat_metrics();
        if !self.seats.close_seat(&metrics, seat) {
            return Ok(());
        }
        // A preview seat holds an image the way a terminal holds a session, and
        // the pane going away is the one taking it — **that** pane's, not every
        // preview's: closing one leaf beside a pinned one must leave the pinned
        // one showing what it was showing.
        if kind == bt_layout::SeatKind::Preview {
            let surface = self.preview_here(seat);
            self.clear_preview_image(surface);
            self.clear_preview_view(surface);
        }
        // A terminal seat holds a shell, and the pane going away takes that too.
        // Closing the pane and leaving the ConPTY alive would leak a process
        // with nothing to draw it and nothing to read it — the pipe fills, and
        // the child blocks forever on a write no one will ever drain.
        if kind == bt_layout::SeatKind::Terminal && self.sessions.contains_key(&seat) {
            // **The ending step, for this one leaf** ([`end_leaves_of`]): the place it held in
            // this window's queue is given up — the program never withdrew and nobody answered,
            // so the ledger says `expire … reason=leaf-gone` and the serial stands where it is —
            // and then its shell is taken, and taken apart somewhere else
            // (T-PANE-CLOSE-OFF-THREAD): a click that closes a pane may not be answered with a
            // `ClosePseudoConsole` that returns when the program inside has finished winding
            // down.
            let index = self.window.active_tab;
            let place = self.window.place();
            let moving = self.cargo_in_flight();
            end_leaves_of(
                [(index, &mut self.window.tabs[index])],
                Some(seat),
                moving.as_ref(),
                |_| notify::desktop_reach(true, place),
                &mut self.window.attention_next_place,
                Instant::now(),
                attention_trace::global(),
            );
            self.sessions.remove(&seat);
            // Keyboard focus cannot stay on a seat that no longer exists. The
            // rule is [`TabState::refocus_after_losing`]'s, shared with the two
            // cross-boundary gestures that also take a leaf out of a tab.
            self.refocus_after_losing(seat);
        }
        // A files column holds a root the way a terminal holds a shell, and the
        // pane going away takes that too. There is no process to shut down —
        // closing a files pane *is* discarding what it was looking at — but the
        // entry has to go, because seat ids are re-minted from a counter and a
        // state left behind comes back as the next column silently inheriting a
        // root nobody chose for it.
        if kind == bt_layout::SeatKind::Files {
            self.files.remove(&seat);
            // And what it had read. The cache is keyed on the same re-minted
            // seat id, so leaving it behind is the same bug one line up in a
            // more confusing shape: the next column would come up already
            // showing somebody else's directories under its own root.
            self.file_trees.remove(&seat);
            // And what it had learned about the repository under it, for exactly
            // the same reason: a re-minted seat id must not come up already
            // holding another column's branches.
            self.git_trees.remove(&seat);
        }
        debug_assert!(
            self.sessions_match_terminals(),
            "item 6: closing a pane leaves the tab's shells matching its tree"
        );
        debug_assert!(
            self.files_match_files_seats(),
            "A3: closing a pane leaves the tab's files states matching its tree"
        );
        self.settle_seat_set_change()?;
        // And then, uniquely to closing: the pointer is standing still over
        // whatever moved *into* the gap, so the hover it lost above is
        // immediately owed again from the new rectangles.
        if let Some(position) = self.window.pointer_position {
            self.update_chrome_hover(position)?;
        }
        Ok(())
    }

    /// The axis `Alt+Shift+D`'s duplicate cuts along.
    ///
    /// **The setting's answer for the focused pane** (user ruling, 2026-08-16),
    /// which under the default `Auto` is what it always was: across the focused
    /// pane's longer side. A pane the solver has not placed cannot be measured,
    /// and the side-by-side split is the one the dev chord had always used.
    ///
    /// It goes through [`Self::settings_split_axis`] rather than reading the
    /// setting itself, because the ruling is that *every* direction-less split
    /// obeys one answer and a second reader is a second place that can stop
    /// obeying it.
    pub(in crate::runtime) fn duplicate_split_axis(&self) -> Axis {
        self.settings_split_axis(self.focused_leaf)
    }

    /// The axis an "auto" split of **one named pane** cuts along: across its
    /// longer side, so the two halves come out as square as the pane allows.
    ///
    /// Windows Terminal's `duplicatePane` defaults to `split: "auto"` and means
    /// exactly this. The measurement is the solver's own rectangle, never a
    /// guess (red line L10); a pane the solver has not placed cannot be measured
    /// and falls to the side-by-side cut the dev chord always used.
    ///
    /// Named seat rather than the focused leaf, because the pane head's `⊞` is
    /// a button on a *particular* head and the pointer can press it over a pane
    /// that does not hold the keyboard. Reading the focused pane's rectangle
    /// there would cut a tall pane sideways because the wide one next door said
    /// so.
    pub(crate) fn pane_split_axis(&self, seat: SeatId) -> Axis {
        let scale = self.window.renderer.scale_factor() as f32;
        seats::pane_body_viewport(&self.seats, &self.seat_layout, seat, scale)
            .map_or(Axis::Row, |body| auto_split_axis(body.width, body.height))
    }

    /// [`Self::split_focused_terminal`], for a pane named outright, arriving on
    /// this pane's own terms.
    ///
    /// The keyboard's three split chords all mean "the pane I am typing in", and
    /// for seven months that was the only way to ask, so the source was read off
    /// `focused_leaf` inside the verb. The pane head's menu asks about the pane
    /// **under the pointer** (user ruling, 2026-08-15), and while D40 does move
    /// focus into a pane on the way down for a press on its head, leaning on
    /// that would make this verb's subject an accident of routing order rather
    /// than something the call site said. So it is said.
    pub(crate) fn split_terminal_seat(&mut self, source: SeatId, dir: Axis) -> Result<()> {
        self.split_seat(source, dir, false, SplitSeed::Inherit)
    }

    /// **Every split in this window, with the one thing they differ in named.**
    ///
    /// Four call sites and one machine: the chords, the menu's picker, the
    /// menu's four verbs, and the drag that lands a pane on an edge. What varies
    /// between them is the axis, which side the arriving leaf goes on, and what
    /// the arriving shell is — and all three are parameters here rather than
    /// three near-copies of the eighty lines below, which is the shape in which
    /// one of them silently stops re-solving before it spawns.
    ///
    /// `leading` is `Edit::SplitSeat`'s own flag, carried through
    /// `Seats::split_terminal` since the tree existed and passed as `false` by
    /// every caller until the picker's `Left` and `Up` zones needed the other
    /// value. **No layout work was owed for them**: "put the new pane first" is
    /// a tree edit the solver has always been able to make.
    pub(in crate::runtime) fn split_seat(
        &mut self,
        source: SeatId,
        dir: Axis,
        leading: bool,
        seed: SplitSeed,
    ) -> Result<()> {
        let metrics = self.seat_metrics();
        // Ask the solver first. `split_terminal` leaves the tree untouched when
        // it refuses, so there is nothing to undo on this path.
        let Some(arriving) = self.seats.split_terminal(&metrics, source, dir, leading) else {
            return Ok(());
        };
        // Re-solve before spawning: the new pane's shell has to be told how many
        // columns it has, and that answer comes from the solve the split just
        // changed — never invented here (red line L10).
        self.commit_seat_geometry()?;
        let scale = self.window.renderer.scale_factor() as f32;
        let Some(body) = seats::pane_body_viewport(&self.seats, &self.seat_layout, arriving, scale)
        else {
            // The solver placed no rectangle for the seat it just minted, which
            // would mean the tree and its solve disagree. Undo the split rather
            // than leave a seat nothing can draw.
            self.seats.close_seat(&metrics, arriving);
            self.commit_seat_geometry()?;
            return Ok(());
        };
        // **The arriving pane is the same kind of shell as the pane it came out
        // of, and stands where that pane stands.** Both halves are inherited from
        // the *source leaf* rather than from the tab, which is the whole of what
        // per-leaf profiles buy at this call site: splitting a Git Bash pane used
        // to seat a PowerShell beside it, because a split spawned the default
        // shell whatever the pane was running. Splitting is "another one of
        // these, here", and it is now able to mean it.
        let inherited = self
            .sessions
            .get(&source)
            // Where the source stands, the one `Restart shell` reads: its report, else its shell
            // process's folder (T-CWD-FROM-PROCESS), else where it was put down — never its
            // profile's default.
            .map(|leaf| seed.applied(&leaf.profile, leaf.seed_place_for_a_new_shell().as_ref()))
            // A seat with no shell to inherit from has no profile to inherit
            // either, and the fallback is the one answer that is startable by
            // construction. Said here rather than left to `LeafSeed::default()`,
            // whose profile is now the empty id — which names no row and would
            // reach the spawn as a degradation nobody caused.
            .unwrap_or_else(|| seed.applied(profiles::fallback_profile_id(), None));
        // **And it is born in this tab** (coordinator's ruling 2026-10-09): a split — which is also
        // `Duplicate pane` and `Split with` — takes what a launch carried into the tab.
        let inherited = born_in_tab(
            inherited,
            self.window.tabs[self.window.active_tab]
                .carried_environment
                .as_ref(),
        );
        let wake = &self.window.pty_wake;
        let formulas = FormulaSwitches::from_settings(self.app.settings_store.loaded());
        let scrollback = scrollback_quota(self.app.settings_store.loaded().scrollback_lines);
        // **A split starts at 100 %** (owner ruling 2026-09-23, 3): it inherits its source's
        // profile and place, and not its text size.
        let view = LeafView::at(&mut self.app.gpu, &self.window.renderer, TextScale::ACTUAL)?;
        let leaf = create_leaf_session(
            view,
            body,
            LeafId {
                tab: self.window.tabs[self.window.active_tab].id,
                seat: arriving,
            },
            wake,
            None,
            &inherited,
            &self.app.profile_programs,
            &self.app.settings_store.loaded().default_profile,
            formulas,
            scrollback,
            self.app.settings_store.loaded().line_wrapping,
            &bt_pty::SystemShellEnvironment,
        )?;
        self.sessions.insert(arriving, leaf);
        debug_assert!(
            self.sessions_match_terminals(),
            "item 6: a split seats a shell for the leaf it minted, and only that"
        );
        // Focus follows the split, keyboard and layout together: you split in
        // order to work in the new pane.
        self.focused_leaf = arriving;
        self.seats.set_focus(arriving);
        self.settle_seat_set_change()?;
        self.refresh_chrome();
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })
    }

    /// **Which live page one preview surface is showing**, docked or torn off
    /// (§7.7 ⑩ 欠账, 2026-08-25).
    ///
    /// [`Self::web_on`] asked of a surface instead of a seat, and it is the seam
    /// the whole of this row's move across turns on: a docked page is the page
    /// on this tab's leaf, a floated one is the page the window is *carrying*
    /// ([`float::FloatPreview::page`]), and `pop_out_preview` closed the seat
    /// that used to be able to answer for it. A glance card never holds a page —
    /// a hover is a question and a browser is not something a question starts.
    pub(in crate::runtime) fn rail_page(&self, surface: PreviewSurface) -> Option<LeafId> {
        match surface {
            PreviewSurface::Seat(leaf) => self.window.web.contains_key(&leaf).then_some(leaf),
            PreviewSurface::Float(id) => self.page_carried_by(id),
            PreviewSurface::Peek => None,
        }
    }

    /// **The band one preview surface's rail rides on**, in physical pixels.
    ///
    /// The two hosts' one genuine disagreement, and the only thing
    /// [`seats::preview_rail_geometry_in`] has to be told: a docked seat's row is
    /// its pane less its head ([`seats::preview_rail_band`]), a window's is the
    /// strip its own chassis reserved ([`float::FloatGeometry::rail`]).
    /// Everything downstream of this rectangle — where each control stands, what
    /// the pointer is on, where a caret goes — is one derivation for both.
    ///
    /// `None` is "this surface wears no row", which is a real answer for three
    /// different reasons and none of them is a failure: a document with nothing
    /// to point at, a pane on a tab that is not in front, and a window squeezed
    /// past its own floor.
    pub(in crate::runtime) fn rail_band(
        &self,
        surface: PreviewSurface,
        scale: f32,
    ) -> Option<[f32; 4]> {
        match surface {
            PreviewSurface::Seat(leaf) => {
                if leaf.tab != self.id {
                    return None;
                }
                let rect = seats::full_pane_rect(&self.seat_layout, leaf.seat)?;
                Some(seats::preview_rail_band(rect, scale))
            }
            PreviewSurface::Float(id) => {
                let win = self.window.float.drawn().find(|win| win.epoch == id)?;
                let fade = self.float_fade_of(win, Instant::now(), scale);
                // `0.0` for the `DOCK` caption, exactly as `float_body_rect`
                // passes: the row is the frame less its border, its head and its
                // foot, and none of those turn on what the head has room for. It
                // is what keeps this an `&self` question, which every reader of
                // it is.
                float::float_geometry(
                    risen_frame(win.frame, fade),
                    win.mode,
                    scale,
                    0.0,
                    self.float_head_tools(id),
                )
                .rail
            }
            PreviewSurface::Peek => None,
        }
    }

    /// **One preview rail, exactly as it was drawn this frame.**
    ///
    /// The band this surface stands on and the widths the paint stored for it,
    /// put together in the one place — because a tip, a menu and a press that
    /// each built their own would be three rows disagreeing about where a button
    /// is. [`Self::preview_rail_measure`]'s own sentence, with the derivation
    /// finished rather than left to each caller.
    pub(in crate::runtime) fn rail_geometry(
        &self,
        surface: PreviewSurface,
        scale: f32,
    ) -> Option<seats::PreviewRailGeometry> {
        let measure = self.preview_rail_measure(surface)?;
        Some(seats::preview_rail_geometry_in(
            self.rail_band(surface, scale)?,
            scale,
            &measure,
        ))
    }

    /// Put one pane's window at content column `wanted`.
    ///
    /// Expressed through the projection's own mutator so the clamp stays in the
    /// one place that owns it — `HorizontalProjection::new` — and the drag and
    /// the wheel arrive at the same column by the same arithmetic.
    pub(crate) fn scroll_seat_to_column(
        &mut self,
        seat: SeatId,
        wanted: ContentColumn,
    ) -> Result<()> {
        let active = self.window.active_tab;
        let Some(leaf) = self.window.tabs[active].sessions.get_mut(&seat) else {
            return Ok(());
        };
        if leaf.projection.horizontal().x_origin() == wanted {
            return Ok(());
        }
        leaf.projection.set_horizontal_origin(wanted);
        self.wake_terminal_column(seat);
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        self.repaint_pane_change(seat)
    }

    /// Move one pane's window `columns` to the right, negative for left.
    pub(in crate::runtime) fn scroll_seat_by_columns(
        &mut self,
        seat: SeatId,
        columns: i32,
    ) -> Result<()> {
        let active = self.window.active_tab;
        let Some(leaf) = self.window.tabs[active].sessions.get_mut(&seat) else {
            return Ok(());
        };
        let before = leaf.projection.horizontal().x_origin();
        leaf.projection.scroll_horizontal_by(columns);
        // The wake is unconditional even when the window was already against the
        // end: a reader flicking at a hard stop is still reading sideways, and a
        // mark that went out under their hand would say they had stopped.
        self.wake_terminal_column(seat);
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        if self.leaf(seat).projection.horizontal().x_origin() == before {
            return Ok(());
        }
        self.repaint_pane_change(seat)
    }

    /// Put one pane's view at `wanted` subpixels above the live bottom.
    ///
    /// Expressed as a delta through `scroll_by_subpixels` rather than as a
    /// setter of its own, so the clamp stays in the one place that owns it —
    /// the drag and the wheel end up at the same extent by the same arithmetic.
    pub(crate) fn scroll_seat_to_subpixels(&mut self, seat: SeatId, wanted: i64) -> Result<()> {
        // The bar moves the view like any other scroll, so it lands a band that is still changing
        // face like any other scroll (§7.1.5p ⑪).
        self.settle_math_toggle()?;
        let active = self.window.active_tab;
        let Some(leaf) = self.window.tabs[active].sessions.get_mut(&seat) else {
            return Ok(());
        };
        let current = leaf.projection.scroll_offset_subpixels();
        if current == wanted {
            return Ok(());
        }
        leaf.projection.scroll_by_subpixels(wanted - current);
        self.woke_terminal_thumb(seat)?;
        self.repaint_pane_change(seat)
    }

    /// Re-solve after a tree edit and carry the consequences to the terminal.
    ///
    /// Deliberately routed through the same coalescer a window resize uses: a
    /// divider drag and an OS resize are the same event as far as ConPTY is
    /// concerned, and §4.2 says the solver does not participate in that
    /// debounce — it answers every frame, and someone else decides when the
    /// child hears about it.
    pub(crate) fn commit_seat_geometry(&mut self) -> Result<()> {
        let trace_started = self.app.trace_perf.then(Instant::now);
        let render_physical =
            presentation_physical_size(self.window.renderer.presentation_geometry());
        if render_physical.width == 0 || render_physical.height == 0 {
            return Ok(());
        }
        // A seat rectangle changing is a resize as far as a transient flyout is
        // concerned: its anchor was a physical point on the old pane and its
        // raster was sized to it. tiny-window §3.5 generalises the existing
        // dissolve rule to exactly this case, so the same two lines `resize`
        // already runs run here.
        self.window.peek_hover.clear();
        self.window.renderer.set_peek_overlay(None);
        let now = Instant::now();
        self.defer_preview_resample(now);
        // **U8 — `snapshotPanes()` (mock-up 6557-6562), before the layout moves.**
        //
        // What it measures is `getBoundingClientRect`, which is where each pane
        // *is on screen* — the solved rectangle with whatever transform is still
        // running already applied. That is what makes a second split 80ms into
        // the first one's flight start from the 60% of the way the pane had
        // genuinely travelled, instead of teleporting to a box it never reached.
        // Taken here rather than inside the re-solve because this is the last
        // moment `self.seat_layout` still describes what the user can see.
        let before = self
            .window
            .pane_motion
            .snapshot(&self.pane_rects(), now, self.app.motion);
        self.resolve_seat_layout(render_physical);
        let solved_at = trace_started.map(|_| Instant::now());
        let next_grid =
            self.resize_leaves_to_layout(now, "resize terminal actor for a seat layout change")?;
        let resized_at = trace_started.map(|_| Instant::now());
        // **U8, R5 — only a structural tree change animates.**
        //
        // Every layout-mutating path in this file converges here, and most of
        // them are not structural: a divider drag steers a ratio, a focus change
        // feeds W2's concession ladder, a DPI change re-solves the same tree on
        // a new rectangle. All of them move rectangles; none of them is a pane
        // arriving, leaving or changing places, so "the layout re-solved" cannot
        // be the gate and the tree's own revision is.
        //
        // Nothing is cancelled on the other branch, and that is CSS's answer as
        // much as ours: a transform is expressed *relative* to whatever box the
        // element is currently laid out in, so a flight that is running when the
        // window is resized keeps its remaining offset and decays onto the new
        // rectangle. Both draw seams read the transform against the live solve,
        // which is that behaviour with no extra code.
        if self.window.pane_motion_revision != self.seats.structure_revision() {
            self.window.pane_motion_revision = self.seats.structure_revision();
            let after = self.pane_rects();
            self.window
                .pane_motion
                .begin(&before, &after, now, self.app.motion);
            // The chrome `resolve_seat_layout` built a moment ago was built
            // through the *previous* frame's transforms, because the tweens that
            // decide it did not exist yet. Rebuilt here rather than by moving
            // the solve: only a structural commit pays for it, which is once per
            // split rather than once per divider event.
            self.refresh_chrome();
        }
        self.sync_math_layout_key();
        // The grid actually in force, which inside a coalescing window is not yet the one the
        // child has heard. The present gate admits the grid the frame will really carry, never the
        // one merely solved.
        //
        // A tab with no shell has no grid and nothing to gate: the present it is
        // about to make is chrome and a files column, neither of which is
        // measured in cells (§7.1.6h).
        self.owe_resize_present();
        self.mark_session_dirty(now);
        self.publish_frame(FrameTrigger {
            occurred_at: now,
            source: FrameSource::Resize,
        })?;
        let published_at = trace_started.map(|_| Instant::now());
        let synchronous_present = self.held_divider_drag().copied().is_none();
        // Pointer motion must stay ahead of the swapchain. `publish_frame` already requested a
        // redraw and `LatestFrameSlot` keeps the newest geometry, so presenting synchronously here
        // would make every divider event wait on GPU acquire/vsync before Windows can deliver the
        // next event. Non-drag seat edits still present immediately; a live drag is frame-paced by
        // RedrawRequested and may coalesce only superseded intermediate positions.
        if synchronous_present {
            self.redraw()?;
        }
        if let (Some(started), Some(solved), Some(resized), Some(published), Some(next_grid)) = (
            trace_started,
            solved_at,
            resized_at,
            published_at,
            next_grid,
        ) {
            crate::diagnostics::stderr_line(format!(
                "BT_PERF_TRACE resize_frame solve_us={} actor_us={} publish_us={} redraw_us={} total_us={} queued={} columns={} rows={}",
                solved.saturating_duration_since(started).as_micros(),
                resized.saturating_duration_since(solved).as_micros(),
                published.saturating_duration_since(resized).as_micros(),
                Instant::now()
                    .saturating_duration_since(published)
                    .as_micros(),
                started.elapsed().as_micros(),
                u8::from(!synchronous_present),
                next_grid.columns,
                next_grid.rows,
            ));
        }
        Ok(())
    }

    /// The row geometry of whichever host this is — **one answer for two
    /// surfaces** (P150).
    ///
    /// Both hosts draw the same list with the same row height and the same
    /// scroll, and both the hit test and the paint already go through
    /// [`seats::files_tree_geometry`]. Asking it once here is what lets the drag
    /// source, the glance card and the press all measure a row the same way; a
    /// second derivation would be a rectangle that disagrees with the one under
    /// the pointer, which is the whole class of bug the hit test's own note is
    /// about.
    pub(in crate::runtime) fn row_geometry(
        &mut self,
        host: RowHost,
    ) -> Option<seats::FilesTreeGeometry> {
        let scale = self.window.renderer.scale_factor() as f32;
        match host {
            // A Git page is not a tree and has no tree geometry. Everything that
            // needs a row's rectangle asks [`Self::peek_row_rect`], which knows
            // both; this function answers only for the two hosts that really are
            // one list, and answering `None` here rather than inventing a
            // geometry is what keeps a drag from starting on a page that has no
            // rows to drag.
            // A terminal has no tree geometry either, and for the stronger
            // reason: it has no rows. Its glance is anchored on the run under
            // the pointer ([`Self::terminal_reference_at`]), which is the whole
            // of what a reference has instead of a row rectangle.
            RowHost::Git(_) | RowHost::Terminal(_) => None,
            RowHost::Column(seat) => {
                let trees = self.files_trees(Instant::now());
                let segmented = self.git_panel_on();
                seats::files_tree_geometry_of(&self.seat_layout, &trees, scale, segmented, seat)
            }
            RowHost::Float(id) => {
                let tools = self.float_head_tools(id);
                let dock_label = self.float_dock_label_width(scale);
                let now = Instant::now();
                let win = self.window.float.drawn().find(|win| win.epoch == id)?;
                let files = win.files()?;
                let rows = files::tree_view(&files.files, &files.cache).rows.len();
                let scroll = files.cache.scroll_px;
                let geometry = float::float_geometry(
                    risen_frame(win.frame, self.float_fade_of(win, now, scale)),
                    win.mode,
                    scale,
                    dock_label,
                    tools,
                );
                Some(seats::files_tree_geometry(
                    geometry.body,
                    rows,
                    scroll,
                    scale,
                ))
            }
        }
    }

    /// **The space verb's second half** — the leaf the plan just landed is
    /// empty, and this is what it was landed for.
    ///
    /// Split out from the commit because the *tree* edit and the *content* it
    /// carries are two different failures: a plan that does not fit never
    /// reaches here, and a file that cannot be read still leaves a pane the user
    /// asked for, with the preview's own refusal card in it.
    fn fill_row_leaf(&mut self, payload: &RowPayload, seat: SeatId) -> Result<()> {
        match payload.kind {
            RowPayloadKind::File => {
                self.open_preview_onto(self.preview_here(seat), payload.path.clone())
            }
            RowPayloadKind::Folder => {
                let root = payload.path.display().to_string();
                let active = self.window.active_tab;
                self.window.tabs[active].files.insert(
                    seat,
                    seats::FilesLeafState {
                        root,
                        ..seats::FilesLeafState::default()
                    },
                );
                self.window.tabs[active].file_trees.remove(&seat);
                self.window.tabs[active].git_trees.remove(&seat);
                self.mark_session_dirty(Instant::now());
                if self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
                Ok(())
            }
        }
    }

    /// The gate's box this frame, or `None` when nothing is being asked.
    pub(in crate::runtime) fn dirty_gate_layout(&mut self) -> Option<restore::GateLayout> {
        let request = self.window.dirty_gate.request()?.clone();
        let names = self.gate_dirty_names(&request);
        if names.is_empty() {
            return None;
        }
        // The request's lines, and the line a question put at the end of a close's
        // wait adds (S-6b).
        let lines = self.window.dirty_gate.lines(&names);
        let title = gate_title_over(&self.window.tabs, self.window.active_tab, &request);
        // **`Discard all` when the card is a list, `Discard` when it names one
        // thing** (B1). The word is a fact about the subject, and the subject is
        // what `offers_save` is deciding about too.
        let discard_text = if request.offers_save() {
            restore::gate_discard_all_text()
        } else {
            request.answer_text()
        };
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (width, height) = (width as f32, height as f32);
        let scale = self.window.renderer.scale_factor() as f32;
        let room = restore::content_width(width, scale);
        let offers_save = request.offers_save();
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let content = restore::GateContent {
            title,
            // Each line is wrapped on its own, so a file name too long for the
            // dialog runs onto a second line of its own row rather than being
            // run together with the name below it.
            message_lines: lines
                .iter()
                .flat_map(|line| {
                    restore::wrap(line, room, |run| {
                        renderer.measure_chrome_text(gpu, run, restore::SUB_FONT_LOGICAL_PX * scale)
                    })
                })
                .collect(),
            discard_text,
            cancel_text_width: renderer.measure_chrome_text(
                gpu,
                restore::gate_cancel_text(),
                restore::BUTTON_FONT_LOGICAL_PX * scale,
            ),
            discard_text_width: renderer.measure_chrome_text(
                gpu,
                discard_text,
                restore::BUTTON_FONT_LOGICAL_PX * scale,
            ),
            save_text_width: offers_save.then(|| {
                renderer.measure_chrome_text(
                    gpu,
                    restore::quit_save_text(),
                    restore::BUTTON_FONT_LOGICAL_PX * scale,
                )
            }),
        };
        Some(restore::gate_layout(&content, width, height, scale))
    }

    /// Where the file menu is, if one is up.
    ///
    /// Unlike [`Runtime::root_menu_layout`] this cannot fold for want of
    /// something to hang from: the anchor is the point the pointer was at, and a
    /// point does not stop existing when the tree behind it is rebuilt — so it is
    /// one of the seven popups [`Runtime::popups_up`] has no stand to ask about.
    /// What the menu *does* fold for is the window changing size under it, which
    /// the caller handles by closing it — a menu is a moment, and a resize ends
    /// the moment.
    pub(in crate::runtime) fn file_menu_layout(&mut self) -> Option<profiles::FileMenuLayout> {
        let menu = self.window.file_menu.as_ref()?;
        let point = menu.point;
        let crumbs: Vec<String> = menu.crumbs.iter().map(|level| level.name.clone()).collect();
        let (subject, powers) = (menu.subject, file_menu_powers(menu.row.as_ref()));
        let look = self.file_menu_look(subject, powers, &crumbs);
        let scale = self.window.renderer.scale_factor() as f32;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        Some(profiles::file_menu_layout(
            point,
            (width as f32, height as f32),
            scale,
            &look,
            &mut measure,
        ))
    }

    /// Where the terminal menu is, if one is up.
    ///
    /// [`Runtime::git_menu_layout`]'s twin down to the borrow dance: the look is
    /// a bundle of `Copy` scalars rather than borrows, so unlike the git menu it
    /// needs no owned draw struct — the measure closure can hold the renderer
    /// mutably while the look sits on the stack.
    pub(in crate::runtime) fn term_menu_layout(&mut self) -> Option<profiles::TermMenuLayout> {
        let menu = self.window.term_menu.as_ref()?;
        let (point, look) = (
            menu.point,
            profiles::TermMenuLook {
                pane: menu.pane,
                subject: menu.subject,
                hover: menu.hover,
                lone: menu.lone,
                submenu_open: menu.submenu_open,
            },
        );
        let scale = self.window.renderer.scale_factor() as f32;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        // **The effective table**, so a rebound chord follows into the menu the
        // same frame it follows into the hint card (gesture audit 2026-08-26,
        // 系统性发现 ②). Borrowed beside the GPU rather than through it: the two
        // are separate fields of `app` and the split borrow is what lets the
        // menu be measured and read from the same expression.
        let shortcuts = &self.app.shortcuts;
        // **The machine**, on the shortcut table's own footing and for the
        // ruling of 2026-09-06: it decides which rows the `Split with ▸` child
        // has, so the menu cannot be measured without it.
        let programs = &self.app.profile_programs;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        Some(profiles::term_menu_layout(
            point,
            (width as f32, height as f32),
            scale,
            &look,
            shortcuts,
            programs,
            &mut measure,
        ))
    }

    /// `Select all` — **every plane this pane has**, not the screenful in front
    /// of you.
    ///
    /// The anchors come from [`DualPlaneSession::select_all_selection`], which
    /// walks the same three planes a copy is cut from, and they are installed
    /// through [`TabState::set_leaf_selection`] — the one door that also drops
    /// every other pane's selection, because this window holds one.
    pub(in crate::runtime) fn select_all_in_pane(&mut self, seat: SeatId) -> Result<()> {
        let Some(selection) = self
            .sessions
            .get(&seat)
            .and_then(|leaf| leaf.session.select_all_selection())
        else {
            return Ok(());
        };
        self.set_pane_view_selection(seat, Some(selection));
        self.repaint_pane_change(seat)
    }

    /// `Clear screen` — **the screen scrolls away and the row you are typing on
    /// stays**, performed by the terminal on both sides of the pty (§7.1.6,
    /// clear-screen ruling 2026-09-05).
    ///
    /// Nothing is written to the PTY as *input*, which is the whole of the
    /// ruling's first half: `cls` typed at a prompt is a *command*, and a menu
    /// row that typed one for you would land in the middle of whatever
    /// half-finished line was already there, would be refused outright by a
    /// program that is not a shell, and would enter that shell's history.
    ///
    /// **Why the cursor's row is kept, and why the host is told.** The first
    /// draft of this row fed `ESC [ 2 J` `ESC [ H` into this session's own parser
    /// and stopped there. Measured on a real ConPTY (`crates/bt-pty`'s
    /// `clear_screen_keeps_the_prompt_and_the_host_agrees`), that leaves a pane
    /// with no prompt on it and no way to get one: the child's screen lives in
    /// the console host's buffer, ConPTY sends this window only the difference
    /// against what *it* believes is displayed, and a clear it never heard about
    /// gives it no reason to redraw anything. The recorded evidence is that the
    /// child says nothing at all afterwards, and that the next keystroke arrives
    /// as an absolute `CUP` to the row and column the prompt used to end at — a
    /// lone character floating on a blank pane, which is precisely the defect
    /// this rewrite answers.
    ///
    /// So: this window keeps the cursor's row and moves it to the top
    /// (`DualPlaneSession::clear_screen_keeping_cursor_row`), and the host is
    /// asked to do the same to its own buffer
    /// (`PtySession::clear_host_buffer`, `keepCursorRow`). The two then agree —
    /// measured: after the signal the host addresses the kept row as row 1 — so
    /// the prompt is on the screen the moment the menu closes and stays where
    /// both sides think it is.
    ///
    /// The rows above it **scroll out into history the ordinary way**, so
    /// everything cleared is still there to be scrolled back to and still there
    /// to be searched. That is exactly what the row below it is not.
    pub(in crate::runtime) fn clear_pane_screen(&mut self, seat: SeatId) -> Result<()> {
        let Some(leaf) = self.sessions.get_mut(&seat) else {
            return Ok(());
        };
        let alternate = leaf.session.terminal_modes().alternate_screen;
        // **The host is asked first, because its answer is what decides the shape.** Not for
        // tidiness: a window that moved rows and only then discovered it had nobody to tell would
        // have already made the screen the 2026-09-05 report describes.
        //
        // **Not on the alternate screen**: that buffer belongs to the program drawing it, which
        // repaints its own canvas with absolute addresses and needs no help keeping in step.
        let host = if alternate {
            bt_term::HostScreen::Cleared
        } else {
            match leaf.pty.as_ref() {
                // No child, so no second buffer to disagree with this one.
                None => bt_term::HostScreen::Cleared,
                Some(pty) => host_screen_after(pty.clear_host_buffer(true)),
            }
        };
        leaf.session
            .clear_screen_keeping_cursor_row(host)
            .context("clear one pane's screen locally")?;
        // The live selection goes with the screen it was drawn on (§7.1.6). Not
        // because the anchors would dangle — they name rows that are now in
        // history — but because a highlight left standing over cleared cells is
        // the window claiming a selection of what is no longer there.
        self.clear_pane_selection(seat);
        self.repaint_pane_change(seat)
    }

    /// `Clear scrollback` — **the whole of §3.1's ED3 pipeline**, on this pane.
    ///
    /// One escape and not a bespoke teardown, and that is the point: `ESC [ 3 J`
    /// already runs transcript, staging, blocks, indexes, caches, anchor
    /// degradation and tombstones through
    /// `LifecycleDirective::ClearHistoryAndStaging`, and the command marks go
    /// with it through the ledger (`retire_command_marks`) rather than through a
    /// second list that would have to be remembered. A menu row that emptied
    /// some of those by hand would be a second, shorter definition of what
    /// deleting history means — and the first thing to fall off it would be
    /// whichever structure the next slice adds.
    pub(in crate::runtime) fn clear_pane_scrollback(&mut self, seat: SeatId) -> Result<()> {
        let Some(leaf) = self.sessions.get_mut(&seat) else {
            return Ok(());
        };
        leaf.session
            .feed(b"\x1b[3J")
            .context("delete one pane's transcript locally")?;
        // A selection that reached into the history it was cut from is a
        // selection of lines that no longer exist.
        self.clear_pane_selection(seat);
        self.repaint_pane_change(seat)
    }

    /// Where the pane menu is, if one is up. [`Runtime::file_menu_layout`]'s
    /// twin, and anchored the same way: at the point the pointer was at, which
    /// no re-layout can move or destroy.
    pub(in crate::runtime) fn pane_menu_layout(&mut self) -> Option<profiles::PaneMenuLayout> {
        let menu = self.window.pane_menu.as_ref()?;
        let (point, submenu, zoomed) = (menu.point, menu.submenu, menu.zoomed);
        let scale = self.window.renderer.scale_factor() as f32;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let windows = self.other_window_rows();
        let shortcuts = &self.app.shortcuts;
        // **The machine**, on `windows`' own footing (user ruling 2026-09-06):
        // both decide how many rows a child has, not merely how wide it is.
        let programs = &self.app.profile_programs;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        Some(profiles::pane_menu_layout(
            point,
            (width as f32, height as f32),
            scale,
            submenu,
            zoomed,
            &windows,
            shortcuts,
            programs,
            &mut measure,
        ))
    }

    /// Raise the menu on one pane head.
    ///
    /// **Terminal heads only.** Every verb here is about a shell — four of them
    /// put a second one somewhere, one moves this one, one ends it — and closing
    /// a files column or a preview is what that head's own `×` is for. A menu of
    /// verbs a surface cannot perform is worse than no menu: the same refusal
    /// [`Runtime::open_file_menu`] makes for a directory row, made here for the
    /// same reason and in the same one place rather than at each caller.
    pub(in crate::runtime) fn open_pane_menu(
        &mut self,
        seat: SeatId,
        point: [f32; 2],
    ) -> Result<()> {
        if !self.sessions.contains_key(&seat) {
            return Ok(());
        }
        // E61: the opener closes the others.
        self.close_popups_except(Popup::Pane);
        // **A pane menu already up — another head's — is replaced, which is its
        // going away** (owner ruling 2026-09-23): the pin follows the menu that
        // is up, so the new one starts unpinned and a press that raised it pins
        // it on its own way out.
        self.window.chevrons.menu_gone(Popup::Pane);
        // Its `Split with` lists programs: the machine is asked again as it opens.
        self.ask_the_program_walk(crate::programs_lane::Trigger::ProgramMenu);
        self.window.pane_menu = Some(PaneMenuState {
            point,
            seat,
            zoomed: self.seats.seat_is_zoomed(seat),
            hover: None,
            lit_by: profiles::LitBy::Pointer,
            submenu: None,
            pointer_was: None,
            submenu_hold_until: None,
        });
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The `⌄`'s verb: open the menu under it, or put away the one it opened
    /// (user ruling, 2026-08-16).
    ///
    /// The toggle half is the `⌄` grammar's second door — a click opens what a
    /// rest would have opened, and a click on a button whose menu is already up
    /// closes it. Without it the hover rule would make the button unclickable in
    /// practice: by the time a hand has travelled to a chevron and pressed it,
    /// the rest has usually already opened the menu, and a press that opened it
    /// *again* would be a press that did nothing at all.
    ///
    /// The menu drops from the button's own bottom-left corner rather than from
    /// the pointer, which is where a menu belonging to a *control* hangs — the
    /// strip's `⌄` does the same, and a right click on the head still drops this
    /// same menu at the pointer, because that gesture belongs to the surface
    /// rather than to a button on it.
    pub(in crate::runtime) fn toggle_pane_menu(&mut self, seat: SeatId) -> Result<()> {
        if self
            .window
            .pane_menu
            .as_ref()
            .is_some_and(|menu| menu.seat == seat)
        {
            self.window.chevrons.clear();
            self.close_pane_menu()?;
            return Ok(());
        }
        let Some(anchor) = self.pane_chevron_box(seat) else {
            return Ok(());
        };
        let scale = self.window.renderer.scale_factor() as f32;
        self.open_pane_menu(
            seat,
            [
                anchor[0],
                anchor[3] + profiles::MENU_OFFSET_LOGICAL_PX * scale,
            ],
        )
    }

    /// The `⌄`'s box on one pane, or `None` when that pane has no room for one.
    ///
    /// **Two layouts, one button** (§7.1.6i): a pane with a head answers with
    /// the chevron in its run, and a lone terminal answers with the corner
    /// ghost. Everything downstream — the menu's anchor, the tooltip's anchor,
    /// the toggle — is written once and works in both.
    ///
    /// Re-derived from the frame the seat is standing in rather than remembered,
    /// which is the `&self`-hit-test discipline every other control in this
    /// window keeps: the rectangle a menu hangs off has to be the rectangle the
    /// button was drawn in, by one derivation and not by two that agree today.
    fn pane_chevron_box(&self, seat: SeatId) -> Option<[f32; 4]> {
        let scale = self.window.renderer.scale_factor() as f32;
        seats::pane_chevron_box(
            &self.seats,
            &self.seat_layout,
            seat,
            scale,
            self.window.search.seat(),
        )
    }

    /// The pane menu's own level of the overlay stack, or nothing when none is
    /// up.
    pub(in crate::runtime) fn pane_menu_layer(&mut self) -> MenuPaint {
        let Some(layout) = self.pane_menu_layout() else {
            return MenuPaint::none();
        };
        let Some(menu) = self.window.pane_menu.as_ref() else {
            return MenuPaint::none();
        };
        let travel = layout.travel();
        let child_travel = layout.submenu_travel().unwrap_or(Travel::Right);
        let (hover, seat) = (menu.hover, menu.seat);
        // The profile the pane is *running*, which is what the submenu marks —
        // never the window's default. A pane you split from a Git Bash is a Git
        // Bash, and a submenu that ticked PowerShell on it would be telling you
        // about the window rather than about the pane the menu was raised on.
        // `position_of` and not `index_of_id`: the tick names a row of the table
        // being drawn, and a pane whose profile has been deleted has no row to
        // tick rather than the fallback's.
        let current = self
            .sessions
            .get(&seat)
            .and_then(|leaf| profiles::position_of(&leaf.profile));
        let windows = self.other_window_rows();
        let programs = &self.app.profile_programs;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        let mut layers =
            profiles::pane_menu_build(&layout, hover, current, programs, &windows, &mut measure);
        // `push_submenu`'s seam again: the menu first, the child after it.
        let child = layers.split_off(1);
        MenuPaint {
            menu: layers,
            travel,
            child: Some((child, child_travel)),
        }
    }

    pub(in crate::runtime) fn close_pane_menu(&mut self) -> Result<bool> {
        if self.window.pane_menu.take().is_none() {
            return Ok(false);
        }
        self.window.chevrons.menu_gone(Popup::Pane);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// Open or shut the `Split with` submenu, and report whether anything moved.
    ///
    /// The hold is cleared on both edges: an opening submenu has nothing to
    /// survive yet, and a closing one has nothing left to survive for.
    pub(in crate::runtime) fn set_pane_submenu(
        &mut self,
        open: Option<profiles::PaneMenuRow>,
    ) -> Result<bool> {
        let Some(menu) = self.window.pane_menu.as_mut() else {
            return Ok(false);
        };
        if menu.submenu == open {
            return Ok(false);
        }
        // **The heading is the row that was open, not a name written down here**
        // (B9). It said `SplitWith` while that was the only row with a list, and
        // a closing `Move to window` would have handed the keyboard to a row two
        // above the one it was standing on.
        let was = menu.submenu;
        menu.submenu = open;
        menu.submenu_hold_until = None;
        // The highlight follows the surface it is on. Opening lands on the first
        // row, which is what `→` and a click both mean; closing takes the
        // highlight back to the heading it came from, so `←` leaves the keyboard
        // somewhere rather than nowhere.
        // Placed, not pointed at: it follows its item until a hand moves it.
        menu.lit_by = profiles::LitBy::Keyboard;
        menu.hover = match (open, was) {
            (Some(_), _) => Some(profiles::PaneMenuHover::Submenu(0)),
            (None, Some(heading)) => Some(profiles::PaneMenuHover::Row(heading)),
            (None, None) => menu.hover,
        };
        // **The ring goes out with the list** (B9): a highlighted window with no
        // menu naming it is a window wearing a mark nobody can explain. Armed
        // again by the first hover inside the list that has just opened.
        self.aim_at_window(None);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// **The pane menu's hover, with the safety triangle in it** (#53).
    ///
    /// Answers whether the pointer is on either of the menu's two surfaces, so
    /// the caller can stop the pointer stream there exactly as it does for every
    /// other popup.
    ///
    /// Four things happen, in this order and for these reasons:
    ///
    /// 1. **What is under the pointer** is asked once, of the same layout the
    ///    menu was drawn from.
    /// 2. **The triangle is consulted only when it could change the answer** —
    ///    that is, only when a submenu is open and the pointer has moved onto
    ///    something that is neither the submenu nor its heading. Everywhere else
    ///    it has no opinion, and asking it would be a rule with no subject.
    /// 3. **A held highlight does not move**, which is the whole of what the
    ///    triangle buys: the row the hand is crossing does not light up, and the
    ///    submenu it was crossing toward stays open.
    /// 4. **The apex is re-seated** on every move the triangle does *not* hold,
    ///    so a hand that changes direction is measured from where it changed
    ///    rather than from where it started three rows ago.
    pub(in crate::runtime) fn drive_pane_menu_hover(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let Some(layout) = self.pane_menu_layout() else {
            return Ok(false);
        };
        let hit = profiles::pane_menu_hit(&layout, position.x, position.y);
        let submenu = layout.submenu_frame();
        let to = [position.x as f32, position.y as f32];
        let now = Instant::now();
        let Some(menu) = self.window.pane_menu.as_mut() else {
            return Ok(false);
        };
        // **On the child at all**, which is a question about its frame and not
        // about its rows — see [`profiles::PaneMenuLayout::on_submenu`]. Reading
        // it off `hit` alone was the second cause of the 2026-08-19 report: the
        // hit says `Surface` both for the parent's padding and for the child's,
        // so a hand landing in the child's own leading border-and-padding read
        // as "not on the child", the triangle was asked about a pointer already
        // past its base, and the child shut under the hand that had just reached
        // it. The heading keeps its place beside it: a hand back on the row the
        // child hangs from has not left the child either.
        let heading = menu.submenu;
        let hovering_child = layout.on_submenu(to[0], to[1])
            || matches!(hit, Some(profiles::PaneMenuHit::Row(row)) if Some(row) == heading);
        let was_open = menu.submenu;
        let mut held = false;
        if let Some(submenu) = submenu
            && !hovering_child
        {
            // The hold is armed the first time the pointer leaves the child's
            // corridor and is spent by the clock, never by a row: a hand that
            // stops inside the triangle sends no more events, so the cap has to
            // be a deadline rather than a count of moves.
            let from = menu.pointer_was.unwrap_or(to);
            if profiles::safe_triangle_holds(from, to, submenu) {
                let until = *menu
                    .submenu_hold_until
                    .get_or_insert(now + profiles::SUBMENU_SAFE_HOLD);
                held = now < until;
            }
            if !held {
                menu.submenu = None;
                menu.submenu_hold_until = None;
            }
        } else {
            menu.submenu_hold_until = None;
        }
        // The apex only moves when the triangle is not holding. While it holds,
        // the aim is still the one the hand set when it left the heading — a
        // triangle re-drawn from each intermediate position narrows to nothing
        // and the whole rule evaporates halfway across.
        if !held {
            menu.pointer_was = Some(to);
        }
        let hovered = match hit {
            Some(profiles::PaneMenuHit::Zone(zone)) => Some(profiles::PaneMenuHover::Zone(zone)),
            Some(profiles::PaneMenuHit::Row(row)) => Some(profiles::PaneMenuHover::Row(row)),
            Some(profiles::PaneMenuHit::Submenu(index)) => {
                Some(profiles::PaneMenuHover::Submenu(index))
            }
            // The padding, and everywhere outside: nothing is lit. A menu whose
            // last-hovered row stayed lit while the pointer sat in its own margin
            // would be a menu Enter could fire from a place that looks idle.
            Some(profiles::PaneMenuHit::Surface) | None => None,
        };
        // The submenu going is a change even when the highlight does not move —
        // a hold that expires over the row it was already on takes a whole
        // window off the screen, and a repaint skipped because "the hover is the
        // same" would leave that window drawn over nothing.
        let mut changed = menu.submenu != was_open;
        if !held && menu.hover != hovered {
            menu.hover = hovered;
            menu.lit_by = profiles::LitBy::Pointer;
            changed = true;
        }
        // **The ring is the hover, seen from the other window** (B9). Read off
        // the highlight rather than off the hit, so the keyboard's walk lights
        // the same window the pointer's would.
        let ring = match (layout.submenu_kind(), menu.hover) {
            (
                Some(profiles::PaneMenuRow::MoveToWindow),
                Some(profiles::PaneMenuHover::Submenu(at)),
            ) => Some(at),
            _ => None,
        };
        // Resting on the heading opens the child, on the same 250ms the chevrons
        // themselves take (user ruling, 2026-08-16) — one number for "a hand has
        // settled on something that has more behind it". It is armed here and
        // matured in `advance_pane_menu`.
        let resting_on = match hit {
            Some(profiles::PaneMenuHit::Row(row)) if row.has_submenu() => Some(row),
            _ => None,
        };
        if resting_on.is_some() && menu.submenu != resting_on {
            menu.submenu_hold_until
                .get_or_insert(now + profiles::CHEVRON_HOVER_OPEN_DELAY);
        }
        let inside = hit.is_some();
        let aim = ring.and_then(|at| self.other_window_ids().get(at).copied());
        self.aim_at_window(aim);
        if changed && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(inside)
    }

    /// The pane menu's own clocks, matured.
    ///
    /// Two live in the one slot and they are never both meaningful: while the
    /// submenu is shut the deadline is the heading's 250ms rest, and while it is
    /// open it is the safety triangle's 300ms cap. One field because there is one
    /// question — "what does this menu owe at some instant" — and a menu cannot
    /// be both waiting to open its child and holding it open against the rows.
    pub(in crate::runtime) fn advance_pane_menu(&mut self, now: Instant) -> Result<()> {
        let Some(menu) = self.window.pane_menu.as_mut() else {
            return Ok(());
        };
        let Some(due) = menu.submenu_hold_until else {
            return Ok(());
        };
        if now < due {
            return Ok(());
        }
        // **A matured hold is spent on the turn it comes due, whatever follows**
        // (T-WINDOW-THREAD-UNDER-LOAD). Left standing by a road below that cannot act — a rest
        // whose pointer has left the window, a heading that has no child to open — it was an
        // instant in the past that the wake fold handed `ControlFlow::WaitUntil` on every turn,
        // and the loop spun until the next pointer move. Whatever the roads below arm again
        // (the hover they drive) is a new hold of its own.
        menu.submenu_hold_until = None;
        if menu.submenu.is_some() {
            // The cap ran out with the hand still short of the child. The
            // submenu goes, and the highlight is owed to whatever the pointer is
            // actually over — which is asked of the geometry rather than
            // remembered, because the rows have not moved but the hold was
            // deliberately keeping the answer stale.
            self.set_pane_submenu(None)?;
            if let Some(position) = self.window.pointer_position {
                self.drive_pane_menu_hover(position)?;
            }
            return Ok(());
        }
        // The rest on the heading matured: the child opens, exactly as a rest on
        // a chevron opens its own menu. **Which child is a fact about where the
        // hand is resting** (B9), asked of the same hit test the hover reads.
        let Some(row) = self
            .pane_menu_row_under_pointer()
            .filter(|row| row.has_submenu())
        else {
            return Ok(());
        };
        self.set_pane_submenu(Some(row))?;
        Ok(())
    }

    /// Which row of the pane menu the pointer is on, if it is on one.
    ///
    /// Its own reader because two callers want it at moments when neither has a
    /// layout in hand — a matured rest and a press — and re-deriving it beside
    /// each of them is how one menu grows two hit tests.
    fn pane_menu_row_under_pointer(&mut self) -> Option<profiles::PaneMenuRow> {
        let position = self.window.pointer_position?;
        let layout = self.pane_menu_layout()?;
        match profiles::pane_menu_hit(&layout, position.x, position.y) {
            Some(profiles::PaneMenuHit::Row(row)) => Some(row),
            _ => None,
        }
    }

    /// The pane menu's next wake-up, for the loop's set.
    pub(in crate::runtime) fn pane_menu_deadline(&self) -> Option<Instant> {
        self.window.pane_menu.as_ref()?.submenu_hold_until
    }

    /// One docked chrome target, spelled the way a trigger is spelled.
    ///
    /// The two controls a float can also wear are named by surface and part, so
    /// that one button has one name whichever host drew it; everything else in
    /// the chrome exists on one host only and wears its own target. See
    /// [`PopoverTrigger`] for why a second spelling would be a second button.
    pub(in crate::runtime) fn docked_popover_trigger(
        &self,
        target: seats::ChromeTarget,
    ) -> PopoverTrigger {
        if let Some((seat, part)) = seats::preview_rail_control(target) {
            return PopoverTrigger::Rail(self.preview_here(seat), part);
        }
        if let seats::ChromeTarget::GitGraphTool { seat, tool } = target {
            return PopoverTrigger::GraphTool(self.preview_here(seat), tool);
        }
        PopoverTrigger::Chrome(target)
    }

    /// Do what one entry of the pane menu says, and put the menu away.
    ///
    /// The menu closes *first*, on [`Runtime::run_file_menu_row`]'s order and
    /// for a sharper version of its reason: most of these verbs rebuild the
    /// layout the menu is drawn over, one destroys the pane it was raised on,
    /// and one opens a modal dialog with a message loop of its own.
    ///
    /// The seat comes out of the menu, never from the pointer. Between the press
    /// that raised this and the one that ran it, the pointer has travelled —
    /// down the menu, which is drawn over some *other* pane as often as not.
    pub(in crate::runtime) fn run_pane_menu_row(
        &mut self,
        hit: profiles::PaneMenuHit,
    ) -> Result<()> {
        // **The child's row is resolved while the child still exists** (B9). A
        // `Submenu` hit counts rows on the glass, and what those rows are about
        // is a fact about the layout — which is built out of the menu state the
        // very next line takes away.
        let submenu = match hit {
            profiles::PaneMenuHit::Submenu(at) => {
                let layout = self.pane_menu_layout();
                let kind = layout
                    .as_ref()
                    .and_then(profiles::PaneMenuLayout::submenu_kind);
                let of = layout.as_ref().and_then(|layout| layout.submenu_row(at));
                match (kind, of) {
                    (Some(kind), Some(of)) => Some((kind, of)),
                    // A press on a child that is no longer there. The ordinary
                    // case rather than a fault: a menu can be taken down by
                    // anything between the press and this line.
                    _ => return Ok(()),
                }
            }
            _ => None,
        };
        // Resolved *before* the take, so `other_window_ids` reads the same
        // directory the list was drawn from.
        let bound_for = match submenu {
            Some((profiles::PaneMenuRow::MoveToWindow, of)) => {
                Some(self.other_window_ids().get(of).copied())
            }
            _ => None,
        };
        let Some(menu) = self.window.pane_menu.take() else {
            return Ok(());
        };
        let seat = menu.seat;
        self.window.chevrons.clear();
        self.window.chevrons.menu_gone(Popup::Pane);
        // **The ring goes with the menu**, wherever the press lands: a window
        // still wearing the mark after the list that put it there has gone is a
        // window claiming to be a destination nobody is choosing.
        self.aim_at_window(None);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        if let Some(window) = bound_for {
            let Some(window) = window else {
                return Ok(());
            };
            return self.move_pane_to_window(seat, window);
        }
        // Every other child row is the profile list's, and `run_pane_verb` has
        // taken a `PROFILES` index there since the day the submenu existed.
        let hit = match submenu {
            Some((_, of)) => profiles::PaneMenuHit::Submenu(of),
            None => hit,
        };
        self.run_pane_verb(seat, hit)
    }

    /// **Move this pane into a window that is already open** (B9, user ruling
    /// 2026-08-25).
    ///
    /// [`Runtime::move_pane_to_new_window`] with the destination named instead
    /// of made, and it walks the same road F2's cross-window *drag* already
    /// walks — the pane is promoted to a tab where it stands and the tab is
    /// handed over by [`FolioApp::transfer_tab`]. That road is a
    /// [`DragHandover`] because only `FolioApp` can see two windows at once, and
    /// a menu row arrives at one `Runtime` that can see neither: so the row
    /// records the errand and `about_to_wait` spends it, in the same turn and
    /// before any frame.
    ///
    /// **It is the drag's errand and not a second one**, which is what keeps the
    /// promotion, the transfer, the refusal card and the emptied source tab from
    /// growing a menu-shaped copy of themselves.
    ///
    /// The landing is the end of the target's strip. A drag lands where the hand
    /// let go and this row has no hand over that window at all — so the honest
    /// answer is the one every append in this program gives, which is "after
    /// everything that is already there".
    fn move_pane_to_window(&mut self, seat: SeatId, window: WindowId) -> Result<()> {
        if window == self.window_id() {
            return Ok(());
        }
        let slot = self
            .app
            .windows_open
            .iter()
            .find(|open| open.id == window)
            .map_or(0, |open| open.tabs);
        let leaf = LeafId {
            tab: self.window.tabs[self.window.active_tab].id,
            seat,
        };
        self.app.pending_handover = Some(DragHandover {
            cargo: DragSource::Pane(leaf),
            from: self.window_id(),
            into: HandoverInto::Window {
                window,
                landing: DropLanding::StripExtract { slot },
            },
        });
        Ok(())
    }

    /// **One pane verb, wherever it was asked for** (§7.1.6e's rule: one verb,
    /// two doors, never two implementations).
    ///
    /// The pane menu's rows and §7.1.6i's right-click segment both land here, so
    /// `Duplicate pane` means the same split whichever door raised it and the day
    /// the split's seed changes there is one place to change it. The seat is the
    /// caller's, because the two doors carry it differently — one in its own
    /// menu state, one in the terminal menu's — and both took it from the press
    /// that raised them rather than from where the pointer has since travelled.
    pub(in crate::runtime) fn run_pane_verb(
        &mut self,
        seat: SeatId,
        hit: profiles::PaneMenuHit,
    ) -> Result<()> {
        // The pane may have gone while the menu stood open — its shell exited,
        // or another gesture took it. Every verb below needs it to still be
        // there, so it is asked once.
        if !self.sessions.contains_key(&seat) {
            return Ok(());
        }
        match hit {
            // The picker: the one place in this window where the direction is
            // the *gesture*, so the setting is not consulted. `Left` and `Up`
            // are the same two axes with the arriving leaf inserted first, which
            // is `Edit::SplitSeat`'s `leading` flag and has been since the tree
            // existed.
            profiles::PaneMenuHit::Zone(zone) => {
                self.split_seat(seat, zone.axis(), zone.leading(), SplitSeed::Inherit)
            }
            // The row's id, read off the table the submenu was drawn from: the
            // press is the last moment this position is certainly that profile,
            // and the split it seeds may be a frame or a folder chooser later.
            profiles::PaneMenuHit::Submenu(profile) => self.split_seat(
                seat,
                self.settings_split_axis(seat),
                false,
                SplitSeed::Profile(profiles::id(profile)),
            ),
            profiles::PaneMenuHit::Row(row) => match row {
                // The heading is not a verb: pressing it opens the submenu, and
                // that press never reaches here (see `press_pane_menu`). Listed
                // so the match is exhaustive over a closed set rather than over
                // a wildcard that would silently swallow a row added later.
                profiles::PaneMenuRow::Picker
                | profiles::PaneMenuRow::SplitWith
                | profiles::PaneMenuRow::MoveToWindow => Ok(()),
                // §7.1.6l — the row and the double-click on the head are one
                // verb behind two doors, which is what `run_pane_verb` is for.
                profiles::PaneMenuRow::ZoomPane => self.toggle_pane_zoom(seat),
                profiles::PaneMenuRow::NewInFolder => {
                    self.browse_for_split_root(seat);
                    Ok(())
                }
                // The split's own default: same profile, same directory. It is
                // exactly what a bare split already inherits, so this row is the
                // *name* of that behaviour rather than a second implementation
                // of it — which is why it hands over `Inherit` and not a seed it
                // assembled itself.
                profiles::PaneMenuRow::Duplicate => self.split_seat(
                    seat,
                    self.settings_split_axis(seat),
                    false,
                    SplitSeed::Inherit,
                ),
                profiles::PaneMenuRow::MoveToNewTab => self.move_pane_to_new_tab(seat),
                // The row above it and this one are one journey of two lengths,
                // and this one is *made of* that one — see
                // `Runtime::move_pane_to_new_window`, which promotes the pane
                // through the very function the row above spends and then hands
                // the tab to the application's transfer.
                profiles::PaneMenuRow::MoveToNewWindow => self.move_pane_to_new_window(seat),
                profiles::PaneMenuRow::ResetTerminalModes => self.reset_terminal_modes(seat),
                // The `×`'s own verb, reached through the `×`'s own door — so the
                // gate a destruction has to pass is passed once and not twice.
                profiles::PaneMenuRow::ClosePane => self.close_pane(seat),
            },
            // The menu's own padding. A press there is the menu swallowing it,
            // which `press_pane_menu` already decided; nothing to run.
            profiles::PaneMenuHit::Surface => Ok(()),
        }
    }

    /// **§7.1.6l — this pane alone on the stage, or back to the tiling.**
    ///
    /// One implementation behind both doors, which is §7.1.6e's rule and the
    /// reason the double-click and the menu row both land here rather than each
    /// doing their own version of it.
    ///
    /// The whole verb is a field and a re-solve. There is no restore path
    /// because nothing was taken apart: `Seats::solve` asks the solver for
    /// `LayoutMode::Focus` while the field is set, red line L13 leaves the tree
    /// alone, and clearing the field puts the tiling back because the tiling
    /// never went anywhere.
    ///
    /// `commit_seat_geometry` is the same one every layout-mutating path in this
    /// file converges on, so the panes that are no longer presented stop being
    /// asked for rectangles, the one that is gets ConPTY's resize, and the FLIP
    /// stays out of it — a zoom bumps no structure revision, and a posture
    /// changing is not a pane arriving.
    ///
    /// The window minimum is re-asked for the ordinary reason: it is the
    /// technical floor and the same for every tree (user ruling 2026-08-08), so
    /// this call cannot change it — and it is made anyway, beside its two
    /// siblings in the collapsed-bar arm and in `set_focus`'s, because a caller
    /// that skips it because it happens to know the answer is a caller that will
    /// be wrong the day the answer changes.
    pub(in crate::runtime) fn toggle_pane_zoom(&mut self, seat: SeatId) -> Result<()> {
        // **A zoom takes every other pane off the glass, and the gesture held on one with it**
        // (T-POINTER-CAPTURE R-9, off the glass) — before the posture changes, while that pane
        // is still drawn. Letting a zoom go takes nothing away.
        if self.seats.toggle_would_zoom_in(seat) {
            self.end_capture(Ending::OffTheGlass(Leaving {
                tab: self.id,
                kept: Some(seat),
            }))?;
        }
        if !self.seats.toggle_zoom(seat) {
            return Ok(());
        }
        self.apply_window_min_inner_size()?;
        self.commit_seat_geometry()?;
        // The posture is not on disk (§7.1.6l) and nothing else about this tab
        // moved, so there is deliberately no `mark_session_dirty` here: a zoom
        // is not a change to the layout the session remembers.
        Ok(())
    }

    /// **`New terminal in folder…`** — ask the system for a folder and split
    /// into it.
    ///
    /// The chooser only gets *queued* here, for [`Runtime::browse_for_root`]'s
    /// reason at length: `IFileDialog::Show` runs a nested message loop that
    /// would re-enter this window's own event handling underneath the `&mut`
    /// borrow that started it. The answer arrives in
    /// [`Runtime::apply_folder_pick_result`].
    ///
    /// **The menu is already gone** — `run_pane_menu_row` took it before this
    /// runs — and that is the ruling's own requirement ("while the modal dialog
    /// is up the menu is closed"). A popup left standing behind a system modal
    /// is a popup nothing can dismiss: it takes no input while the dialog owns
    /// the loop, and the click that dismisses the dialog is spent on the dialog.
    ///
    /// The chooser opens where the pane is standing (`LeafSession::folder`: its
    /// shell's last report, else its shell process's folder, else the folder it
    /// was opened in) — the same courtesy `browse_for_root` shows a column, and
    /// the same one I88 asks of every arriving shell.
    fn browse_for_split_root(&mut self, seat: SeatId) {
        let start = self.sessions.get(&seat).and_then(|leaf| {
            leaf.folder(&crate::pane_folder::SystemProcessFolders)
                .into_folder()
        });
        match self.window.folder_picker.request(start.as_deref()) {
            Ok(true) => self.window.folder_pick = Some(FolderPick::SplitInto(seat)),
            // Already queued or already open: a second request while one is up
            // is one dialog, not two, and whoever asked first keeps the answer.
            Ok(false) => {}
            Err(error) => crate::diagnostics::stderr_line(format!(
                "recoverable folder chooser failure: {error}"
            )),
        }
    }

    /// **`Move pane to new window` — the pane leaves this window** (multiwindow
    /// slice F1c; `plan.md` F1c, `DESIGN.md` §2.10).
    ///
    /// **Two doors, one journey, and the journey is composed rather than
    /// copied.** The plan's own sentence is 「pane 拖出窗界 = 先按 `StripExtract`
    /// 升格成 tab 再成窗,一条路」, and this is that sentence in two lines: the
    /// pane becomes a tab through [`Self::extract_pane_into_new_tab`] — the same
    /// function the row above and the drag onto the strip both spend — and the
    /// tab then moves through [`FolioApp::transfer_tab`], which is the whole of
    /// F1b's transaction and the only thing in this program that moves a tab
    /// between windows. Nothing here re-implements either half, which is why a
    /// tear-out can never quietly become a respawn.
    ///
    /// **A lone pane skips the first half rather than failing it.** `close_seat`
    /// will not empty a tree, so the tear-out answers `None` for a pane with no
    /// siblings — and that pane is already the whole tab, so the tab that moves
    /// is the one it is standing in. This is the row §7.1.6i said would catch
    /// the debt `Move pane to new tab` leaves on a lone pane.
    ///
    /// **The window has to exist before the tab can move into it**, and only the
    /// loop's own door may create one, so the errand is written down here and
    /// spent in [`FolioApp::open_pending_window`] — in the same turn, before any
    /// frame is drawn. [`App::pending_new_windows`]'s standing shape and its
    /// standing reason.
    fn move_pane_to_new_window(&mut self, seat: SeatId) -> Result<()> {
        let standing = self.window.tabs[self.window.active_tab].id;
        let slot = self.window.tabs.len();
        let leaf = LeafId {
            tab: standing,
            seat,
        };
        let promoted = self.extract_pane_into_new_tab(leaf, slot)?;
        let errand = TearOut {
            tab: promoted.unwrap_or(standing),
            from: self.window_id(),
            promoted: promoted.is_some(),
            // A menu row names no place — see [`TearOut::at`].
            at: None,
        };
        let like = self.window_id();
        self.app
            .pending_new_windows
            .push(NewWindowPlan::receiving(like, errand));
        Ok(())
    }

    /// One float's chassis, laid out where that window stands this frame.
    ///
    /// The rise is [`risen_frame`]'s, shared with the hit test so the window is
    /// asked about exactly where it is drawn.
    pub(in crate::runtime) fn float_geometry_of(
        &mut self,
        id: float::FloatId,
    ) -> Option<(float::FloatGeometry, float::FloatFade)> {
        let now = Instant::now();
        let scale = self.window.renderer.scale_factor() as f32;
        let tools = self.float_head_tools(id);
        let (mode, frame, fade) = {
            let win = self.window.float.drawn().find(|win| win.epoch == id)?;
            (win.mode, win.frame, self.float_fade_of(win, now, scale))
        };
        let dock_label = self.float_dock_label_width(scale);
        Some((
            float::float_geometry(risen_frame(frame, fade), mode, scale, dock_label, tools),
            fade,
        ))
    }

    /// How wide the `DOCK` caption is — only the font knows, so it is measured
    /// beside the renderer exactly as the tip's and the badge's are.
    ///
    /// **Measured wearing what it is drawn wearing.** `float::build` sets this
    /// label semibold and tracked at `FLOAT_HEAD_TRACKING_EM`, per
    /// `.float-win .fly-head button { font-weight: 600; letter-spacing: .04em }`
    /// (mock-up 720-725). Measuring it as plain regular-weight text made the
    /// button's box a couple of pixels too narrow for its own caption, and the
    /// label's rect *is* that box less its padding — so the `K` was clipped
    /// against the bounds the shortfall had drawn.
    pub(in crate::runtime) fn float_dock_label_width(&mut self, scale: f32) -> f32 {
        let font = float::FLOAT_DOCK_FONT_LOGICAL_PX * scale;
        self.window.renderer.measure_chrome_label(
            &mut self.app.gpu,
            float_dock_label(),
            font,
            bt_render::ChromeLabelWeight::SemiBold,
            float::FLOAT_HEAD_TRACKING_EM,
            false,
        )
    }

    /// `DOCK` — the float becomes a column in the tab you are **looking at**.
    ///
    /// Not the tab it was born in (§7.1.2, user ruling 2026-07-17): a pinned
    /// window floats across tab switches precisely so it can serve you wherever
    /// you are, and sending it home on Dock would be the app deciding for you
    /// where you meant to put it. It also erases the "the tab it came from has
    /// since died" special case, which is the sort of thing a rule earns its
    /// keep by deleting.
    /// **Only the window whose button was pressed is collected**, which is rule
    /// ⑤ of the 2026-08-12 ruling at its plainest: DOCK is one window's control,
    /// and the others stay exactly where they are.
    pub(in crate::runtime) fn dock_float(&mut self, id: float::FloatId) -> Result<()> {
        let Some(win) = self.window.float.wipe(id) else {
            return Ok(());
        };
        self.forget_dead_float_gestures();
        // The window is gone, so whatever content plane it carried goes with it.
        self.sweep_preview_panes();
        // Same reason as dismissal: the window this shape belonged to is gone,
        // and Dock can be reached without the pointer moving afterwards.
        self.apply_pointer_cursor();
        // Docking a *tree* seats a files column. A buffer float's own DOCK lands
        // a preview pane instead, and that door is not open yet.
        let float::FloatTenant::Files(tenant) = win.tenant else {
            return Ok(());
        };
        let metrics = self.seat_metrics();
        let Some(seat) = self.seats.add_files_pane(&metrics, Some(tenant.width)) else {
            return Ok(());
        };
        let active = self.window.active_tab;
        self.window.tabs[active].files.insert(seat, tenant.files);
        self.window.tabs[active]
            .file_trees
            .insert(seat, tenant.cache);
        // **And the repository, the same way and for the same reason** (user
        // ruling, 2026-08-19). `DOCK` is the reverse of the pop-out, so a window
        // that was on its Git page becomes a column on its Git page — with what
        // it had already read, so docking spends no subprocess and the page does
        // not blink back through "Reading the repository…" on the way in.
        self.window.tabs[active].git_trees.insert(seat, tenant.git);
        self.window.tabs[active]
            .git_scroll
            .insert(seat, tenant.git_scroll);
        // `DOCK` is a button and this is the click on it.
        self.set_files_keyboard(Some(seat), FilesFocusArrival::Pointer);
        // **The hole this used to be** (user report, 2026-08-13). A column
        // arriving by this door narrows every pane beside it exactly as one
        // arriving by `Ctrl+Shift+B` does — but this path re-solved the layout
        // and stopped there, and `resolve_seat_layout` moves rectangles without
        // telling a single shell about them. The panes were redrawn narrower
        // while their ConPTYs went on writing at the old width, which is a
        // prompt that reprints itself over its own middle. The keyboard twin
        // never had the bug because it always ran the full ceremony; this one
        // now runs the same one.
        self.settle_seat_set_change()?;
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// **The page a docking float was carrying, moved onto the pane it landed
    /// on** (§7.14a) — [`Self::dock_preview_float`]'s one caller.
    ///
    /// A separate function because it is a *transaction on the window's page
    /// map*, and the dock around it is a transaction on the tab's panes: the two
    /// have different failure modes and the failure of this one must not undo
    /// that one. A page that could not be moved is reported and left where it
    /// is; the next frame finds it under a float that is gone, gives it no
    /// rectangle, and [`Self::advance_web_page`] retires it. A pane with a
    /// document under it and no browser is a pane; a pane with a browser
    /// nothing can address is a leak.
    pub(in crate::runtime) fn dock_the_page_of(
        &mut self,
        id: float::FloatId,
        landing: PreviewSurface,
    ) {
        let Some(carried) = self
            .window
            .float
            .live(id)
            .and_then(float::FloatWin::preview)
            .and_then(|preview| preview.page)
        else {
            return;
        };
        let PreviewSurface::Seat(landed) = landing else {
            return;
        };
        let address = match self.window.window.page_address(bt_platform::PageVisual {
            tab: landed.tab.0,
            seat: landed.seat.0,
        }) {
            Ok(address) => address,
            Err(error) => {
                crate::diagnostics::stderr_line(format!("BT_WEB {error}"));
                return;
            }
        };
        // **A seat that already holds a page keeps it.** `preview_landing_surface`
        // picks a preview seat, and a preview seat can itself be a page; moving
        // this one on top would drop a live browser without going through the
        // door that waits for its process to exit and gives its profile folder
        // back. So the carried page stays where it is and the next frame retires
        // it the ordinary way — it has neither a float nor a seat any more —
        // which is one page lost and not two.
        if self.window.web.contains_key(&landed) {
            crate::diagnostics::stderr_line(format!(
                "BT_WEB the docked page had nowhere to land: {landed:?} is already showing one"
            ));
            return;
        }
        let mut outcomes = Vec::new();
        let report = {
            let window = &mut *self.window;
            let Some(web) = window.web.remove(&carried) else {
                return;
            };
            // Re-keyed under the leaf it is landing on before anything is asked
            // of it, so that every later reader — the placement, the retirement,
            // the keyboard — finds it under one name. `rehost` is given the same
            // compositor twice on purpose: this is a move inside one window, and
            // the visual it is being pointed at is the landing pane's.
            let entry = window.web.entry(landed).or_insert(web);
            entry.rehost(
                &window.compositor,
                &window.compositor,
                address,
                true,
                &mut outcomes,
            )
        };
        if let Some(error) = report.error() {
            crate::diagnostics::stderr_line(format!(
                "BT_WEB the docked page could not follow its pane: {error}"
            ));
        }
        if let Err(error) = self.apply_web_outcomes(landed, outcomes) {
            crate::diagnostics::stderr_line(format!("BT_WEB {error}"));
        }
    }

    /// **A pane's picture moved, so the card that is a picture of it owes a
    /// frame** (§7.1.6b′ T-5, review row R5-7).
    ///
    /// One function because there are two roads and they must not disagree. The
    /// ordinary one is the drain: bytes reached a screen, which is the same
    /// condition `DualPlaneSession::feed_at` bumps `screen_revision` on — the very
    /// number [`focus_thumb::FocusThumbnails`]'s damage gate keys a terminal seat
    /// to. The other passes through no ring at all: a DEC 2026 block whose
    /// timeout expires is released turns after its bytes arrived, moving cells
    /// with nothing coming through the drain, and the card column was never told.
    ///
    /// **The damage key is checked, not the projection**, and it is checked
    /// against the cards that are actually on screen: a collapsed column, a
    /// window not in the mode, or a tab scrolled out of the rail leaves this at
    /// one rectangle comparison per speaking tab and no frame asked for.
    pub(crate) fn panes_spoke(&mut self, spoke: &[usize], now: Instant) {
        if spoke.is_empty() {
            return;
        }
        if let Some(geometry) = self.focus_rail_geometry_now(now)
            && spoke.iter().any(|index| geometry.card_is_in_view(*index))
        {
            self.window.cards.pane_spoke();
        }
    }

    pub(in crate::runtime) fn drawn_rail(&self, now: Instant) -> (i32, u8) {
        let scale = self.window.renderer.scale_factor() as f32;
        let state = self.sampled_rail(now);
        (
            (state.width_logical_px() * scale).round() as i32,
            (state.text_opacity * 255.0).round() as u8,
        )
    }

    /// The dock box's opacity as the overlay would draw it, quantised to the
    /// 1/255 a layer's alpha resolves to — `None` when there is no box.
    pub(in crate::runtime) fn drawn_dock_reveal(&self, now: Instant, motion: Motion) -> Option<u8> {
        self.window.drop_preview.as_ref().map(|shown| {
            let (reveal, _) = shown.reveal.sample(now, motion);
            (reveal.clamp(0.0, 1.0) * 255.0).round() as u8
        })
    }

    /// **Which of this tab's preview seats wear a play button** (user ruling
    /// 2026-08-27; §7.23 ⑩).
    ///
    /// Three facts, settled here so that the painter and the hit test are handed
    /// one answer rather than each deriving a policy
    /// ([`Self::play_button_rasters`] draws by it and
    /// [`seats::hit_preview_play`] presses by it):
    ///
    /// 1. **The pane is showing a video whose spelling can be played** —
    ///    [`preview::path_names_a_video`], which is the playable column
    ///    of the one table. A `.mov` has a face and no player, and what it wears
    ///    instead is a sentence in its fact line, not a button that would refuse.
    /// 2. **Nothing is already on that seat.** A pane that is playing has an
    ///    engine on it, and a play button over a playing video would be a
    ///    control offering what is already happening.
    /// 3. **It is a seat and not a float**, because the verb needs a leaf to put
    ///    an engine on and `pop_out_preview` closes the leaf it tore off.
    ///
    /// **The decoded frame is deliberately not one of them.** A `.webm` carrying
    /// AV1 plays in that engine on a machine whose Media Foundation cannot draw
    /// its first frame (§7.23 ②), and a button gated on the picture would be
    /// missing on exactly the files that most need it — the ones whose pane has
    /// nothing to show until it plays.
    pub(crate) fn seats_wearing_a_play_button(&self) -> std::collections::BTreeSet<SeatId> {
        let tab = match self.window.tabs.iter().find(|tab| tab.id == self.id) {
            Some(tab) => tab,
            None => return std::collections::BTreeSet::new(),
        };
        tab.seats
            .preview_seats()
            .into_iter()
            .filter(|seat| {
                let leaf = LeafId {
                    tab: self.id,
                    seat: *seat,
                };
                // **A seat that is playing wears a pause on its bar, not a play
                // on its picture** (route B slice ②; §7.44 ①).
                //
                // The `is_closing` clause this replaces was route A's whole
                // problem in one line: a browser left the glass when `close` was
                // called and its map entry survived for as long as the process
                // took to end, so a stopped video had no button for most of a
                // second and sometimes for ten. A seat is removed from the map
                // the instant it is stopped — the shutdown is synchronous — so
                // the button comes back in the same frame the picture does, and
                // there is no second state to ask about.
                if self.window.video.get(PreviewSurface::Seat(leaf)).is_some() {
                    return false;
                }
                tab.preview_panes
                    .get(PreviewSurface::Seat(leaf))
                    .and_then(|pane: &crate::PreviewPane| pane.image())
                    .is_some_and(|image| preview::path_names_a_video(&image.path))
            })
            .collect()
    }

    /// Carry the current solve to every shell of the active tab — one rectangle
    /// per leaf, read from one expression.
    ///
    /// **The geometry is the same sentence for all of them**: a leaf's grid is
    /// [`seats::pane_body_viewport`] of *that leaf's* seat, and of nothing else.
    /// **So is the timing.** Every leaf goes through
    /// [`schedule_leaf_grid_change`]: its actor reflows in this turn, and its
    /// child hears the last word of the gesture at the shared 200 ms quiet
    /// boundary. The siblings used to be told at once instead — "they have no
    /// drag to coalesce" was never true of them, only unnoticed, because the
    /// drag being coalesced is the *window's* and it moves every pane's
    /// rectangle at once. A four-pane split therefore made three uncoalesced
    /// `ResizePseudoConsole` round trips on the window thread per OS `Resized`,
    /// each one re-entering conhost and invalidating a PSReadLine anchor.
    ///
    /// The focused leaf used to be sized from [`Self::resolve_seat_layout`]'s
    /// return instead, which is the body of `seats.identity()` — the tab's
    /// *primary* seat. That is a stored field naming one fixed leaf; it moves
    /// only when the leaf it names is closed, and never with focus. In a
    /// lone-terminal tab it is the focused leaf and the mistake was invisible,
    /// which is why it survived U12-B. Split the tab and focus the other pane
    /// and the focused shell was told the primary pane's column count: a narrow
    /// pane stopped wrapping because its shell believed it was wide, and
    /// PSReadLine — whose anchor arithmetic trusts the reported buffer width —
    /// raised "the value must be greater than or equal to zero and less than the
    /// console's buffer size". Taking focus away appeared to cure it because the
    /// unfocused carry below was already reading the right rectangle.
    ///
    /// Without the unfocused half a split pane keeps the width it was born with:
    /// the window resizes, the divider moves, and one pane's shell goes on
    /// believing it has the columns it had at spawn.
    /// Answers the grid the focused leaf's own rectangle solved to — what the
    /// perf trace means by "the size this resize frame was about" — or `None`
    /// when the solver placed no rectangle for it.
    pub(in crate::runtime) fn resize_leaves_to_layout(
        &mut self,
        observed_at: Instant,
        context: &'static str,
    ) -> Result<Option<GridSize>> {
        // **No pane is handed a grid cut from the rectangle the window is
        // leaving** (T-CARD-ANCHOR-DPI; see [`DpiRectangle`], which holds the
        // argument). The gate is here rather than at the two DPI call sites
        // because the sentence is about the rectangle and not about who is
        // asking: a tab activated, a divider let go or a seat layout settled
        // inside the same interval would cut the same phantom grid out of the
        // same pixels.
        //
        // Nothing is queued by the refusal. The rectangle this window ends the
        // DPI change with is re-solved either by the `Resized` that follows or
        // by [`Self::settle_dpi_rectangle`] on the next turn, and both of those
        // re-derive every leaf's grid from the tree as it stands.
        if !self.window.dpi_rectangle.may_cut_a_grid() {
            return Ok(None);
        }
        let scale = self.window.renderer.scale_factor() as f32;
        // **The shells are about to be told what the tree looks like**, which
        // makes this the one honest place to record that they know. See
        // [`WindowRuntime::shells_settled_revision`]: every other reading of "the seat
        // set changed" is a reading of intent, and intent is exactly what the
        // two verbs that forgot the ceremony had.
        self.window.shells_settled_revision = self.seats.structure_revision();
        let plan = leaf_resize_plan(&self.seats, &self.seat_layout, self.focused_leaf, scale);
        let active = self.window.active_tab;
        // Read before the walk takes a `&mut` of this tab's sessions
        // (`BT_CARD_TRACE`).
        let window = u64::from(self.window.window.id());
        let tab_id = self.window.tabs[active].id;
        // The panes without the keyboard, before the focused one, so the pane a
        // gesture is aimed at is the last one this turn touches.
        for target in plan.iter().copied().filter(|target| !target.focused) {
            let (seat, body) = (target.seat, target.body);
            let physical = PhysicalSize::new(body.width, body.height);
            let Some(leaf) = self.window.tabs[active].sessions.get_mut(&seat) else {
                continue;
            };
            // This leaf's own metrics (ticket 37), through `LeafSession::grid_for`.
            let next_grid = leaf.grid_for(body);
            schedule_leaf_grid_change(
                leaf,
                next_grid,
                physical,
                observed_at,
                LeafOnStage::Shown,
                context,
                card_trace::Pane {
                    window,
                    tab: tab_id,
                    seat,
                },
            )?;
        }
        // And the tabs nobody is looking at, before the focused pane for the
        // reason its siblings go before it.
        self.resize_hidden_leaves_to_layout(observed_at, context)?;
        // A seat the solver could not place has no rectangle, so there is no
        // size to carry and the leaf keeps the one it has until a later solve
        // places it. Substituting some other rectangle here — the window's, the
        // primary seat's — is precisely the defect this method was rewritten to
        // end.
        let Some(target) = plan.iter().copied().find(|target| target.focused) else {
            return Ok(None);
        };
        let body = target.body;
        // A tab with no shell has no mark and so no rail; its grid is only ever
        // traced, and [`Self::schedule_grid_change`] carries it nowhere (§7.1.6h).
        //
        // At the focused pane's own metrics (ticket 37): `grid_for` subtracts the rail's
        // reservation at window scale first and divides by this pane's cell after, which is
        // `cmdrail::terminal_grid_for`'s own order. A tab with no shell has no pane metrics
        // either, and its grid is traced at the window's base size.
        let next_grid = match self.window.tabs[active].focused() {
            Some(leaf) => leaf.grid_for(body),
            None => cmdrail::terminal_grid_for(&self.window.renderer.base_metrics(), body, false),
        };
        self.schedule_grid_change(
            next_grid,
            PhysicalSize::new(body.width, body.height),
            observed_at,
            context,
        )?;
        Ok(Some(next_grid))
    }

    /// **Every tab's panes, because a window's rectangle is a fact about the
    /// window** (user report 2026-09-09).
    ///
    /// [`Self::resize_leaves_to_layout`] is the tab on the stage; this is every
    /// other one, and it is here for the reason [`Self::apply_scale_factor`] and
    /// [`Self::sync_math_layout_key`] already walk every leaf of every tab. A
    /// DPI, a face and the rectangle this window has are facts about the
    /// *window*; not one of them is about the pane holding the keyboard, and a
    /// tab that answered them only when somebody looked at it would be a tab
    /// whose shells believe whatever was true the last time it was on stage.
    ///
    /// **What that cost.** A session saved `maximized: true` records the window's
    /// *normal* rectangle beside the flag, so the window opens at that rectangle,
    /// every restored tab is built against it, and `put_the_window_on_the_glass`
    /// asks Windows to maximize only after every shell is already running. The
    /// tab on the stage was put right by the `Resized` that followed; the tabs
    /// behind it were not. On the report's machine 960 logical pixels less the
    /// focus column is too narrow for three panes, so one of them was born a bar
    /// — and stayed one, through the whole of its shell's first prompt, until the
    /// tab was clicked.
    ///
    /// Each tab is solved into [`WindowRuntime::seat_viewport`], the very
    /// rectangle the stage was solved into and stored rather than recomputed for
    /// A12's reason, so "every tab is solved into the same box"
    /// ([`TabState::seat_layout`]) is true of the box as well as of the sentence.
    /// The solve is pure and `O(seats)` and each leaf's notification is coalesced
    /// on its own queue, so a window with a dozen tabs pays a dozen tree walks
    /// per gesture and one `ResizePseudoConsole` per pane per gesture.
    ///
    /// **And one reflow per pane per gesture, which is the half this arrived
    /// without** (user report 2026-09-09). A tree walk is pure and cheap; the
    /// `resize_at` at the end of [`schedule_leaf_grid_change`] is a full vendor
    /// reflow of that pane's grid on the window thread, and taking it per hidden
    /// pane per OS event put the frame the visible pane owes the glass behind
    /// eleven of them on a twelve-tab window. So every leaf here is `Behind`:
    /// the notification and the reflow are both released at the quiet boundary,
    /// which is the same sentence the child was already being told and the same
    /// one [`commit_leaf_resize`] was already written to say. Nothing about
    /// *which* size a hidden tab ends at changes — only when its own actor hears
    /// it, and no frame is drawn from a hidden tab in between.
    fn resize_hidden_leaves_to_layout(
        &mut self,
        observed_at: Instant,
        context: &'static str,
    ) -> Result<()> {
        // Read before any tab is borrowed mutably (`BT_CARD_TRACE`).
        let window = u64::from(self.window.window.id());
        let scale = self.window.renderer.scale_factor() as f32;
        let metrics = seats::seat_metrics(self.window.renderer.dpi_milli().get());
        let viewport = self.window.seat_viewport;
        let policy = self.window.size_policy;
        let active = self.window.active_tab;
        for index in 0..self.window.tabs.len() {
            if index == active {
                continue;
            }
            let tab = &self.window.tabs[index];
            let (layout, overflow) = solve_tree(&tab.seats, viewport, &metrics, policy);
            let sized: Vec<(SeatId, GridSize, PhysicalSize<u32>)> =
                leaf_resize_plan(&tab.seats, &layout, tab.focused_leaf, scale)
                    .into_iter()
                    .filter_map(|target| {
                        let leaf = tab.sessions.get(&target.seat)?;
                        Some((
                            target.seat,
                            leaf.grid_for(target.body),
                            PhysicalSize::new(target.body.width, target.body.height),
                        ))
                    })
                    .collect();
            let tab = &mut self.window.tabs[index];
            tab.seat_layout = layout;
            tab.seat_overflow = overflow;
            let tab_id = tab.id;
            for (seat, next_grid, physical) in sized {
                let Some(leaf) = tab.sessions.get_mut(&seat) else {
                    continue;
                };
                schedule_leaf_grid_change(
                    leaf,
                    next_grid,
                    physical,
                    observed_at,
                    LeafOnStage::Behind,
                    context,
                    card_trace::Pane {
                        window,
                        tab: tab_id,
                        seat,
                    },
                )?;
            }
        }
        Ok(())
    }

    /// The pane the pointer is standing in, and the cell of it under the pointer.
    ///
    /// One lookup for both halves, for the reason [`Self::forwarded_mouse_hit`]
    /// records below: a hit and the pane it belongs to that were fetched
    /// separately can disagree about which pane they mean.
    pub(in crate::runtime) fn pane_frame_hit(&self) -> Option<(SeatId, bt_render::GridHit)> {
        let (seat, position, frame) = self.pane_hit_context()?;
        let hit = self
            .pane_frame_metrics(seat)?
            .hit_test_frame(frame, position.x, position.y)?;
        Some((seat, hit))
    }

    /// The cell under the pointer as the *mouse protocol* names it: the same hit
    /// [`Self::frame_hit`] answers, mapped through the live viewport of the frame it was read
    /// from.
    ///
    /// One lookup, deliberately. The translation used to be a second one — the hit taken from the
    /// pane under the pointer, the frame taken from `WindowRuntime::last_presented_frame` — and two
    /// lookups can disagree about which pane they mean, or about whether there is a frame at all.
    /// Both happen on one gesture: `focus_pane_at` empties that window-level slot the instant
    /// keyboard focus moves, and the press that moved it is still on its way down the router, so
    /// the very next line went looking for a frame that had just been taken away. Reading the
    /// frame beside the hit removes the disagreement by construction rather than by testing for
    /// it — there is no second slot left to be empty.
    /// The frame one pane last drew — the cells a gesture in that pane is made of,
    /// and the anchors a selection set on that pane's session must be read from.
    ///
    /// Asked by seat rather than of the focused leaf. The two are the same pane
    /// for the length of a press, because D40 moves the focus into the pane on the
    /// way down; but "the same by construction" and "the same because of the order
    /// two other things happen in" are different guarantees, and only the first one
    /// survives someone reordering the router.
    ///
    /// The leaf's own copy, never `WindowRuntime::last_presented_frame`. That window-level mirror is
    /// presentation bookkeeping (the projection hold, the unchanged-frame skip) and `focus_pane_at`
    /// empties it the instant keyboard focus moves — while the press that moved it is still on its
    /// way down the router to the very lines that want a frame. `None` means this pane has not
    /// drawn yet, which is the same "there are no cells here" [`Self::pane_hit_context`] already
    /// answers, and the callers already do nothing about.
    pub(crate) fn pane_frame(&self, seat: SeatId) -> Option<&ViewportFrame> {
        self.sessions.get(&seat)?.last_presented_frame.as_ref()
    }

    /// **The terminal panes of the tab on screen that are not at 100 %, with the percentage
    /// each asked for** (ticket 37) — read off each pane's own rung whenever it is asked, for the
    /// painter and the hit test alike, and never kept.
    pub(crate) fn pane_text_sizes(&self) -> std::collections::BTreeMap<SeatId, u16> {
        self.sessions
            .iter()
            .filter(|(_, leaf)| !leaf.text_scale.is_actual())
            .map(|(seat, leaf)| (*seat, leaf.text_scale.percent()))
            .collect()
    }

    /// **The metrics [`Self::pane_frame`]'s picture was drawn at** (ticket 37) — the one answer
    /// every pointer question about `seat` measures cells with.
    ///
    /// The presented picture's, never the pane's newest: a size step whose picture has not yet
    /// reached the glass has changed [`LeafSession::metrics`] and not the rows under the
    /// pointer, and a hit that paired the old row map with the new cell width would name a cell
    /// nobody can see.
    pub(crate) fn pane_frame_metrics(&self, seat: SeatId) -> Option<bt_render::CellMetrics> {
        Some(self.sessions.get(&seat)?.presented_metrics)
    }

    /// **Which spelling of an absolute path the shell in `seat` prints** — the pane's namespace,
    /// read from the session that was told it at spawn.
    ///
    /// It is what [`bt_transcript::paths::PathNamer::Pane`] carries, and therefore what decides
    /// whether a `\\wsl.localhost\<distro>\…` target printed into *this* pane names a place this
    /// window may read (route D of the untrusted-path audit, 2026-09-08). A seat with no session
    /// behind it — a folder tab, a file tab — prints nothing at all, and this machine's own
    /// spelling (the namespace's default: `Windows` on Windows, `Posix` elsewhere) is the honest
    /// answer for a pane that has no shell to speak another namespace.
    pub(crate) fn seat_path_namespace(
        &self,
        seat: SeatId,
    ) -> bt_transcript::paths::PrintedPathNamespace {
        self.sessions
            .get(&seat)
            .map(|leaf| leaf.session.printed_path_namespace())
            .unwrap_or_default()
    }

    /// [`Self::seat_path_namespace`] for the pane the pointer is standing in.
    ///
    /// The hover's own pane and not the focused one, because a hyperlink hover belongs to whatever
    /// pane the pointer is over ([`Self::hover_pane`]'s whole reason for existing), and the link
    /// under it was printed by *that* pane's shell.
    pub(in crate::runtime) fn hovered_pane_path_namespace(
        &self,
    ) -> bt_transcript::paths::PrintedPathNamespace {
        self.window
            .hover_pane
            .map(|seat| self.seat_path_namespace(seat))
            .unwrap_or_default()
    }

    /// **What this window is allowed to know about a path `seat`'s shell printed** — the ledger
    /// read that stands where a filesystem call used to (audit 3 C-2).
    ///
    /// The whole of the rule is in the body: a `get` on a `BTreeMap` the *worker* filled. The
    /// window thread has no other way to ask, and a pane with no session has no ledger, which is
    /// the honest `None` — a folder tab prints nothing and answers for nothing.
    pub(crate) fn seat_path_verdict(
        &self,
        seat: SeatId,
        path: &Path,
    ) -> Option<bt_term::PathVerdict> {
        self.sessions.get(&seat)?.session.path_verdict(path)
    }

    /// [`Self::seat_path_verdict`] for the pane the pointer is standing in, for
    /// [`Self::hovered_pane_path_namespace`]'s reason: the link under the pointer was printed by
    /// *that* pane's shell, so it is that pane's ledger that answers for it.
    pub(in crate::runtime) fn hovered_pane_path_verdict(
        &self,
        path: &Path,
    ) -> Option<bt_term::PathVerdict> {
        self.seat_path_verdict(self.window.hover_pane?, path)
    }

    /// The cell a selection drag that began in `seat` is over, with the pointer
    /// clamped into that pane's own body.
    ///
    /// Once a drag has begun the pointer is free to wander — into the pane next
    /// door, onto the chrome, past the window's edge — and every one of those
    /// points still has to name a cell of the *origin* pane, because that is the
    /// grid whose anchors this selection is made of. Reading the cell from
    /// whatever pane the pointer happens to be over would apply a neighbour's row
    /// and column to this pane's frame: a selection nobody asked for, at a place
    /// nobody pointed at, in a pane the gesture never belonged to.
    ///
    /// Clamping rather than refusing is also the terminal convention. Dragging
    /// below a pane selects to the end of what is there; it does not stop
    /// selecting at the edge and it does not reach into the pane below.
    pub(in crate::runtime) fn drag_hit_in_pane(&self, seat: SeatId) -> Option<bt_render::GridHit> {
        let position = self.window.pointer_position?;
        let frame = self.pane_frame(seat)?;
        let scale = self.window.renderer.scale_factor() as f32;
        let body = seats::pane_body_viewport(&self.seats, &self.seat_layout, seat, scale)?;
        let (x, y) = clamp_into_body(body, position.x, position.y);
        self.pane_frame_metrics(seat)?
            .clamped_hit_test_frame(frame, x, y)
    }

    /// Set one pane's selection: the pane the gesture belongs to, named outright.
    ///
    /// Never through the focused-leaf deref, which answers "whichever pane holds
    /// the keyboard *now*" — the right pane during a drag only because nothing
    /// moves focus while a button is held.
    /// Hand a selection to one pane of the tab on screen.
    ///
    /// The window's half of [`TabState::set_leaf_selection`], which is where the
    /// "one selection to a tab" rule lives. Every route that gives a pane a
    /// selection — the drag, the double click, the triple click — comes through
    /// here, so it is the one door that rule has to be written on.
    pub(in crate::runtime) fn set_pane_view_selection(
        &mut self,
        seat: SeatId,
        selection: Option<ViewSelection>,
    ) {
        let active = self.window.active_tab;
        self.window.tabs[active].set_leaf_selection(seat, selection);
    }

    /// The pane the pointer is standing in, that pane's own shell, and the cell under the pointer.
    ///
    /// The subject of every hover verb below, and deliberately never the focused leaf. A pointer
    /// answers about what it is pointing at; a hover that first required a click would be the
    /// window refusing to say what it can already see (user ruling 2026-08-10). Nothing reached
    /// from here writes focus either — reading is not taking.
    pub(in crate::runtime) fn hovered_leaf(
        &self,
    ) -> Option<(SeatId, &LeafSession, bt_render::GridHit)> {
        let (seat, position, frame) = self.pane_hit_context()?;
        let hit = self
            .pane_frame_metrics(seat)?
            .hit_test_frame(frame, position.x, position.y)?;
        Some((seat, self.sessions.get(&seat)?, hit))
    }

    /// Notice the pointer crossing from one pane into another, and pay what the crossing owes.
    ///
    /// Returns the pane the pointer is now in. The pane it entered owes a scan of the frame it
    /// already drew — nothing has published there, so nothing has scanned it, and a hover that
    /// waited for that pane's next PTY byte to answer would answer late or never on a quiet shell.
    /// Both panes owe a repaint, which is how the marks move across with the pointer.
    pub(in crate::runtime) fn observe_hovered_pane(&mut self) -> Result<Option<SeatId>> {
        let hovered = self.pane_hit_context().map(|(seat, _, _)| seat);
        if self.window.hover_pane == hovered {
            return Ok(hovered);
        }
        self.window.hover_pane = hovered;
        if let Some(seat) = hovered {
            self.rescan_pane_references(seat);
        }
        self.repaint_hovered_pane()?;
        Ok(hovered)
    }

    /// Re-derive one pane's image references from the frame that pane last drew.
    ///
    /// Idempotent and cheap to repeat: it reads a frame that is already in hand and asks that
    /// pane's own shell what it draws, which is the same question `publish_frame_inner` asks for
    /// the focused pane on every frame.
    pub(in crate::runtime) fn rescan_pane_references(&mut self, seat: SeatId) {
        let active = self.window.active_tab;
        let Some(leaf) = self.window.tabs[active].sessions.get_mut(&seat) else {
            return;
        };
        let Some(frame) = leaf.last_presented_frame.as_ref() else {
            leaf.frame_image_references = FrameImageReferences::default();
            return;
        };
        leaf.frame_image_references = FrameImageReferences {
            columns: frame.columns.get(),
            references: leaf.session.frame_image_references(frame),
        };
    }

    /// Put the pane under the pointer — and the pane it just left — back on the glass.
    ///
    /// Two doors, because a window with a fleet in it has two ways to redraw a pane. The focused
    /// leaf reaches the glass by publishing a frame, which is the path every existing contract
    /// (presentation hold, scroll anchoring, the unchanged-frame skip) is written against. Every
    /// other pane is re-projected by `redraw` itself, so asking for one is the whole of what it
    /// takes to move its marks. Asking for both is correct and costs one frame: a pointer leaving
    /// the focused pane for a neighbour owes marks to the neighbour and owes their removal to the
    /// pane it left.
    pub(in crate::runtime) fn repaint_hovered_pane(&mut self) -> Result<()> {
        hang_watch::during(hang_watch::Station::WindowRedraw, || {
            self.window.window.request_redraw()
        });
        self.publish_interaction_frame()
    }

    /// Get a change made *to one named pane* onto the glass — including when
    /// that pane is not the one holding the keyboard.
    ///
    /// [`Self::publish_interaction_frame`] builds the **focused** leaf's frame,
    /// and it is allowed to decline: a presentation hold standing on that leaf
    /// returns before it ever reaches `request_redraw`. For the focused pane's
    /// own content that is exactly right — the hold exists to keep the last
    /// complete frame on screen instead of flashing through a resize reprint.
    ///
    /// For **every other pane it is wrong**, and silently so. An unfocused pane
    /// is composed nowhere but inside `redraw`, and `redraw` returns
    /// immediately unless a frame reached the slot, so a hold owned by the
    /// focused leaf freezes panes whose own shells are holding nothing. That is
    /// the shape of the wheel bug: `scroll_by_subpixels` clears the hold on the
    /// leaf it scrolls, so a notch over the focused pane always healed its own
    /// path to the screen while a notch over any other pane moved that pane's
    /// projection and then had no way to draw it.
    ///
    /// The repair is the one this window already uses for a peek overlay
    /// (`present_peek_overlay`): when the focused leaf declines, re-present the
    /// frame **already on screen**. Its pixels are unchanged and stay unchanged
    /// — the hold is honoured to the letter — but putting it back in the slot
    /// runs the compositor, and the compositor rebuilds every other pane from
    /// that pane's own projection. Nothing is invented for the held pane; the
    /// panes that have something new to show simply stop being hostage to it.
    pub(in crate::runtime) fn repaint_pane_change(&mut self, seat: SeatId) -> Result<()> {
        self.repaint_pane_change_inner(seat, None)
    }

    /// A wheel can leave the view at its clamp; other pane changes still owe
    /// their unconditional frame. The focused frame's digest cannot tell us
    /// whether an unfocused view moved, so carry that answer from the scroll.
    pub(in crate::runtime) fn repaint_pane_change_inner(
        &mut self,
        seat: SeatId,
        wheel_view_moved: Option<bool>,
    ) -> Result<()> {
        let trigger = FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        };
        if self.publish_frame_inner(trigger, wheel_view_moved.is_some())?
            || seat == self.focused_leaf
            || wheel_view_moved == Some(false)
        {
            return Ok(());
        }
        self.represent_on_screen_frame(trigger)
    }

    /// Drop one pane's selection — the pane named, and not whichever holds the
    /// keyboard. The two differ for a gesture, which belongs to the pane it began
    /// in for as long as the button is down.
    pub(in crate::runtime) fn clear_pane_selection(&mut self, seat: SeatId) {
        let active = self.window.active_tab;
        self.window.tabs[active].clear_leaf_selection(seat);
    }

    /// Advance a divider drag. Returns whether the pointer was consumed.
    ///
    /// Every frame re-reads the split's slot from the current solve and asks
    /// `bt-layout::apply` for the ratio: the clamp, and the refusal when the
    /// clamp is unsatisfiable, are §2.4's and are not re-derived here. Red line
    /// L9 is upheld by the edit itself — `DragDivider`'s focus set is exactly
    /// that one split, so nothing rebalances mid-gesture.
    pub(in crate::runtime) fn drive_divider_drag(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let Some(drag) = self.held_divider_drag().copied() else {
            return Ok(false);
        };
        let Some(slot) = self
            .seats
            .split_slots(&self.seat_layout)
            .into_iter()
            .find(|slot| slot.id == drag.split)
        else {
            self.drop_divider_drag();
            return Ok(false);
        };
        let along = match drag.dir {
            Axis::Row => position.x,
            Axis::Col => position.y,
        };
        let scale_ppm = seats::scale_ppm(self.window.renderer.dpi_milli().get());
        let metrics = self.seat_metrics();
        // A refusal, and a clamp that changed nothing, both mean "do not
        // re-solve": §2.4 rules that an infeasible drag has zero side effects
        // rather than writing a value the next solve will "correct", which
        // would dress a refusal up as a jitter.
        //
        // The two grips ask the same question of the same slot and differ only
        // in what they write — a proportion or a width — which is exactly the
        // difference §3.4 draws. Both defer their clamp to `bt-layout::apply`,
        // so neither holds an opinion this module could get out of step with.
        let moved = match drag.grip {
            DividerGrip::Ratio(_) => {
                let Some(reserved) = self.seats.split_reserved(&metrics, drag.split) else {
                    return Ok(true);
                };
                let Some((requested, usable)) =
                    seats::requested_ratio(slot, reserved, scale_ppm, along)
                else {
                    return Ok(true);
                };
                self.seats
                    .drag_divider(&metrics, drag.split, requested, usable)
                    == Ok(true)
            }
            DividerGrip::FixedExtent { leading, .. } => {
                let Some((requested, usable)) =
                    seats::requested_fixed_extent(slot, leading, scale_ppm, along)
                else {
                    return Ok(true);
                };
                self.seats
                    .drag_fixed_extent(&metrics, drag.split, requested, usable)
                    == Ok(true)
            }
        };
        if moved {
            // **The second door into sovereignty** (最小值主权, 2026-08-08: "a
            // minimum is law to the program and advice to the user").
            //
            // `Edit::DragDivider` already writes the ratio the hand asked for
            // without consulting a minimum — that half was done the day the
            // ruling landed. What was missing is that the *solve* which turns
            // that ratio into rectangles still ran under `Lawful`, so the
            // concession chain put the layout straight back: measured on a
            // restored 960-logical window holding two preview panes and a
            // terminal, dragging the root divider from 1/3 to 1/2 moved nothing
            // at all — the terminal stayed collapsed at 24px and both previews
            // stayed on their 360 floor. Under `Sovereign` the same ratio gives
            // the terminal 254px and lets the two previews fall to 352 together,
            // which is the ruling's own words: past the floors, the floors give
            // way *in proportion*.
            //
            // Taken here rather than only for the frame the button is down,
            // because "sovereignty, once taken, is returned only by another
            // claim" (`size_authority_for_rectangle`): a layout that snapped
            // back the instant the hand let go would be the refusal arriving one
            // frame late. `claim_lawful_layout` is what hands the minima back
            // their force, and it is the program's own door.
            self.window.size_policy = SizePolicy::Sovereign;
            self.commit_seat_geometry()?;
        }
        Ok(true)
    }

    /// F71/T225: abandon a divider drag and put back the one value it moved.
    ///
    /// "Never mind, and no commit" — the same sentence Esc says to a tab drag,
    /// and it reaches here by the same two doors: the Esc key, and losing the
    /// window, which on Win32 is losing the mouse capture and is this platform's
    /// only `pointercancel` (F72). A drag that ends without either a button-up
    /// or a teardown would keep steering the layout from a pointer nobody is
    /// holding.
    ///
    /// The restore goes back through `Edit::DragDivider` rather than writing the
    /// ratio into the tree directly, and that is the point of it: §2.4's
    /// feasibility judgement and clamp are asked once more, so if the viewport
    /// changed mid-gesture the answer is the *current* legal value nearest the
    /// one we started from, rather than a ratio that was legal for a window that
    /// no longer exists. When nothing changed the clamp is idempotent and the
    /// origin comes back byte for byte.
    ///
    /// Zero side effects when there is nothing to undo: a press that never moved
    /// the ratio restores a value equal to the one already there, `drag_divider`
    /// reports no change, and no re-solve is asked for.
    pub(crate) fn cancel_divider_drag(&mut self) -> Result<bool> {
        let Some(drag) = self.take_divider_drag() else {
            return Ok(false);
        };
        self.put_the_ratio_back(drag)?;
        if let Some(position) = self.window.pointer_position {
            self.update_chrome_hover(position)?;
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// **A divider drag already let go of, put back** — the cancel's own business, for a drag
    /// that is no longer in the slot: [`Self::cancel_divider_drag`]'s, and a capture's ending's
    /// ([`Self::end_capture`]), which runs it before the tab whose seats it reads changes. Each
    /// caller then re-asks the hover the restored layout moved, and repaints the chrome.
    pub(crate) fn put_the_ratio_back(&mut self, drag: DividerDrag) -> Result<()> {
        self.window.seat_pointer.dragging = None;
        let usable = self
            .seats
            .split_slots(&self.seat_layout)
            .into_iter()
            .find(|slot| slot.id == drag.split)
            .map(|slot| slot.slot.extent(slot.dir) - bt_layout::DIVIDER);
        let metrics = self.seat_metrics();
        let restored = match (usable, drag.grip) {
            // The split is gone from the solve, so there is no value of its to
            // put back and nothing to re-solve for.
            (None, _) => false,
            (Some(usable), DividerGrip::Ratio(origin)) => {
                self.seats
                    .drag_divider(&metrics, drag.split, origin, usable)
                    == Ok(true)
            }
            // The pixel half of the same sentence, and it goes back through the
            // same edit for the same reason: a window that changed size under
            // the gesture gets the current legal width nearest the one we
            // started from, rather than a width that fitted a slot that is gone.
            (Some(usable), DividerGrip::FixedExtent { origin, .. }) => {
                self.seats
                    .drag_fixed_extent(&metrics, drag.split, origin, usable)
                    == Ok(true)
            }
        };
        if restored {
            self.commit_seat_geometry()?;
        }
        self.apply_pointer_cursor();
        Ok(())
    }

    /// **The docked chrome's own ladder** — everything this window draws *in*
    /// the layout, smallest target first.
    ///
    /// Reached only through [`Self::pointer_target_at`], which asks the floating
    /// windows first, and private to it for the reason the whole of §7.15 ⑩
    /// exists: this ladder knows nothing about floats, so anything that called
    /// it directly would be reading the chrome *behind* a window as though the
    /// window were not there. It keeps `&self` because that is what it is — a
    /// walk of solved rectangles — and the router above it is the one that needs
    /// a face to measure a caption with.
    pub(in crate::runtime) fn docked_chrome_target_at(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<seats::ChromeTarget> {
        let scale = self.window.renderer.scale_factor() as f32;
        let (width, _) = self.window.renderer.presentation_geometry().swapchain_size;
        let width = width as f32;
        let now = Instant::now();
        let rail = self.sampled_rail(now);
        let pane_transforms = self.pane_transforms(now);
        self.tab_list_target_at(position)
            .or_else(|| {
                seats::hit_window_chrome(
                    width,
                    scale,
                    self.platform_chrome(),
                    rail,
                    self.is_quake_window(),
                    position.x,
                    position.y,
                )
            })
            // Before the pane heads, because the root button lives *inside* one:
            // B17's judgement that a single element can be both a drag handle and a
            // button only holds if the button answers first.
            .or_else(|| {
                seats::hit_files_root(
                    &self.seat_layout,
                    &self.window.files_name_widths,
                    scale,
                    position.x,
                    position.y,
                )
            })
            // And the text-size mark, which lives inside a terminal head for the same
            // reason and answers before it (ticket 37).
            .or_else(|| {
                seats::hit_text_size(
                    &self.seats,
                    &self.seat_layout,
                    &self.pane_text_sizes(),
                    scale,
                    self.window.search.seat(),
                    position.x,
                    position.y,
                )
            })
            // And before them for the same reason: the preview head's four controls
            // live inside the drag handle too, and three of them are dead zones in it
            // (the mock-up's `.pane-close, .pane-files, .pv-tool` exclusion, 5874).
            .or_else(|| {
                // Every preview seat's own tools, so the head the walk is standing on
                // is laid out to the name *it* is drawn with. One set for all of them
                // put the second head's buttons wherever the first head's name
                // happened to end.
                let tools: Vec<(SeatId, seats::PreviewHeadTools)> = self
                    .seats
                    .preview_seats()
                    .into_iter()
                    .map(|seat| (seat, self.preview_head_tools(seat)))
                    .collect();
                // The reveal, on [`seats::hit_chrome`]'s own terms and for its own
                // reason: these five are the split head's run in another head.
                seats::hit_preview_head(
                    &self.seat_layout,
                    scale,
                    &tools,
                    self.head_run(),
                    position.x,
                    position.y,
                )
            })
            // And the row under it (user ruling 2026-08-24), on the same terms: its
            // controls stand inside a band the pane head's drag would otherwise
            // claim, and the band itself answers for what its controls leave over —
            // see [`seats::ChromeTarget::PreviewRail`]. Asked after the head only
            // because the two never overlap; either order gives the same answers,
            // and this one reads in the order the rows are drawn.
            .or_else(|| {
                let rails: Vec<(SeatId, seats::PreviewRailMeasure)> = self
                    .seats
                    .preview_seats()
                    .into_iter()
                    .filter_map(|seat| {
                        Some((seat, self.preview_rail_measure(self.preview_here(seat))?))
                    })
                    .collect();
                seats::hit_preview_rail(&self.seat_layout, scale, &rails, position.x, position.y)
            })
            // The lone pane's corner ghost (§7.1.6i), before the pane heads for the
            // same smallest-target-first reason everything above it is asked first:
            // it floats *over* the terminal's own body, and the body is not chrome,
            // so nothing below would ever return it to the head's arm to be
            // out-ranked.
            .or_else(|| {
                seats::hit_pane_ghost(
                    &self.seats,
                    &self.seat_layout,
                    scale,
                    self.window.search.seat(),
                    position.x,
                    position.y,
                )
            })
            .or_else(|| {
                seats::hit_chrome_in_motion(
                    &self.seats,
                    &self.seat_layout,
                    scale,
                    // **The run that is on screen, not the run the rectangles
                    // allow** (user report, 2026-08-26). A pane head's `×`, `⌄`,
                    // folder and pop-out are drawn only while the hand is inside
                    // that pane, and this hit test used to answer for them whatever
                    // the hand had done — so a press that arrived without a hover
                    // first (a probe's injected click, or the press right after a
                    // resize, which clears `pane_hover` because the border being
                    // dragged is non-client) tore a column out into a floating
                    // window from a head showing nothing but a drag handle.
                    //
                    // It is the *stored* hover and not a fresh `pane_at` of this
                    // very position, which is what makes it the same fact the paint
                    // used: this is asked on the press as well as on the move, and
                    // on the press the only honest question is "what is the reader
                    // looking at".
                    //
                    // And since 裁4 (2026-08-26) it is the *pair*: a head whose own
                    // menu is standing is showing its run even though `pane_hover`
                    // was cleared the instant the hand reached the list, so the
                    // `✕` beside the `⌄` that opened it goes on taking a press.
                    self.head_run(),
                    seats::PaneMotionFrame::new(&pane_transforms),
                    position.x,
                    position.y,
                )
            })
            // Last, and only for what the pane heads left over: a files column's
            // head carries the same `×` and the same drag handle every other pane
            // has, and the tree lives strictly below it. Asking the rows first would
            // put a row where the close button is on any column scrolled far enough.
            // The foot before the rows, and before the rows can even reach it: the
            // tree's body now stops at the strip, so the two cannot overlap. Asked
            // first anyway, for the reason the root button is asked before the head
            // it lives in — a control's claim on its own rectangle should not depend
            // on a *second* rectangle happening to have been shortened correctly.
            .or_else(|| {
                seats::hit_files_foot(
                    &self.seats,
                    &self.seat_layout,
                    scale,
                    position.x,
                    position.y,
                )
            })
            // The card's button, asked only while the card is up: a control that
            // answers a hit test it is not on screen for is an invisible button.
            .or_else(|| {
                let seat = self.seats.preview()?;
                let button = self.window.preview_card_verbs.get(&seat).copied()?;
                seats::hit_preview_card_button(
                    &self.seats,
                    &self.seat_layout,
                    button,
                    scale,
                    position.x,
                    position.y,
                )
            })
            // **The switch before the page it switches**, and before the tree too:
            // it stands between the head and the body, and a thirty-pixel strip the
            // body also claimed would be a control you cannot press.
            .or_else(|| {
                seats::hit_files_seg(
                    &self.seat_layout,
                    &self.files_seg_widths(),
                    scale,
                    position.x,
                    position.y,
                )
            })
            .or_else(|| {
                seats::hit_git_panel(
                    &self.seat_layout,
                    &self.window.git_pages_shown,
                    scale,
                    position.x,
                    position.y,
                )
            })
            // **The play button, in the preview seat's own body** (user ruling
            // 2026-08-27; §7.23 ⑩) — asked here for the graph's own reason one
            // rung down, and asked *before* it because the two cannot both be on
            // one pane and the cheaper question reads first: a set membership
            // against a solved rectangle.
            //
            // Answering here rather than in `chrome_mouse_input`'s fallback ladder
            // is the ruling: this pane's body is a picture, and every rung of that
            // ladder — the body thumb, a link, a block's scrollbar, the picture's
            // own pan — is a gesture *on* the picture. A control standing on it
            // out-ranks all of them, exactly as the graph's toolbar does.
            .or_else(|| {
                seats::hit_preview_play(
                    &self.seats,
                    &self.seat_layout,
                    &self.seats_wearing_a_play_button(),
                    scale,
                    position.x,
                    position.y,
                )
            })
            // The graph, in the preview seat's own body. Asked here rather than
            // among the preview pane's controls because it *is* the body: nothing
            // else claims that rectangle while a graph is on it, and the document
            // underneath has no text to place a caret in.
            .or_else(|| {
                seats::hit_git_graph(
                    &self.seats,
                    &self.seat_layout,
                    // The docked ones. A window's graph is answered by the float
                    // host's own hit test, which runs before this whole chain.
                    // **And this tab's,** which the key now says rather than the
                    // hit test assuming: `git_graphs_shown` is the window's map and
                    // spans every tab, so before §7.12 ⓑ a graph standing on a
                    // background tab offered its rows to a press on the seat of the
                    // same number in front of you.
                    self.window
                        .git_graphs_shown
                        .iter()
                        .filter_map(|(surface, content)| match surface {
                            PreviewSurface::Seat(leaf) if leaf.tab == self.id => {
                                Some((leaf.seat, content))
                            }
                            PreviewSurface::Seat(_)
                            | PreviewSurface::Float(_)
                            | PreviewSurface::Peek => None,
                        }),
                    scale,
                    position.x,
                    position.y,
                )
            })
            .or_else(|| {
                seats::hit_files_tree(
                    &self.seat_layout,
                    &self.files_tree_contents(),
                    scale,
                    self.git_panel_on(),
                    position.x,
                    position.y,
                )
            })
    }

    pub(in crate::runtime) fn update_chrome_hover_target_in_pane(
        &mut self,
        hover: Option<seats::ChromeTarget>,
        pane: Option<bt_layout::SeatId>,
    ) -> Result<()> {
        if self.window.seat_pointer.hover == hover && self.window.seat_pointer.pane_hover == pane {
            return Ok(());
        }
        // **The play button is not in the chrome pass**, so the ordinary
        // repaint below does not reach it (§7.23 ⑩). It is the one control this
        // window draws in the preview body's own raster lane, and that lane is
        // rebuilt by `refresh_preview_body` — asked here only when the pointer
        // actually crossed this button, because rebuilding every pane's document
        // on every pointer move is the cost that lane exists to avoid.
        let crossed_the_play_button = [self.window.seat_pointer.hover, hover]
            .into_iter()
            .flatten()
            .any(|target| matches!(target, seats::ChromeTarget::PreviewPlay(_)));
        self.window.seat_pointer.hover = hover;
        self.window.seat_pointer.pane_hover = pane;
        if crossed_the_play_button {
            self.refresh_preview_body();
        }
        self.apply_pointer_cursor();
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// J118 — the press on a pane head has travelled 6px, so the pane is in the
    /// air.
    ///
    /// Nothing is measured and nothing is committed. A pane's head is not a
    /// handle the pane hangs from: the tree stays exactly as it was for the whole
    /// gesture, and the only thing that moves is the ghost. That is the mock-up's
    /// shape too — `startDrag(e, { kind: "pane", leafId })` records a leaf and
    /// nothing else (5839) — and it is why a pane drag that comes to nothing
    /// leaves no trace at all.
    ///
    /// The `.pane-close` dead zone (C35, mock-up 5837) needs no guard here: it is
    /// [`seats::ChromeTarget::PaneClose`], a target of its own, so a press on the
    /// `×` never reaches the head's arm of the router in the first place. "The
    /// button is not the bar" is true at the hit test, which is the only place it
    /// can be true once rather than everywhere.
    pub(in crate::runtime) fn begin_pane_drag(
        &mut self,
        seat: SeatId,
        position: PhysicalPosition<f64>,
    ) -> Result<()> {
        // **No focus-mode guard, and that is a ruling rather than a gap.** The
        // mock-up refuses every pane drag while the mode is on ("focus mode: the
        // tree is parked, no pane drags", 5841) because its focus mode parks the
        // tree behind a stage. Ours does not: §7.1.6b′ ① is that the mode
        // replaces the *tab list* and nothing else — "`render` 里没有聚焦分支,
        // 画的仍是 `renderNode(activeTab().tree)`" — so the stage in front of the
        // column is the ordinary tree, its panes are the ordinary panes, and a
        // drag among them is the ordinary gesture. What the mode does refuse is
        // the tear-out, and that refusal lives where the surface being dropped
        // on is described (`seats::PaneOffers::extract`, ②) rather than at the
        // moment a pane leaves the ground.
        // K135 covers a pane already, and by identity rather than by geometry:
        // a pane held over its own rectangle has no landing however that
        // rectangle has moved since the press. A home rectangle here would be
        // the same rule stated worse.
        // **The tab is recorded here and never re-read** (§7.1.6k). A pane drag
        // can outlive its tab being on screen now — the spring switches the view
        // and leaves the pane where it was picked up — so "which pane" has to be
        // answered once, at the moment the hand closed, rather than by asking
        // which tab happens to be showing later on.
        let leaf = LeafId {
            tab: self.window.tabs[self.window.active_tab].id,
            seat,
        };
        self.begin_drag(DragSource::Pane(leaf), DragCarry::Pane, position, None)
    }

    /// **H93 for §7.1.6k's landing** — whether `target`'s tree can actually take
    /// this pane at its end.
    ///
    /// The same [`seats::Seats::plan_drop`] the release will run, on the same
    /// inputs, asked for the promise instead of the commit. It is *re-asked*
    /// rather than remembered for [`Runtime::commit_layout_drop`]'s reason: a
    /// plan is what letting go would produce, computed, never a remembered
    /// picture — and a survey answered from a cache would go on lighting a tab
    /// up after a window resize made the drop unlawful.
    ///
    /// One tree walk and one solve per pointer move, on a tree of a few leaves,
    /// and only while a pane is actually resting on a foreign tab.
    pub(in crate::runtime) fn pane_adopt_fits(&self, leaf: LeafId, target: TabId) -> bool {
        let Some(from) = self.tab_state(leaf.tab) else {
            return false;
        };
        let Some(travelling) = from.seats.tree().find_seat(leaf.seat).cloned() else {
            return false;
        };
        self.arrival_fits(target, &bt_layout::LayoutNode::seat(travelling))
    }

    /// The layout's own box in device pixels — what every rim distance is
    /// measured from (K128/K130).
    ///
    /// Built from the swapchain and the DPI rather than from the seats inside it,
    /// through the same helper the solver's viewport comes from, so the rim and
    /// the rectangles it competes with cannot disagree about where the layout
    /// begins.
    pub(crate) fn layout_host_rect(&self) -> [f64; 4] {
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let dpi_milli = self.window.renderer.dpi_milli().get();
        let scale_ppm = seats::scale_ppm(dpi_milli);
        // The same inset `solve_seats` hands `logical_viewport`, through the same
        // helper: these two are twins, and a rim measured one pixel to the left
        // of where the solver put the terminal is a left-edge drop zone that aims
        // into the rail.
        seats::device_viewport(
            width,
            height,
            // The posture, because the solver is handed the posture: a rim
            // measured against the stored preference would aim its left-edge drop
            // zone at the pixels the card column is standing on.
            seats::rail_inset_device_px(self.rail_posture(), scale_ppm),
            // The same bar the solver's viewport starts under, through the same
            // helper and for [`seats::device_viewport`]'s own reason: a rim
            // measured one row above where the seats begin is a top-edge drop
            // zone aimed at the bar.
            seats::chrome_band_device_px(scale_ppm, self.platform_chrome()),
        )
    }

    /// **U7 — let go over the layout** (L136-L140, G81-G83, D43).
    ///
    /// The plan is computed from the drag's own inputs rather than lifted out of
    /// [`WindowRuntime::drop_preview`], and the two are the same object for a reason
    /// that is not a coincidence: `plan_drop` is pure (T223/D2), so the same
    /// inputs give back the same tree to the bit. Reading the cache would be
    /// asking a *remembered* answer to a question the world may have changed
    /// under — the fade that keeps a retired preview alive for its hundred
    /// milliseconds is enough on its own to make the cache older than the
    /// release — and re-asking costs one tree walk on a tree of a few leaves.
    ///
    /// Everything after the adoption is what a tree change costs, in the order
    /// `close_pane` and the preview toggle already pay it: the pointer's picture
    /// is stale because the rectangles moved under it, the window's own minimum
    /// is a function of the tree, and the terminal's columns are a function of
    /// its seat's rectangle. `commit_seat_geometry` marks the session dirty on
    /// the way through, so the strip's order, the tree's shape and the focused
    /// leaf all reach disk through the one channel that already carries them.
    ///
    /// Answers whether the tree changed.
    pub(in crate::runtime) fn commit_layout_drop(&mut self, drag: &Drag) -> Result<bool> {
        let Some(inputs) = self.plan_inputs_for(drag) else {
            return Ok(false);
        };
        let Some(plan) = self.plan_for(&inputs) else {
            return Ok(false);
        };
        // Three things are read off the *old* world, because the adoption below
        // replaces it and none of them can be asked afterwards.
        //
        // `arrived` is the renumbering's own record (N159/D44) and moves out of
        // the plan, which `adopt_drop` consumes. The displaced seat is N161's:
        // `ReplaceSeat` seats the arriving tree where the target pane stood, so
        // by the time the tree is installed there is no such seat to clone, and
        // everything durable about that pane lives on the `Seat` (§5).
        //
        // N160②'s "which pin does the ejected pane inherit" is *not* captured
        // here even though it is the same kind of before-and-after question. It
        // belongs beside the N160① that would overwrite it, which is
        // [`absorb_tab_into_layout`] — where a test can run the two in order.
        // **A row's centre verb is committed before anything is adopted, and
        // there is nothing to adopt.** `plan_for` built the *live* tree for it
        // (see [`seats::Seats::plan_content_drop`]), so `adopt_drop` would
        // install the layout that is already installed and move the focus for a
        // gesture that moved no pane. What the drop actually is, is one sentence
        // about one pane's content — and the verb table decides which.
        if let Some(payload) = drag.source.row() {
            let target_kind = self.aimed_seat_kind(inputs.landing);
            match row_verb(payload.kind, inputs.landing, target_kind) {
                // M147 — the box said so while the hand was open, and the
                // release says the same thing by doing nothing at all.
                RowVerb::Refused => return Ok(false),
                RowVerb::Retarget(target) => {
                    self.retarget_row_drop(payload, target)?;
                    return Ok(true);
                }
                // **The 2026-09-16 ruling's own line, behind the 2026-09-17
                // review's three questions.** A path let go of over a terminal's
                // middle goes through the door an Explorer or Finder drop goes
                // through ([`Runtime::paste_paths_into`]), so the quoting, the
                // `paste_paths_as` spelling, the refusal notice and the delivery
                // are that road's rather than a second one that drifts from it.
                // The keyboard does not move to it, because a drop is a pointer
                // gesture. What it must *not* inherit from the rest of the drag
                // engine is the cached aim — see [`Self::paste_offer_kept`].
                RowVerb::PastePath(_) => {
                    let Some(target) = self.paste_offer_kept(drag, &plan) else {
                        return Ok(false);
                    };
                    let path = payload.path.clone();
                    // **And the keyboard follows the path, strictly afterwards**
                    // (owner's ruling 2026-09-17). Behind the write's own
                    // answer, so a release that reached no shell — a stale
                    // target, a name the shell cannot spell — moves nothing:
                    // the reader is left exactly where they were, which is what
                    // every other refusal on this road already does. And it is
                    // the seat of the `PasteTarget` the two readings agreed on
                    // — never the hover-time seat, and never whichever pane
                    // happens to be holding the keyboard.
                    if self.paste_paths_into(target, vec![path], "write dragged path to PTY")? {
                        self.focus_the_pane_a_path_landed_in(target.seat)?;
                    }
                    return Ok(true);
                }
                RowVerb::Split => {}
            }
        }
        // **§7.1.6k′ — a pane the spring left in another tab is a *cross-tab*
        // move, however much the aim looks like a local one.**
        //
        // Everything above this line edits one tree: `adopt_drop` installs the
        // plan into the tab on screen and the source, if there was one, is a tab
        // giving up its whole self. A foreign pane has to be plucked out of a
        // tree nobody is drawing and filed into this one, its seven tables of
        // content moving with it and its own tab possibly emptying — which is
        // [`Runtime::move_pane_across_tabs`] to the letter, and the very door the
        // tab list's drop already goes through.
        //
        // `plan` is dropped here rather than handed over, and that costs one tree
        // walk and buys the discipline `commit_layout_drop` is built on: the
        // criterion is `plan_for`'s, and `plan_drop` is pure (T223/D2), so the
        // plan the commit rebuilds from the same inputs is the same tree to the
        // bit. What must not be re-decided is the *judgement*, and it is not —
        // a refusal reaches the commit through `plan.fits()` here and through the
        // same [`pane_can_become_a_tab`] there.
        let showing = self.window.tabs[self.window.active_tab].id;
        if let Some(leaf) = drag.source.pane().filter(|leaf| leaf.tab != showing) {
            // M147 — the dashed box already said so while the hand was open, and
            // the release says the same thing by doing nothing at all.
            let Some(aim) = inputs.landing.layout_aim().filter(|_| plan.fits()) else {
                return Ok(false);
            };
            return self.move_pane_across_tabs(leaf, showing, aim);
        }
        let arrived = plan.arrived.clone();
        // **The seat the plan minted for a row**, read before `adopt_drop`
        // consumes the plan. `arrived` is the renumbering's own record and a row
        // arrives as exactly one leaf, so the single pair in it is the identity
        // the payload has to be filed under — derived from the plan rather than
        // re-found in the new tree, which is the same argument N159's remapping
        // already makes.
        let landed_row = drag
            .source
            .row()
            .and_then(|_| arrived.first().map(|(_, landed)| *landed));
        // N161 — a centre with a whole *layout* in the hand evicts the target
        // pane. Only a tab can reach this line holding one: the foreign pane's
        // arm above has already returned, and every other source either arrives
        // as a seat of this tree (a swap, which evicts nobody) or as a leaf the
        // plan minted (a row, which never reaches a centre as a space verb).
        let displaced = match (inputs.landing, &drag.source) {
            (DropLanding::SeatCentre { target }, DragSource::Tab(_)) => {
                self.seats.tree().find_seat(target).cloned()
            }
            _ => None,
        };
        if self.seats.adopt_drop(plan).is_none() {
            return Ok(false);
        }
        if let (Some(payload), Some(seat)) = (drag.source.row(), landed_row) {
            // The pane exists and is empty; filling it is the same verb the
            // ordinary doors run, on a surface named rather than chosen.
            self.settle_seat_set_change()?;
            self.fill_row_leaf(&payload.clone(), seat)?;
            return Ok(true);
        }
        if let Some(source) = drag.tab() {
            self.absorb_tab(source, &arrived, displaced)?;
        }
        debug_assert!(
            self.sessions_match_terminals(),
            "item 6: a layout drop leaves the tab's shells matching its tree"
        );
        self.settle_seat_set_change()?;
        Ok(true)
    }

    /// **N157/K123 — let go of a pane over the strip.**
    ///
    /// [`tear_pane_into_tab`] does the whole of the move and is a free function
    /// so that a test can run it over a constructed `TabState`. What is left here
    /// is the window's: minting the tab id, handing over the solver, and putting
    /// the new entry in the run.
    ///
    /// **The new tab is not activated, and N157 is explicit about it.** You stay
    /// in the layout you were in and the pane waits for you in the strip — the
    /// gesture was "put this over there", not "take me there". So `active_tab`
    /// only moves to keep naming the tab it already named, which the insertion
    /// shifts by one when it lands at or before it.
    ///
    /// Its shell keeps the grid it had until you go and look at it, which is the
    /// same deal a revived tab gets: `activate_tab` re-solves the layout and
    /// schedules the grid change on the way in, and a shell resized to a
    /// rectangle nobody is drawing would be told a column count that never
    /// reaches the screen.
    ///
    /// Answers whether anything moved. `false` leaves the gesture to J120's slide
    /// home, which for a pane is a no-op — the tree was untouched for the whole
    /// drag.
    pub(in crate::runtime) fn commit_pane_extract(
        &mut self,
        drag: &Drag,
        slot: usize,
    ) -> Result<bool> {
        let DragSource::Pane(leaf) = drag.source else {
            return Ok(false);
        };
        // Whether anything moved is the whole of what a drop reports; the id the
        // tear-out now hands back is F1c's caller's, not this one's.
        Ok(self.extract_pane_into_new_tab(leaf, slot)?.is_some())
    }

    /// **§7.1.6k — let go of a pane over one of the strip's own tabs.**
    ///
    /// [`pane_into_tab`] does the whole of the move and is a free function so
    /// that a test can run it over two constructed `TabState`s. What is left here
    /// is the window's: finding the two entries, handing over the solver and the
    /// setting, and — when the tab the pane left is now empty — taking its entry
    /// out of the run.
    ///
    /// **The target is not activated**, and N157 is the precedent rather than an
    /// analogy: you aimed at a tab and said "put this over there", not "take me
    /// there". So the view does not move, and the pane waits for you in the tab
    /// you pointed at. The one exception is the tab you were *looking at* ceasing
    /// to exist because you moved its last pane away — there is no layout left to
    /// stay in, and the honest place to be is the one the pane went to.
    ///
    /// Answers whether anything moved. `false` leaves the gesture to J120's slide
    /// home, which for a pane is a no-op — the trees are untouched on that path.
    pub(in crate::runtime) fn commit_pane_adopt(
        &mut self,
        drag: &Drag,
        target: TabId,
    ) -> Result<bool> {
        let DragSource::Pane(leaf) = drag.source else {
            return Ok(false);
        };
        // **The tab list aims at the end of the tree and always has**, whichever
        // surface asked: *"追加为树末尾分屏"*. It is stated here rather than
        // inside [`Runtime::move_pane_across_tabs`] because it is what this
        // *door* means; the stage's door names a zone instead, and the thing
        // they share is everything after the aim.
        self.move_pane_across_tabs(leaf, target, seats::LayoutAim::Rim(self.append_edge()))
    }

    /// **The rail as it stands this instant** — the stored layout with both of
    /// its clocks read.
    ///
    /// The one place the two tweens become a [`seats::RailState`], so the paint,
    /// the hit test and the terminal's inset cannot be looking at three different
    /// moments of the same animation.
    ///
    /// Outside icon mode the fade is simply not a rule: the mock-up scopes every
    /// `opacity` declaration in that list to `.window.rail-icons` (lines 892-903),
    /// so an expanded rail's words are *there*, full stop, and are not waiting on
    /// a tween that has no reason to have been started.
    /// The fold is sampled on both paths and outside that `if`, because it is the
    /// one clock that runs while the rail is *not* drawing an icon rail — a rail
    /// folding away is collapsing, and `draws_icon_rail()` is false throughout.
    /// Reading it only on the icon path is precisely how the fold would go back to
    /// snapping.
    pub(crate) fn sampled_rail(&self, now: Instant) -> seats::RailState {
        let fold = Some(self.window.rail_fold.sample(now, self.app.motion).0);
        // **The window's posture, and the clocks on top of it.** The join of the
        // two halves is [`Self::rail_posture`]'s one line and is taken from there
        // rather than written a second time here: two spellings of "the rail as
        // this window wears it" is how the paint and the solver came to disagree
        // about whether a card column was on screen.
        //
        // Asked of the state as it *will be* rather than of the stored one: a
        // window in focus mode draws no icon rail whatever `Sidebar` says, so
        // asking the stored rail would sample the opening tween of a panel that
        // is not on screen.
        let resting = self.rail_posture();
        if !resting.draws_icon_rail() {
            return seats::RailState {
                open: 1.0,
                text_opacity: 1.0,
                fold,
                ..resting
            };
        }
        seats::RailState {
            open: self.window.rail_open.sample(now, self.app.motion).0,
            text_opacity: self.window.rail_text.sample(now, self.app.motion).0,
            fold,
            ..resting
        }
    }

    /// The rail's state as the *window* is currently wearing it — the stored
    /// preference with this window's focus-mode bit joined to it.
    ///
    /// [`Self::sampled_rail`] without the clock, for the callers that ask a shape
    /// question outside a frame: "is an icon rail live", "how much width does the
    /// panel cost". Those must answer to the posture and not to the preference,
    /// or a window in focus mode goes on reasoning about a rail nobody is
    /// drawing.
    ///
    /// **The solver is one of those callers, and that is the whole of the fix
    /// this branch carries.** The card column is in the flow exactly as an
    /// expanded rail is: `terminal_inset_logical_px` answers the column's own
    /// width for it, and the viewport `solve_seats` is handed begins there. Passing
    /// [`WindowRuntime::rail`] instead — which it did — is asking the solver to
    /// lay the stage out as though the panel were not on screen, and a panel
    /// drawn over a stage that was never told about it is the occlusion the
    /// screenshots showed.
    /// **And the card height joins here too** (user ruling 2026-08-21).
    ///
    /// `Appearance ▸ Focus card height` is a *file* preference, like the mode's
    /// own bit — one answer for every window — while `window.rail` holds the two
    /// preferences this window happens to be wearing. Joining it at the one
    /// place the posture is assembled is what makes the row take effect at once
    /// and in every window: nothing caches a card height, so the next frame
    /// solves the column at whatever the file now says. A copy kept on
    /// `WindowRuntime` would be a second answer to re-sync on every door.
    pub(crate) fn rail_posture(&self) -> seats::RailState {
        seats::RailState {
            focus: self.window.focus_mode,
            focus_card_body_logical_px: self.app.settings_store.loaded().focus_card_height as f32,
            ..self.window.rail
        }
    }

    /// **A pane that changed its address takes its page with it** (§7.10 ④‴).
    ///
    /// [`move_seat_content`] and [`pane_into_new_tab`] carry the seven tables a
    /// pane can be holding, and a page is not one of them — it cannot be, because
    /// [`WindowRuntime::web`] belongs to the **window** and those two are
    /// functions over a [`TabState`]. So the pane's other half is carried here,
    /// by the three window-level doors that know both ends of the journey:
    /// [`Self::extract_pane_into_new_tab`], [`Self::move_pane_across_tabs`] and
    /// [`Self::absorb_tab`].
    ///
    /// **Without it the browser is not merely misfiled — it is closed.** The map
    /// is keyed by [`LeafId`] and both halves of that key change on a move (the
    /// arriving tree re-mints its seat numbers from one, and the tab is a
    /// different tab), so the page went on answering to a name whose tab no
    /// longer has that seat; `advance_web_page` asks exactly that question of
    /// exactly that tab, called the page orphaned and shut the controller down.
    /// On the machine that is a preview pane dragged onto the tab strip whose
    /// `.html` or `.pdf` goes **blank** while its head and address bar — which
    /// travel with the pane — go on naming the file (user report on `next21`).
    ///
    /// **Taken out of the table before any of them is put back**, which is not
    /// tidiness: a centre *trades* (§7.1.6k′ B4), and the pane that arrives is
    /// filed under the id the pane that left was filed under, so an insert
    /// interleaved with the removes would have one live browser overwrite the
    /// other and lose it without going through the door that waits for its
    /// process to exit.
    ///
    /// [`webhost::WebSeat::rehost`] and not a bare re-key, for the reason written
    /// on it: the seat caches its own address, and the compositor's visual table
    /// is keyed by that address too, so moving only the map entry would leave the
    /// next rebuild asking for a controller under a name nothing answers to. Both
    /// sides are this window's compositor because this is a move inside one
    /// window — the same call the float's re-dock already makes.
    ///
    /// **The keyboard is not taken.** `transfer_tab` passes `take_focus` for the
    /// pane the moved tab is standing on because a person carried that tab into
    /// this window; a tear-out deliberately does not activate the tab it makes
    /// (N157), and a pane dropped on another tab lands wherever the layout put
    /// the focus. Whether the page is what typing goes into is then
    /// `settle_the_web_keyboard`'s answer on the next frame, as it is for every
    /// other page in this window.
    pub(in crate::runtime) fn carry_the_pages_of_moved_panes(
        &mut self,
        moves: &[(LeafId, LeafId)],
    ) -> Result<()> {
        let hosted: BTreeSet<LeafId> = self.window.web.keys().copied().collect();
        let travelling = pages_that_move_with_their_panes(&hosted, moves);
        if travelling.is_empty() {
            return Ok(());
        }
        // **Each page's new place, asked before anything moves**: a window that cannot name one
        // refuses here, with every page still where it was.
        let addressed: Vec<(LeafId, LeafId, webhost::SeatAddress)> = travelling
            .iter()
            .map(|(was, now)| {
                let address = self.window.window.page_address(bt_platform::PageVisual {
                    tab: now.tab.0,
                    seat: now.seat.0,
                })?;
                Ok((*was, *now, address))
            })
            .collect::<Result<_, String>>()
            .map_err(|error| anyhow!(error))?;
        let carried: Vec<(LeafId, webhost::SeatAddress, webhost::WebSeat)> = addressed
            .into_iter()
            .filter_map(|(was, now, address)| {
                let page = self.window.web.remove(&was)?;
                let picture = self.window.web_thumbs.take(was);
                if let Some(picture) = picture {
                    self.window.web_thumbs.put(now, picture);
                }
                Some((now, address, page))
            })
            .collect();
        for (now, address, mut page) in carried {
            let mut outcomes = Vec::new();
            let report = page.rehost(
                &self.window.compositor,
                &self.window.compositor,
                address,
                false,
                &mut outcomes,
            );
            if let Some(error) = report.error() {
                crate::diagnostics::stderr_line(format!(
                    "BT_WEB the page could not follow its pane: {error}"
                ));
            }
            self.window.web.insert(now, page);
            self.apply_web_outcomes(now, outcomes)?;
        }
        Ok(())
    }

    /// **A pane that changed its address takes its recording with it** (user
    /// report on `next22`, defects #202/#204; §7.44 ⑮).
    ///
    /// [`Self::carry_the_pages_of_moved_panes`] said one tab-level table apart
    /// from the seven a pane carries; this is the second, and it is a separate
    /// door rather than a second half of that one because the two tables are
    /// different tables with different keys: [`WindowRuntime::web`] is keyed by
    /// [`LeafId`] and [`WindowRuntime::video`] by [`PreviewSurface`], since a
    /// recording plays on a pane, on a float and on the glance card while a page
    /// only ever plays on a pane. Only the first of those three is a leaf, so
    /// only the first of them can be part of a move at all — which is the whole
    /// of the filter below.
    ///
    /// **Without it the recording does not merely lag — it is shut down.**
    /// [`Runtime::sweep_video_seats`] keeps a seat while the surface it is keyed
    /// by is still about the file it was opened on, and both halves of that key
    /// change on a move; so the seat went on being keyed by an address the
    /// window no longer has, the sweep read it as a surface that has stopped
    /// existing, and the decoder was stopped on the next frame. On the machine
    /// that is a video pane dragged onto the tab strip whose picture goes
    /// **blank** while its head, its control bar and its fact line — which
    /// travel with the pane — go on naming the recording.
    ///
    /// **Taken out of the table before any of them is put back**, which is the
    /// page carrier's reason said again about a different map: a centre *trades*
    /// (§7.1.6k′ B4), so the arriving recording is filed under the key the
    /// departing one was filed under, and an insert interleaved with the removes
    /// would have [`video_seat::VideoSeats::put`] shut down a live engine — the
    /// one thing [`video_seat::VideoSeats::take`] and `put` exist to let this
    /// function avoid.
    ///
    /// No `Result`, unlike its sibling: nothing here can fail. A rehome is one
    /// key changing — the engine is not touched, the decoder does not restart,
    /// the playhead does not go back and the texture keeps its name — so there
    /// is no platform call to report on.
    pub(in crate::runtime) fn carry_the_recordings_of_moved_panes(
        &mut self,
        moves: &[(LeafId, LeafId)],
    ) {
        let hosted: BTreeSet<LeafId> = self
            .window
            .video
            .iter()
            .filter_map(|(surface, _)| match surface {
                PreviewSurface::Seat(leaf) => Some(leaf),
                PreviewSurface::Float(_) | PreviewSurface::Peek => None,
            })
            .collect();
        // The same rule the page table asks, asked of the recording table: a
        // pair whose two halves are equal is not a move and must not be lifted
        // and put back, because the next pair in the transaction may be filing
        // something under that key.
        let travelling = pages_that_move_with_their_panes(&hosted, moves);
        if travelling.is_empty() {
            return;
        }
        let carried: Vec<(PreviewSurface, video_seat::VideoSeat)> = travelling
            .iter()
            .filter_map(|(was, now)| {
                let seat = self.window.video.take(PreviewSurface::Seat(*was))?;
                Some((PreviewSurface::Seat(*now), seat))
            })
            .collect();
        for (now, seat) in carried {
            self.window.video.put(now, seat);
        }
    }

    /// Where the box stands this frame.
    pub(in crate::runtime) fn palette_layout(&mut self) -> Option<palette::PaletteLayout> {
        let state = self.window.palette.as_ref()?;
        let listing = state.listing().clone();
        let scroll = state.scroll();
        let (shown, before, typed) = self.palette_field_look();
        let scale = self.window.renderer.scale_factor() as f32;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        Some(palette::layout(
            (width as f32, height as f32),
            scale,
            &listing,
            palette::FieldLook {
                shown: &shown,
                before: &before,
                typed,
            },
            scroll,
            &mut measure,
        ))
    }

    /// The rail's trailers and the length of its pinned run, together.
    ///
    /// One call so the seam the rail draws and the rows it hits are measured from
    /// one list — [`seats::pinned_run_len`]'s own argument, one level up.
    pub(in crate::runtime) fn rail_list(&self, now: Instant) -> (Vec<seats::TabTrailer>, usize) {
        let trailers = self.tab_trailers(now);
        let pinned = seats::pinned_run_len(&trailers);
        (trailers, pinned)
    }

    /// The rail's live geometry — [`Runtime::strip_geometry`]'s opposite number.
    ///
    /// `None` exactly when no rail is on screen: a horizontal layout, or a
    /// collapsed one. That is [`seats::rail_geometry`]'s own answer passed
    /// through rather than a second reading of the same question.
    pub(crate) fn rail_geometry_now(&self, now: Instant) -> Option<seats::RailGeometry> {
        let scale = self.window.renderer.scale_factor() as f32;
        let (_, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (trailers, pinned) = self.rail_list(now);
        seats::rail_geometry(
            height as f32,
            scale,
            self.platform_chrome(),
            &trailers,
            pinned,
            self.window.rail_scroll,
            self.sampled_rail(now),
        )
    }

    /// **R2's open trigger, in the smallest form that is usable.**
    ///
    /// The pointer is in the rail's own box, or it is not. That is deliberately
    /// *not* the mock-up's `evalRailZone`, which opens on a reach band wider than
    /// the panel and keeps it open over a wider one still — CSS `:hover` "has no
    /// way to express 'stay open while the pointer is anywhere near', and a zone
    /// with hysteresis does". **The zone, its `RAIL_REACH`/`RAIL_KEEP` widths and
    /// the remembered pointer position that lets a resize re-evaluate it without
    /// a mouse-move are deferred to the sidebar ticket**; what is here is the
    /// half that can be honest on its own.
    ///
    /// It is measured against the width the rail is *aiming* at rather than the
    /// width it currently has, and that is what keeps it from chattering: a test
    /// against the animating width would re-decide the question against a
    /// different boundary on every frame of the answer it just gave.
    pub(in crate::runtime) fn drive_rail_zone(&mut self, position: Option<PhysicalPosition<f64>>) {
        // Asked of the window's posture rather than of the stored preference: in
        // focus mode there is no icon rail to reach for, and a trigger that went
        // on opening one would be aiming a tween at a panel nobody is drawing.
        if !self.rail_posture().draws_icon_rail() {
            return;
        }
        let scale = self.window.renderer.scale_factor();
        let aiming_open = self.window.rail_open.to > 0.5;
        let target = seats::RailState {
            open: f32::from(u8::from(aiming_open)),
            ..self.window.rail
        };
        let width = f64::from(target.width_logical_px()) * scale;
        // The rail begins at the bar's lower edge, so a pointer up in the
        // caption run is not in the rail however far left it is. **Asked of the
        // window rather than of the constant** (T-MAC-LIGHTS): the header is a
        // fact about this window's chrome, and a trigger measured from anything
        // else would leave the top of the parked rail refusing to open.
        let top = f64::from(seats::window_band_px(scale as f32, self.platform_chrome()));
        // The peek is handed over as **the trigger it hangs from**, not as a
        // yes/no: only a peek hanging off a rail row is the rail's business, and
        // that is a question about identity that no boolean can carry. See
        // [`rail_zone_wants_open`].
        //
        // The popup arrives as a yes/no for the opposite reason: which popup it
        // is has already been decided by [`Self::rail_grew_a_popup`], against a
        // fact — [`popup_owner`] — rather than against a rectangle.
        self.aim_rail_at(rail_zone_wants_open(
            position,
            width,
            top,
            self.window.float.peek().and_then(|win| win.origin),
            self.rail_grew_a_popup(),
        ));
    }

    /// Point both of the rail's clocks at `open`.
    ///
    /// The delay is one-sided, which is [`RAIL_TEXT_FADE_OPEN_DELAY`]'s own doc:
    /// the words wait for the panel on the way out and leave ahead of it on the
    /// way back. Reduced motion is handled where every other tween handles it —
    /// inside [`RevealTween`], which under `Motion::Reduced` simply reports the
    /// target and asks for no frames at all.
    fn aim_rail_at(&mut self, open: bool) {
        let target = f32::from(u8::from(open));
        if self.window.rail_open.to == target {
            return;
        }
        let now = Instant::now();
        let motion = self.app.motion;
        self.window.rail_open.retarget(target, now, motion);
        if open {
            self.window
                .rail_text
                .retarget_after(target, now, motion, RAIL_TEXT_FADE_OPEN_DELAY);
        } else {
            self.window.rail_text.retarget(target, now, motion);
        }
    }

    /// **`.panel-toggle`'s verb (Q178): fold the rail away, or bring it back.**
    ///
    /// R1's real trigger, arriving where the mock-up puts it — `#btn-rail` in the
    /// title bar (2270). The state it writes has been here since Q178 and the
    /// geometry has always answered to it (`RailState::collapsed` is read by
    /// `terminal_inset_logical_px` and by `rail_geometry`); what was missing was
    /// only something a hand could press.
    ///
    /// Through [`Self::set_rail_state`] rather than by assigning the flag,
    /// because collapsing moves the terminal's left edge exactly as changing the
    /// mode does, and every consequence of that — re-solving the panes, the
    /// window's minimum size, the hover under a pointer that did not move — is
    /// already spelled out there once.
    ///
    /// Not persisted, and deliberately: `layout` and `mode` are answers to "where
    /// do I want my tabs", which is a standing preference, while a fold is
    /// "give me the room back for a minute". The session schema carries the first
    /// pair (v5's `tab_layout`/`sidebar_mode`) and has never carried this, so a
    /// window opens with its rail out, which is also what the mock-up's own
    /// `state.railCollapsed` does on reload.
    pub(in crate::runtime) fn toggle_rail_collapsed(&mut self) -> Result<()> {
        self.set_rail_state(seats::RailState {
            collapsed: !self.window.rail.collapsed,
            ..self.window.rail
        })
    }

    /// Put the rail in a posture and pay for it — the one commit path the dev
    /// chord, the settings dialog and the panel toggle take, so none of them can
    /// move the rail without the panes, the window minimum, the hover and the
    /// session file following it.
    pub(crate) fn set_rail_state(&mut self, state: seats::RailState) -> Result<()> {
        let moved = (self.window.rail.layout, self.window.rail.mode) != (state.layout, state.mode);
        // **The panel takes its own popups with it** (§7.1.6e″ ③). A menu the
        // rail grew is part of the rail, so it cannot be left standing over a
        // window whose rail has just folded away or moved house — see
        // [`rail_change_strands_its_popups`] for which changes those are.
        //
        // Ahead of the assignment below, and that is the point of the order:
        // ownership is read off the posture the popup was raised in, not the one
        // that is about to replace it.
        if rail_change_strands_its_popups(self.window.rail, state) {
            self.close_rail_popups();
        }
        // P168 — the fold travels rather than cutting. `.window.rail-collapsed`
        // sets `.rail`'s width to zero and `.rail` carries `width .18s ease`, so
        // the panel is seen to leave instead of blinking out.
        //
        // Aimed only when the flag actually turned over, because
        // `set_rail_state` is also the settings dialog's commit path: choosing a
        // sidebar mode must not restart a transition on an edge that is not
        // moving. Nothing here needs to care about the horizontal layout —
        // `width_logical_px` answers `0.0` for it whatever the fold holds, so a
        // rail that has no business being on screen cannot be eased onto it.
        //
        // Reduced motion is handled where every other tween handles it, inside
        // `RevealTween`: under `Motion::Reduced` the value snaps to its target
        // and asks for no frames, so the fold is instant and the panel-toggle
        // stays as immediate as it was before it could animate.
        if self.window.rail.collapsed != state.collapsed {
            self.window.rail_fold.retarget(
                f32::from(u8::from(!state.collapsed)),
                Instant::now(),
                self.app.motion,
            );
        }
        self.window.rail = state;
        // **The bar this window wears may have just changed, and the platform's
        // own buttons stand in it** (T-MAC-LIGHTS x T-MAC-PILL). Ahead of the
        // solve below, so the first frame drawn against the new band is drawn
        // with the lights already on it.
        self.follow_the_window_band()?;
        crate::diagnostics::stderr_line(format!(
            "BT_RAIL layout={:?} mode={:?} focus={} inset={}px",
            self.window.rail.layout,
            self.window.rail.mode,
            self.window.focus_mode,
            // The inset the stage will actually be solved with, which is the
            // posture's — a trace printed off the preference would report 0 for a
            // window whose panes had just moved 220px.
            self.rail_posture().terminal_inset_logical_px()
        ));
        // An icon rail is born parked, whatever the last one was doing, and its
        // words are away with it. A pointer already standing inside it opens it
        // again on the very next move — which is the trigger doing its job, not
        // this deciding for it.
        //
        // **Only when the posture actually moved.** A rail that is merely being
        // folded away by `.panel-toggle` is the same rail in the same mode, and
        // re-parking it would spend the list's scroll position on a gesture that
        // never claimed to move it — fold and unfold, and the row you were
        // looking at is gone. "Born parked" is about a rail that has just become
        // an icon rail; a fold does not make a new one.
        if moved {
            self.window.rail_open = RevealTween::over(RAIL_TRANSITION);
            self.window.rail_text = RevealTween::over(RAIL_TEXT_FADE);
            self.window.rail_scroll = 0.0;
        }
        // The inset changed, so every pane's rectangle did. This is the same
        // path a window resize takes, and it has to be taken here: the rail's
        // *mode* moves the terminal's left edge even though the rail's own
        // opening never does (Q179).
        self.commit_seat_geometry()?;
        self.apply_window_min_inner_size()?;
        if let Some(position) = self.window.pointer_position {
            self.window.seat_pointer.hover = self.chrome_target_at(position);
        }
        // Both halves are layout intent, so they persist exactly as the theme
        // and the cursor shape do — a window that opened with the tabs across
        // the top after the user moved them down the side would be the same
        // broken promise.
        if moved {
            self.mark_session_dirty(Instant::now());
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    pub(in crate::runtime) fn scroll_rail(&mut self, delta: MouseScrollDelta) -> Result<()> {
        let now = Instant::now();
        // **The plain notch is this list's; `Alt` aims the seat under the
        // pointer** ([`column_notch`], user ruling 2026-08-21). The decision is
        // asked here, above everything the list does, because the two readings
        // are alternatives and not a fallback: a bare notch is the column's even
        // when the column has nowhere to go, and a modified one that finds no
        // terminal seat under the pointer is the column's too.
        // **And it is asked of the hand rather than of the keyboard** (§13.33
        // ①). `window.modifiers` has had `Option` taken out of it on a Mac whose
        // reader has left `Option key sends Alt` off, because there it is text;
        // a notch composes no text, so this reads what is actually held.
        let notch = column_notch(self.window.modifiers_held);
        if notch == ColumnNotch::Aim && self.aim_focus_card_window(now, delta)? {
            // The one route word that says the aim answered (`BT_MOUSE_TRACE`,
            // §7.60). `wheel_aim` one line above says what it did with it.
            self.mouse_trace(|| "wheel_route taken=rail-aim".to_owned());
            return Ok(());
        }
        // The two lists share `rail_scroll` because they share the panel; what
        // differs between them is only how long the content is, which is exactly
        // what `viewport` and `max_scroll` carry.
        let Some((viewport, max_scroll)) = self
            .rail_geometry_now(now)
            .map(|geometry| (geometry.viewport, geometry.max_scroll))
            .or_else(|| {
                self.focus_rail_geometry_now(now)
                    .map(|geometry| (geometry.viewport, geometry.max_scroll))
            })
        else {
            self.mouse_trace(|| "wheel_route taken=rail-scroll at=no-geometry".to_owned());
            return Ok(());
        };
        let travel = self.vertical_wheel_travel(delta, viewport[1] - viewport[0]);
        // Wheel-up reveals what lies above, which is a smaller offset.
        let scrolled = (self.window.rail_scroll - travel).clamp(0.0, max_scroll);
        if scrolled == self.window.rail_scroll {
            let held = self.window.rail_scroll;
            self.mouse_trace(|| {
                format!(
                    "wheel_route taken=rail-scroll at=clamped rail_scroll={held} \
                     travel={travel} max_scroll={max_scroll}"
                )
            });
            return Ok(());
        }
        let was = self.window.rail_scroll;
        self.mouse_trace(|| {
            format!(
                "wheel_route taken=rail-scroll at=scrolled rail_scroll={was} to={scrolled} \
                 max_scroll={max_scroll}"
            )
        });
        self.window.rail_scroll = scrolled;
        // The list moved under a stationary pointer, so what it is over changed
        // without the pointer having done anything.
        if let Some(position) = self.window.pointer_position {
            self.window.seat_pointer.hover = self.chrome_target_at(position);
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Whether the pointer is over the panel on the left, whichever list it is
    /// holding — the wheel's gate, and `.pane:hover`'s.
    ///
    /// One function over both surfaces because it is one rectangle: the card
    /// column and the rail are the same panel, and a pointer inside it belongs to
    /// it either way. Asking only the rail was right until §7.1.6b′ gave the
    /// panel a second thing to hold.
    pub(in crate::runtime) fn panel_covers(&self, position: PhysicalPosition<f64>) -> bool {
        let now = Instant::now();
        self.rail_geometry_now(now)
            .is_some_and(|rail| rail.covers(position.x, position.y))
            || self
                .focus_rail_geometry_now(now)
                .is_some_and(|column| column.covers(position.x, position.y))
    }

    /// Whether the pointer is over the rail's own box — the wheel's gate.
    pub(in crate::runtime) fn rail_contains(&self, position: PhysicalPosition<f64>) -> bool {
        let now = Instant::now();
        self.rail_geometry_now(now)
            .map(|geometry| seats::rail_run(&geometry))
            .or_else(|| {
                self.focus_rail_geometry_now(now)
                    .map(|geometry| seats::focus_rail_run(&geometry))
            })
            .is_some_and(|run| run.contains(position.x, position.y))
    }

    /// **The rail's own gate, with the column as it is *painted* beside the
    /// column the aim *walks*** (`BT_MOUSE_TRACE`, §7.60).
    ///
    /// Two heights reach [`seats::focus_rail_geometry`] in this program and
    /// nothing had ever compared them: the chrome is built against the bottom of
    /// the lowest pane the solver placed ([`seats::chrome_surface_height`]) and
    /// [`Self::focus_rail_geometry_now`] — which is what `rail_contains` and
    /// [`Self::aim_focus_card_window`] both read — is solved against the
    /// swapchain's. While they agree, a card is where it is drawn; if they ever
    /// part, every card in the column is offset from its own picture and a
    /// gesture aimed at what the reader can see misses. So both are solved here
    /// and printed on one line, with `agree` as the answer.
    ///
    /// **Inside the closure**, so the second solve happens only for a reader who
    /// asked for the file: a diagnostic nobody turned on must cost one atomic
    /// load and nothing else.
    pub(in crate::runtime) fn wheel_rail_trace(
        &self,
        now: Instant,
        position: PhysicalPosition<f64>,
        contains: bool,
    ) {
        self.mouse_trace(|| {
            let aim_height = self
                .window
                .renderer
                .presentation_geometry()
                .swapchain_size
                .1 as f32;
            let paint_height = seats::chrome_surface_height(&self.seat_layout);
            // **The same solver twice, with one number changed.** Anything else
            // would be two columns differing in more than the quantity under
            // investigation, and `agree` would stop meaning what it says.
            let aim = self.focus_rail_geometry_at(now, aim_height);
            let paint = self.focus_rail_geometry_at(now, paint_height);
            mouse_trace::WheelRail {
                contains,
                point: (position.x, position.y),
                rail_scroll: self.window.rail_scroll,
                strip_rail: self.rail_geometry_now(now).map(|geometry| geometry.body),
                aim_height,
                aim: aim.as_ref(),
                paint_height,
                paint: paint.as_ref(),
            }
            .line()
        });
    }

    /// The *program* is about to change the rectangle, so the layout it produces is held to the
    /// minima in full.
    ///
    /// Claims the next rectangle sight unseen rather than recording one now: at this point the
    /// request has been made and the OS has not yet answered it, and predicting the answer is
    /// exactly the guess this mechanism exists to avoid.
    pub(in crate::runtime) fn claim_lawful_layout(&mut self) {
        self.window.size_policy = SizePolicy::Lawful;
        self.window.lawful_client_size = None;
    }

    /// **How far the panel's list is scrolled, said again in the new display's
    /// pixels** (user report 2026-09-12, §7.1.6b′).
    ///
    /// [`WindowRuntime::rail_scroll`] and [`WindowRuntime::tab_scroll`] are the
    /// only numbers the tab panel keeps between frames that are *physical
    /// pixels*. Everything else it stands on — a card's box, its head, its mini
    /// seats, the sticky `+`, the clip box, the hit test — is solved from
    /// `metrics().scale_factor` on the frame it is drawn, so a window carried to
    /// a display of another scale re-derives all of it and needs no help. These
    /// two do not: they were measured against the display the list was last
    /// scrolled on, and left alone they say a different distance there.
    ///
    /// The distance is the fact, and the pixels are how it was written down, so
    /// the restatement is the ratio between the two scales — the same similarity
    /// transform §7.50 already applies to every rectangle on this path ("the
    /// same tree solved again on a new rectangle"). A column standing at its end
    /// on a 200% display stands at its end on a 150% one; a column halfway down
    /// stays halfway down.
    ///
    /// **Both offsets, because the panel has two lists and one of them is not
    /// the cards.** The vertical rail and the card column share `rail_scroll`
    /// and the horizontal strip has `tab_scroll`; all three are read by geometry
    /// that multiplies by the current scale, so all three are stale in exactly
    /// the same way. Restating one and not the others is the next report.
    pub(in crate::runtime) fn restate_panel_scroll(&mut self, measured_at: f64, now_at: f64) {
        self.window.rail_scroll = restated_scroll(self.window.rail_scroll, measured_at, now_at);
        self.window.tab_scroll = restated_scroll(self.window.tab_scroll, measured_at, now_at);
    }

    /// Every terminal pane's rectangle pair on this frame's own clock, and the
    /// preview seat's picture re-placed against the same one.
    ///
    /// Lifted out of [`Self::redraw`] so the chrome-only present can ask the
    /// identical question: a ring turning while a pane is mid-flight has to be
    /// drawn through the transform this instant, and a second copy of this
    /// arithmetic would be a second answer.
    pub(in crate::runtime) fn pane_draws(&mut self, now: Instant) -> Vec<PaneDraw> {
        let scale = self.window.renderer.scale_factor() as f32;
        let motion = self.app.motion;
        let bodies: Vec<PaneDraw> = self
            .seats
            .terminals()
            .into_iter()
            .filter_map(|seat| {
                let body = seats::pane_body_viewport(&self.seats, &self.seat_layout, seat, scale)?;
                let device = self.seat_layout.get(seat)?.device_rect?;
                let pane = [
                    device.left as f32,
                    device.top as f32,
                    device.right as f32,
                    device.bottom as f32,
                ];
                let (viewport, clip) = animated_pane_viewports(
                    body,
                    pane,
                    self.window.pane_motion.transform_of(seat, now, motion),
                );
                Some(PaneDraw {
                    seat,
                    viewport,
                    clip,
                })
            })
            .collect();
        // **U8 — every picture's pane, re-placed on every animated frame.**
        //
        // Their panes FLIP with the rest, and the pair of rectangles each is
        // drawn through is a function of the clock; `refresh_preview_for_layout`
        // runs once per commit and cannot answer for the frames in between. Only
        // the placement is touched here — the raster, its key and the extent it
        // was fitted to belong to the commit, and re-deciding those per frame is
        // the resample storm R2 exists to forbid.
        //
        // **Each against its own pane, and all of them** (§7.1.6k⁵, §7.1.6k⁷).
        // This asked `seats.preview()` — the first preview leaf in the tree —
        // until a tab could hold two, and then it asked which single pane held
        // the one texture lane. Both were the same defect a slice apart: a
        // picture fitted against one pane and placed against another, or not
        // placed at all. The panes of this tab that are holding pictures are the
        // pictures the renderer is holding, so the walk is over them.
        for surface in self.seat_pictures() {
            let PreviewSurface::Seat(leaf) = surface else {
                continue;
            };
            if let Some(placement) = preview_image_placement(
                &self.seats,
                &self.seat_layout,
                leaf.seat,
                scale,
                self.window.pane_motion.transform_of(leaf.seat, now, motion),
            ) {
                self.window.renderer.place_preview_image(
                    picture_channel_owner(surface),
                    placement.seat,
                    placement.clip,
                );
            }
        }
        // And the hosted page, on the same clock and for the same reason: a web
        // seat is a pane, it FLIPs with its neighbours, and a rectangle that is
        // a function of the clock cannot be answered once per commit.
        self.sync_web_page(now);
        bodies
    }

    /// **The page the landing rule would reuse, if that pane holds one** (user
    /// ruling 2026-09-06: 一扇窗允许任意多个网页 pane).
    ///
    /// The singleton rule this replaces (`plan.md` §0 ②「每 tab 单例」) said the
    /// reuse target was *whichever* pane of this tab held a page, first in tree
    /// order. That is what made a second page impossible: a reader who split a
    /// second preview pane and then clicked a path got the **first** pane
    /// navigated and the new pane left standing on its empty placeholder — the
    /// screenshot in the report, and the sentence under it ("一个窗口只能有一个
    /// 网页预览 pane 吗").
    ///
    /// So a page lands where anything else a reader opens lands: on
    /// [`seats::Seats::landing_preview`], the first preview pane that is not
    /// locked. Every consequence the reader already knows follows from a rule
    /// they already know — lock a page and the next one opens beside it, leave it
    /// unlocked and the next one replaces it — and a tab may hold as many pages
    /// as it holds preview panes, each with its own controller on its own visual.
    ///
    /// `None` when the landing pane holds no page, which is what makes the two
    /// arms of the verbs below fork: navigate the page that is there, or make
    /// one.
    pub(crate) fn page_on_the_landing_pane(&self) -> Option<SeatId> {
        self.seats
            .landing_preview()
            .filter(|seat| self.seat_holds_a_page(*seat))
    }

    /// **Which seat `Ctrl+F` opens the capsule on** (§7.7 ②, W2 slice ④).
    ///
    /// `focused_leaf` names the shell the keyboard falls back to and is only
    /// ever written for a seat that is in `sessions` — so on a window whose
    /// keyboard is in a page it still names the terminal beside it, and a
    /// capsule opened through it lands on the wrong pane (found on the machine,
    /// 2026-08-22: `Ctrl+F` over a page opened a search on the shell next door).
    /// The capsule has two hosts now, so the question has two answers and this
    /// is where they are chosen between.
    pub(in crate::runtime) fn search_host_seat(&self) -> SeatId {
        self.focused_web_seat().unwrap_or(self.focused_leaf)
    }

    /// **The one door a frame reaches the screen through.**
    ///
    /// # wgpu presents; the compositor publishes
    ///
    /// Since slice A2 this window's swapchain is the content of a
    /// DirectComposition visual rather than the window's own surface, and wgpu's
    /// dx12 backend calls `SetContent` on that visual without ever calling
    /// `Commit` (`wgpu-hal-30.0.0/src/dx12/mod.rs:1619` — the `Visual` arm,
    /// beside the `VisualFromWndHandle` arm where wgpu owns the composition
    /// device and does commit). Whoever owns the DirectComposition device owns
    /// the commit, and here that is [`WindowRuntime::compositor`]. A present without
    /// its commit is not a dropped frame — it is a screen that never changes
    /// again while every trace in the program reports frames presenting
    /// normally.
    ///
    /// So there is one funnel and it is grep-pinnable: **this is the only call
    /// to `WindowRenderer::present_frame` in `bt-app`**, and the commit is the
    /// statement after it. Two callers reach it — [`Self::redraw`] with a newly
    /// composed frame, and [`Self::present_retained_picture`] with the picture
    /// already on the glass — and a resize present is `redraw` again, not a
    /// third path.
    ///
    /// An associated function rather than a method because both callers hold a
    /// borrow of some other part of `self` across the call: the retained-picture
    /// path is presenting `self.last_presented_frame` itself.
    ///
    /// The commit is skipped when nothing was presented. `Skipped` and
    /// `Reconfigure` both mean the swapchain handed back no image, so there is
    /// nothing new for the compositor to publish and the frame is re-filed by
    /// the caller.
    pub(in crate::runtime) fn present_seats_and_commit(
        gpu: &mut B::Device,
        renderer: &mut B::Glass,
        compositor: &B::Compositor,
        window: &B::Window,
        traces: FrameTraces<'_>,
        seat_frames: &[bt_render::SeatFrame<'_>],
        intent: PresentIntent,
    ) -> Result<Option<PresentOutcome>> {
        hang_watch::during(hang_watch::Station::PresentSeats, || {
            let PresentIntent {
                trigger,
                mut signature,
            } = intent;
            let FrameTraces {
                attempt,
                preview,
                census,
                gate,
                trace_perf,
                slot_overwrites,
                conditions,
            } = traces;
            if gpu.device_loss().is_none() && gate.unchanged(&signature, conditions) {
                trace_unchanged_present(
                    trace_perf,
                    gate,
                    trigger.source,
                    seat_frames.first().map(|seat| seat.frame),
                    slot_overwrites,
                );
                attempt.outcome = present_diagnostics::Outcome::Unchanged;
                return Ok(None);
            }
            // Failure, textless presentation, or even a failed commit must not
            // leave an old signature claiming that the surface is still complete.
            gate.invalidate();
            // **What the frame is allowed to cost to measure**, decided at the one
            // funnel because that is the one place every frame passes. Counting a
            // frame's demand on the glyph atlas means rasterizing each distinct
            // glyph a second time to learn its size, so it is off unless
            // `BT_GLYPH_CENSUS` names a file to write it to; setting the flag is a
            // bool store and asking the gate is a `OnceLock` read.
            renderer.set_glyph_census(glyph_trace::wanted());
            attempt.outcome = present_diagnostics::Outcome::FailedRender;
            // **One window's present is one admitted owner-thread door** (`doors::PresentFrame`,
            // a declared batch: compose, configure, acquire, submit, present). The meter enters
            // its station, `RenderCompose`, and puts the funnel's back when it returns — the pair
            // this replaced. The inner phases stay stations. A refusal is the error a failed
            // render takes.
            let outcome = bt_platform::admission::admitted::<
                bt_platform::admission::doors::PresentFrame,
                _,
            >(|token| {
                renderer.present_frame_with_phases(token, gpu, seat_frames, trigger, |phase| {
                    attempt.render_phase(phase);
                    hang_watch::phase(match phase {
                        PresentPhase::SurfaceConfigure(_) => hang_watch::Station::SurfaceConfigure,
                        PresentPhase::ComposeEncode => hang_watch::Station::RenderCompose,
                        PresentPhase::TextShaping => hang_watch::Station::TextShaping,
                        PresentPhase::AtlasUpload => hang_watch::Station::AtlasUpload,
                        PresentPhase::Layout => hang_watch::Station::RenderLayout,
                        PresentPhase::SurfaceAcquire => hang_watch::Station::SurfaceAcquire,
                        PresentPhase::QueueSubmit => hang_watch::Station::QueueSubmit,
                        PresentPhase::Present => hang_watch::Station::SwapchainPresent,
                        PresentPhase::Complete => hang_watch::Station::RenderCompose,
                    });
                })
            })??;
            // **What this frame did with the documents on it** — the one funnel is
            // also the one place that can say it, and it says it only when the
            // answer moved (`BT_PREVIEW_TRACE`).
            preview_trace::frame(
                preview_trace::global(),
                preview,
                renderer.preview_text_frame(),
            );
            // **And what it asked the shared glyph atlas for** (`BT_GLYPH_CENSUS`,
            // `docs/DESIGN.md` §7.1.3m) — the ceiling §7.1.3l could only measure by
            // hand, measurable on the machine that meets it.
            glyph_trace::frame(glyph_trace::global(), census, renderer.glyph_census());
            // A textless present is still a present: an image went to the swapchain,
            // so the composition tree has to publish it or the glass keeps showing a
            // picture the swapchain no longer holds. What that frame *owes* is a
            // different question, and it is answered by the callers below.
            if matches!(
                outcome,
                PresentOutcome::Presented(_) | PresentOutcome::PresentedWithoutText(_)
            ) {
                // **The skirt shrinks in the same commit the picture grows in.**
                // The swapchain has just been reconfigured and presented, so this
                // is the first instant its new rectangle is true; publishing the
                // two apart would either leave the strip transparent for one more
                // frame or lay the window's ground under a clear that already
                // carries it, and a 60% window would read at 84% along the strip
                // (§7.1.6c-4b). The present funnel is the only place both facts
                // are in hand at once, which is why it is here and not beside the
                // resize handler.
                attempt.outcome = present_diagnostics::Outcome::FailedCommit;
                attempt.phase(Some(5));
                let (covered_width, covered_height) = renderer.presented_swapchain_size();
                let committed = (|| {
                    hang_watch::during(hang_watch::Station::CompositorSize, || {
                        compositor
                            .set_covered_size(covered_width, covered_height)
                            .map_err(|error| anyhow!(error))
                            .context("tell the window's ground how much of it the swapchain covers")
                    })?;
                    // The commit is its own owner-thread door, the present's sibling
                    // (`doors::CompositorCommit`; the meter enters its station). A refusal takes
                    // the failed commit's road.
                    bt_platform::admission::admitted::<
                        bt_platform::admission::doors::CompositorCommit,
                        _,
                    >(|token| compositor.commit(token))
                    .unwrap_or_else(|refused| Err(refused.to_string()))
                    .map_err(|error| anyhow!(error))
                    .context("publish the presented frame to the window's composition tree")
                })();
                attempt.phase(None);
                committed?;
                attempt.landed_at = Some(Instant::now());
                // **The frame that withdraws the skirt has to be asked for.** The
                // ground under the strip is sized against the picture of the
                // *previous* present, because a present queues an image rather than
                // showing one, so it can only ever be withdrawn by a later frame —
                // and a window that fell idle the instant a resize ended would have
                // no later frame. Self-terminating: the frame this asks for is the
                // one that makes the answer `false`.
                if compositor.skirt_covers_anything() {
                    window.request_redraw();
                }
            }
            if matches!(outcome, PresentOutcome::Presented(_)) {
                // A successful resize may configure the surface inside this call.
                signature.renderer = renderer.present_state();
                gate.presented(signature);
            }
            attempt.outcome = match &outcome {
                PresentOutcome::Presented(_) => present_diagnostics::Outcome::Presented,
                PresentOutcome::PresentedWithoutText(_) => {
                    present_diagnostics::Outcome::WithoutText
                }
                PresentOutcome::Skipped => present_diagnostics::Outcome::Skipped,
                PresentOutcome::SkippedNotVisible => present_diagnostics::Outcome::NotVisible,
                PresentOutcome::Reconfigure => present_diagnostics::Outcome::Reconfigure,
            };
            Ok(Some(outcome))
        })
    }

    pub(in crate::runtime) fn retained_seats<'a>(
        tab: &'a TabState,
        focused_frame: Option<&'a ViewportFrame>,
        bodies: &[PaneDraw],
        focused_leaf: SeatId,
        viewport: bt_render::SeatViewport,
        focused: bool,
    ) -> (Vec<SeatId>, Vec<bt_render::SeatFrame<'a>>) {
        let mut seat_ids = Vec::with_capacity(bodies.len());
        let mut seat_frames = Vec::with_capacity(bodies.len());
        // **The focused seat frame is pushed only when there is a shell to push
        // one for** (§7.1.6h). On a folder tab the retained window picture is
        // the *previous* tab's, and the fallback rectangle below is the whole
        // viewport — so pushing it would paint another tab's terminal across
        // this one and leave the chrome to cover its own tracks. The list is
        // then simply empty, and `present_frame` composes zero seats and the
        // chrome over them, which is exactly what such a tab is made of.
        //
        // **Each retained picture with the metrics it was presented at** (ticket 37): a frame
        // on the glass is a pair, and drawing it again at the pane's newer metrics would lay
        // its old rows out in cells of another size.
        if let Some((frame, leaf)) = focused_frame.zip(tab.sessions.get(&focused_leaf)) {
            let focused_body = bodies
                .iter()
                .find(|pane| pane.seat == focused_leaf)
                .copied()
                .unwrap_or(PaneDraw {
                    seat: focused_leaf,
                    viewport,
                    clip: viewport,
                });
            seat_ids.push(focused_leaf);
            seat_frames.push(bt_render::SeatFrame {
                seat: focused_body.viewport,
                clip: focused_body.clip,
                frame,
                metrics: leaf.presented_metrics,
                focused,
            });
        }
        // Each unfocused pane's own last picture, for the same reason the
        // focused one's is reused: a pane that has not been given anything new
        // to say is showing what it last said, and re-projecting it would be
        // asking it to say the same thing at the price of a whole capture.
        // A pane that has never presented is simply not drawn this present —
        // it has nothing on the glass to keep.
        for pane in bodies {
            if pane.seat == focused_leaf {
                continue;
            }
            let Some(leaf) = tab.sessions.get(&pane.seat) else {
                continue;
            };
            let Some(projected) = leaf.last_presented_frame.as_ref() else {
                continue;
            };
            seat_ids.push(pane.seat);
            seat_frames.push(bt_render::SeatFrame {
                seat: pane.viewport,
                clip: pane.clip,
                frame: projected,
                metrics: leaf.presented_metrics,
                focused: false,
            });
        }
        (seat_ids, seat_frames)
    }

    /// The same question with the arriving shape handed in (multiwindow slice
    /// F2).
    ///
    /// Split out because across a window boundary the pane is not in this
    /// window's tabs to be found: the subtree travels on the broker
    /// ([`crate::DragBroker::cargo_tree`]) and this window answers about *its* tree with
    /// *its* viewport and *its* setting, which is the only place those three
    /// live. One author for "will it fit", asked by two callers.
    pub(crate) fn arrival_fits(&self, target: TabId, arriving: &bt_layout::LayoutNode) -> bool {
        let Some(into) = self.tab_state(target) else {
            return false;
        };
        into.seats
            .plan_drop(
                &self.seat_metrics(),
                self.window.seat_viewport,
                seats::LayoutAim::Rim(self.append_edge()),
                seats::DropCargo::Layout(arriving),
            )
            .is_some_and(|plan| plan.fits())
    }

    /// The jump flash, as a band across the row it landed on.
    ///
    /// It is read out of the pane's **last presented frame** rather than from the
    /// projection, and that is what makes it honest across a scroll: the frame is
    /// the picture on the glass, its `cell_anchors` say which content each row is
    /// showing, and a row that has since scrolled out simply is not found and is
    /// not drawn. Nothing here re-derives where a row "should" be.
    fn command_flash_layer(
        &self,
        palette: &bt_render::ChromePalette,
        scale: f32,
    ) -> Option<marks::OverlayLayer> {
        let flash = self.window.command_flash.as_ref()?;
        let elapsed = Instant::now().saturating_duration_since(flash.started);
        // **The pane's own ring, before the row's body is asked for** (§7.55 ⑨).
        //
        // Before, and not inside the row's arm with a fallback, because the two
        // are alternatives: the ring is drawn for a pane that has no row worth
        // pointing at, and `command_rail_body` answers `None` for an alternate
        // screen (§3.2 suspends the rail's *picture* there) — which is exactly
        // the case the ring exists for. A ring reached through that `?` would
        // never be drawn at all, which is the report this arm answers.
        let anchor = match &flash.band {
            FlashBand::Pane => {
                let alpha =
                    cmdrail::flash_fade(elapsed, self.app.motion, |x| cubic_bezier(x, EASE))?
                        * cmdrail::JUMP_FLASH_RING_OPACITY;
                let rect = seats::pane_rect(&self.seat_layout, flash.seat)?;
                return Some(marks::OverlayLayer {
                    quads: cmdrail::flash_ring_quads(rect, alpha, scale, palette),
                    ..marks::OverlayLayer::default()
                });
            }
            FlashBand::Row(anchor) => anchor,
        };
        let alpha = cmdrail::flash_alpha(elapsed, self.app.motion, |x| cubic_bezier(x, EASE))?;
        let body = self.command_rail_body(flash.seat)?;
        let frame = self.pane_frame(flash.seat)?;
        let row = frame_row_of_anchor(frame, anchor)?;
        // Padding only, and padding is window chrome: it is the same at every text size, so it
        // is read through the window's named base interface (ticket 37).
        let padding_px = self.window.renderer.base_metrics().padding_px;
        let top =
            body[1] + padding_px + row.top_subpixels as f32 / bt_viewport::SUBPIXELS_PER_PX as f32;
        let bottom = top + row.height_subpixels as f32 / bt_viewport::SUBPIXELS_PER_PX as f32;
        // Clipped to the body, because a row half scrolled past the top of the
        // viewport must flash the half that is there and never a strip of the pane
        // head above it.
        let band = [
            body[0],
            top.max(body[1]),
            // Short of the reserved scroll lane, which is the mock-up's own
            // arrangement seen from the other side: `.term` carries an 18px right
            // padding and the flash is a background on a block inside it, so the
            // band has never reached under the rail. The text half of that
            // arrangement is enforced now (2026-09-23): a pane with a rail is
            // given a grid whose last column ends left of the rail's resting band
            // (`cmdrail::terminal_grid_for`). The band itself is unchanged — it is
            // a row's background, not text, and runs to the lane as it always did.
            body[2] - bt_render::TERMINAL_SCROLL_LANE_LOGICAL_PX * scale,
            bottom.min(body[3]),
        ];
        Some(marks::OverlayLayer {
            quads: cmdrail::flash_quads(band, alpha, scale, palette),
            ..marks::OverlayLayer::default()
        })
    }

    /// Run the plan (M155/M156).
    pub(crate) fn plan_for(&self, inputs: &PlanInputs) -> Option<seats::DropPlan> {
        let aim = inputs.landing.layout_aim()?;
        // **A row's centre verbs move no rectangles at all**, so the layout the
        // drop would make is the layout that is already on screen. That is a
        // *plan* rather than an absence of one, and it has to be: M147 wants the
        // pane named — with a word on it when the verb is real ("Open in this
        // preview") and traced and wordless when it is refused — and the whole
        // of that drawing is `dock_overlay` reading a plan. Running `plan_drop`
        // here instead would ask for `ReplaceSeat`, which swaps the target leaf
        // for a *fresh* one: a new seat id, a lost pin, and a content plane left
        // filed under a pane that no longer exists. Retargeting is not a tree
        // edit, and the plan must not pretend it is one.
        if let Some(payload) = inputs.source.row()
            && let DropLanding::SeatCentre { target } = inputs.landing
        {
            let kind = self.seats.tree().find_seat(target).map(|seat| seat.kind);
            let mut plan =
                self.seats
                    .plan_content_drop(&self.seat_metrics(), inputs.viewport, target)?;
            // **And the same plan carries the 2026-09-16 text verb**, which
            // moves no rectangle either: the box over a terminal's middle is the
            // live layout with `Paste path` written in it, exactly as a
            // preview's is with "Open in this preview". Asking which *verb*
            // rather than comparing against `Retarget` is what keeps the promise
            // on screen and the release reading one table.
            let verb = row_verb(payload.kind, inputs.landing, kind);
            // **A paste with nothing to paste into is refused while the hand is
            // still open** (review 2026-09-17). `row_verb` reads the seat's
            // *kind*, and a terminal seat with no shell behind it is a kind
            // without a session — `paste_paths_into` would find no recipient and
            // return in silence, which is M147's silent refusal exactly. The
            // outline is the answer instead, and it is the same question the
            // release asks, asked by the same function.
            let honoured = verb.content_target() == Some(target)
                && (!matches!(verb, RowVerb::PastePath(_))
                    || self.paste_offer_at(Some(inputs.landing)).is_some());
            if !honoured {
                plan.refuse();
            }
            return Some(plan);
        }
        // An edge or a rim is a space verb whichever source asked for it, so a
        // row arrives here as the one leaf it becomes — a page for a file, a
        // tree for a folder ([`RowPayloadKind::leaf_kind`]). The seat id is a
        // placeholder: `plan_drop` renumbers everything that arrives out of its
        // own counters and reports the pairs in [`seats::DropPlan::arrived`],
        // which is where the commit reads the identity that actually landed.
        let arriving = inputs
            .source
            .row()
            .map(|payload| bt_layout::LayoutNode::seat(row_arrival_seat(payload.kind)));
        let cargo = match (&inputs.cargo, &arriving, &inputs.source) {
            // M156① — a tab arrives as its whole layout, and since §7.1.6k′ so
            // does a pane the spring left behind in another tab. Which of the
            // two this is was decided once, in [`Runtime::plan_inputs_for`].
            (Some(tree), _, _) => seats::DropCargo::Layout(tree),
            (_, Some(leaf), _) => seats::DropCargo::Layout(leaf),
            // M156② — a pane of *this* tree arrives as the seat it already is,
            // fixed column and all. `inputs.cargo` being `None` for a pane is
            // exactly the statement that the seat is one this tree owns.
            (None, None, DragSource::Pane(leaf)) => seats::DropCargo::Pane(leaf.seat),
            (None, None, DragSource::Tab(_) | DragSource::Row(_)) => return None,
        };
        let mut plan = self
            .seats
            .plan_drop(&self.seat_metrics(), inputs.viewport, aim, cargo)?;
        // A drop this window's strip cannot hold is refused here rather than at
        // the release, and that ordering is the whole of M147: refuse at the
        // release and the box stays blue right up until the hand opens on
        // nothing, which is the picture "this app is broken" makes too.
        //
        // The limit is I106's and it is narrow. N161/K125 — a centre that ejects
        // the target pane into the strip — and [`pane_can_become_a_tab`] is what
        // that pane has to answer. A drop on a files column's middle would push a
        // files column out as a tab with nothing running in it. Every other
        // landing here moves panes only inside one tree and asks nothing of the
        // strip.
        //
        // **Asked of the source again since B4** (user ruling 2026-08-25). It
        // was asked of the cargo while "arrives as a subtree" and "evicts the
        // target" were the same fact; the ruling separates them. A pane's centre
        // now trades wherever the pane came from — the displaced pane goes into
        // the slot the traveller vacated and never becomes a tab — so the only
        // hand left that turns a pane out into the strip is a whole tab's, and
        // that is what this asks about.
        if let DropLanding::SeatCentre { target } = inputs.landing
            && matches!(inputs.source, DragSource::Tab(_))
            && !self
                .seats
                .tree()
                .find_seat(target)
                .is_some_and(|seat| pane_can_become_a_tab(seat.kind))
        {
            plan.refuse();
        }
        Some(plan)
    }

    /// **The two facts a pane head's run is drawn and hit-tested by**, read off
    /// this window once (裁4, 2026-08-26).
    ///
    /// One reader for the paint, the fade register and the hit test, on
    /// [`seats::head_run_revealed`]'s own argument: the moment the three of them
    /// assemble this pair for themselves is the moment one of them forgets the
    /// second arm and a head goes dark under its own open menu.
    pub(crate) fn head_run(&self) -> seats::HeadRun {
        seats::HeadRun {
            hovered: self.window.seat_pointer.pane_hover,
            raised: self.head_that_raised_a_layer().map(|layer| layer.seat),
        }
    }

    /// **Which pane head put the layer that is standing on the glass** — the
    /// second of [`Self::head_run`]'s two facts (user ruling 2026-08-27).
    ///
    /// [`popup_owner`]'s own sentence with a seat in it. A layer is not a
    /// rectangle that happens to be floating over this window: it is an
    /// extension of the control that raised it, and the head that raised one is
    /// therefore a head with something of its own on screen — so it goes on
    /// showing its controls, exactly as the surface under any other popup does.
    /// 裁4 (2026-08-26) ruled that for the `⌄`; the ruling that added this
    /// function extends it to the head's other door, whose flyout is dropped in
    /// the same place and reached the same way.
    ///
    /// **Exhaustive over [`HoverFloat`] and over [`float::FloatTrigger`]**, for
    /// [`popup_owner`]'s reason: a fifth layer, or a fourth way to raise a
    /// flyout, does not compile until it has said whether a head is behind it.
    /// That is also why there is no second boolean anywhere — the answer is
    /// derived from what each layer already records about where it came from,
    /// and nothing here asks where a rectangle happens to be.
    pub(crate) fn head_that_raised_a_layer(&self) -> Option<seats::PaneLayer> {
        HoverFloat::ALL.into_iter().find_map(|layer| match layer {
            // Every [`Popup`] at once, because that is how `HoverFloat` carries
            // them — so the arm asks which one, and answers for each. Three of
            // the eight are raised by a control standing in a head; the other
            // five are raised by a tab, a row, a toolbar or a body.
            HoverFloat::Menu => self.popups_up().any().and_then(|popup| match popup {
                // The palette is raised by a chord and hangs off nothing, so
                // there is no head for a hover float to have come out of.
                Popup::Palette => None,
                // The head's own `⌄` — 裁4's case, the one this rule started as,
                // and one of the two doors a lone pane's corner draws.
                Popup::Pane => self.window.pane_menu.as_ref().map(|menu| seats::PaneLayer {
                    seat: menu.seat,
                    door: Some(icons::ActionIcon::OpenPaneMenu),
                }),
                // A files column's root button, which is the leading control of
                // that column's head — not one of the corner's pair, because a
                // files column always wears a head.
                Popup::Root => self
                    .window
                    .root_menu
                    .seat()
                    .map(|seat| seats::PaneLayer { seat, door: None }),
                // A preview head's name, which *is* the switcher when the pool
                // holds more than one buffer.
                Popup::Preview => self
                    .preview_menu_seat()
                    .map(|seat| seats::PaneLayer { seat, door: None }),
                // The `+`'s list, a file row's menu, the graph toolbar's branch
                // filter, a repository row's menu, the terminal's own right
                // press, and the menu a right press on a tab raises. None of
                // them is dropped from a head.
                Popup::Profile
                | Popup::File
                | Popup::GraphFilter
                | Popup::GitMenu
                | Popup::TermMenu
                | Popup::Tab => None,
            }),
            // **The head's 🗀** — and only it. A flyout summoned from a tab's
            // folder hangs off the strip, and one summoned from a folder printed
            // in a pane's own output (§7.29) hangs off the text, which is not
            // the head either. A leaf in some other tab names a seat this tab
            // does not have, so the tab is checked rather than assumed.
            HoverFloat::Flyout => match self.window.float.peek().and_then(|win| win.origin) {
                Some(float::FloatTrigger::Pane(leaf)) => (leaf.tab
                    == self
                        .window
                        .tabs
                        .get(self.window.active_tab)
                        .map(|tab| tab.id)?)
                .then_some(seats::PaneLayer {
                    seat: leaf.seat,
                    door: Some(icons::ActionIcon::OpenFilesPane),
                }),
                Some(float::FloatTrigger::Tab(_) | float::FloatTrigger::Reference { .. })
                | None => None,
            },
            // A file row's glance card and a tab's layout peek are raised by a
            // row and by a tab. Neither is a head, and neither has ever hidden
            // one.
            HoverFloat::Glance | HoverFloat::LayoutPeek => None,
        })
    }

    /// The dirty buffers one gate is about, by name and in the pool's own order.
    ///
    /// Three questions, one list-maker. What differs between them is only *which*
    /// pools are at risk: a pane and a tab put one tab's pool at risk, and a shut
    /// puts every tab's — and the mock-up's own `state.tabs.flatMap(poolDirtyNames)`
    /// says the third in as many words (P125).
    pub(crate) fn gate_dirty_names(&self, request: &restore::GateRequest) -> Vec<String> {
        dirty_gate_names(&self.window.tabs, self.window.active_tab, request)
    }

    /// **The axis a split with no direction of its own takes** (user ruling,
    /// 2026-08-16, `settings.json` v6).
    ///
    /// One reader for every such verb — the duplicate chord, `Split with…`,
    /// `New terminal in folder…`, `Duplicate pane` — because the ruling is that
    /// they all obey one setting, and a second place that read it would be a
    /// second place that could stop.
    ///
    /// `Auto` is the pane's longer side, which is what all four of them did
    /// before the setting existed and what Windows Terminal's `duplicatePane`
    /// means by `split: "auto"`. The measurement is the solver's own rectangle,
    /// never a guess (red line L10).
    ///
    /// The picker's four zones deliberately do not come through here: a zone
    /// *is* a direction, and a setting that overrode the thing the hand just
    /// pointed at would be the setting overruling the gesture.
    pub(crate) fn settings_split_axis(&self, seat: SeatId) -> Axis {
        // The pane is measured whatever the setting says, because measuring is
        // cheap and because a branch that skipped it would make `Auto` the only
        // path that touches the solver — which is how a rectangle nobody asks
        // for goes stale unnoticed.
        split_axis(
            self.app.settings_store.loaded().split_direction,
            self.pane_split_axis(seat),
        )
    }

    /// The seat this window's page is on, when that seat is the one holding the
    /// keyboard.
    ///
    /// **`focused_preview_seat` and never `focused_leaf`**, for
    /// [`Self::page_holds_the_keyboard`]'s reason and found the same way: the
    /// latter only ever names a seat that is in `sessions`, so it is never a
    /// preview seat and a chord routed through it never fires. One predicate,
    /// asked twice, is how the two came to differ in the first place — so this
    /// is now that predicate.
    /// **Docked only, and that is the point of it** (§7.14c). Every caller hangs
    /// something off the pane's own head — a search capsule, an address editor —
    /// and a page carried into a float has no head in the layout to hang it on:
    /// its seat was closed by `pop_out_preview` and has no rectangle at all. So
    /// the page with the keyboard is asked for first, and then whether it still
    /// has a seat; a floated page answers `None` here and holds the keyboard
    /// anyway, which are two different facts and now say so.
    fn focused_web_seat(&self) -> Option<SeatId> {
        let leaf = self.page_with_the_keyboard()?;
        self.float_holding_the_page(leaf)
            .is_none()
            .then_some(leaf.seat)
    }

    /// The same question asked of a drag the caller is holding.
    ///
    /// [`Runtime::release_drag`] takes the drag out of the window before it
    /// decides anything — a gesture that has ended must not be in flight while
    /// its own consequences run — so the commit cannot ask `self.drag` what it
    /// was aiming at. It asks the drag it has.
    pub(crate) fn plan_inputs_for(&self, drag: &Drag) -> Option<PlanInputs> {
        let landing = drag.landing?;
        // The strip's two landings draw themselves in the strip (K124: "the
        // preview in the strip is the ghost now"), so there is no dock box for
        // them and never was one to fade.
        landing.layout_aim()?;
        let showing = self.window.tabs[self.window.active_tab].id;
        // **The one place a source is turned into the *shape* it arrives in**
        // (M156, and §7.1.6k′'s addition to it). `Some` means "a whole subtree
        // arrives and is renumbered on the way in"; `None` means "a seat of the
        // tree being planned changes places and keeps its own name".
        //
        // Written here rather than in [`Runtime::plan_for`] because this is the
        // cache key: the shape is part of what the plan is a function of, and a
        // spring that moves the view mid-gesture changes the shape without
        // changing the landing.
        let cargo = match &drag.source {
            // A row arrives as one leaf, and that leaf is built where the plan
            // is (see [`Runtime::plan_for`]) — a row has no tree of its own to
            // carry, which is the whole difference between it and a tab.
            DragSource::Row(_) => None,
            // **§7.1.6k′ — a pane is one shape at home and the other away.** In
            // the tab on screen it is a seat this tree owns, so it arrives *as
            // itself*: the same `Seat`, the same id, its fixed extent along
            // with it, and a centre that trades payloads rather than evicting
            // anybody (L138). Once the spring has left the hand in one tab and
            // the view in another, that seat is not in the tree being planned
            // at all — it is a subtree from another room, exactly like a tab's,
            // and it arrives the way one does.
            DragSource::Pane(leaf) if leaf.tab == showing => None,
            DragSource::Pane(leaf) => Some(bt_layout::LayoutNode::seat(
                self.tab_state(leaf.tab)?
                    .seats
                    .tree()
                    .find_seat(leaf.seat)?
                    .clone(),
            )),
            DragSource::Tab(id) => Some(self.tab_state(*id)?.seats.tree().clone()),
        };
        Some(PlanInputs {
            landing,
            source: drag.source.clone(),
            tree: self.seats.tree().clone(),
            cargo,
            viewport: self.window.seat_viewport,
            scale_ppm: seats::scale_ppm(self.window.renderer.dpi_milli().get()),
        })
    }

    /// **Spend this window's thumbnail budget for this frame** (§7.1.6b′ F2).
    ///
    /// The four gates live in `focus_thumb`; this is where the first two are
    /// *applied*, because they are the two that need the picture:
    ///
    /// * **The mode** — no focus column, no thumbnails, and the cache is emptied
    ///   rather than left to rot. That is what makes the frame after `Exit` cost
    ///   exactly what a frame cost before F2 existed, and the counter dump says so
    ///   out loud.
    /// * **Visibility** — the cards are filtered on `FocusRailGeometry::viewport`,
    ///   the very rectangle `focus_rail_chrome` clips its drawing to. One number,
    ///   not two agreeing rules: a card that is drawn is a card that is projected,
    ///   and there is no third state where one is true and the other is not.
    ///
    /// Everything past that is handed down unevaluated — a `SeatDemand` per seat
    /// carrying a *reference* to the session or the cache — so the walks that
    /// actually cost something happen on the far side of the damage check and the
    /// throttle, where a gate can still refuse them.
    ///
    /// # And the reads a card arms (user ruling 2026-08-21)
    ///
    /// **A seat projected into a card is on screen, and that is the whole of why
    /// its document may be read.** "A background tab does not load" is older than
    /// this mode and was right when it was written: a tab nobody was looking at
    /// had nothing on the glass. §7.1.6b′ gave that rule a new subject — the card
    /// column puts *every* seat of *every* visible tab on the glass, shrunk — and
    /// what a card then drew for an unread document was a sentence that was never
    /// going to come true: `📄 Loading…` on a files column nobody was walking, a
    /// bare `notes.md / MD` face over a document nobody was reading.
    ///
    /// So the two reads a card can be missing are armed **here**, and here is the
    /// whole of the boundary:
    ///
    /// * **Only in the mode.** The arming is below the gate above, so a window
    ///   that is not in focus mode returns before either list exists and a
    ///   background tab is left exactly as it was.
    /// * **Once.** The document's ledger is the buffer's own
    ///   ([`preview::PreviewBuffer::claim_head_read`]) and the directory's is the
    ///   cache's own (`DirCache::mark_pending`) — the same two ledgers the
    ///   ordinary doors have always written, so a card looked at sixty times a
    ///   second reads each thing once and a refusal is never retried.
    /// * **Never on this thread.** Both go to the workers that already own those
    ///   two lanes; nothing here touches a disk.
    ///
    /// The projection itself is untouched: it still quotes only what is in
    /// memory, and the next pass finds the body there because somebody else put
    /// it there.
    pub(crate) fn refresh_focus_thumbnails(&mut self, now: Instant, scale: f32, why: &'static str) {
        // **This window's id, read before any borrow** (`BT_CARD_TRACE`,
        // §7.1.6b′) — `mouse_trace::window_line`'s own rule: the clamp below
        // stands inside a live `&mut` of this window's tabs, where `&self`
        // cannot be taken, so the number is taken here and carried.
        let window = u64::from(self.window.window.id());
        let geometry = self.focus_rail_geometry_now(now);
        // **The tab holding the pane that is in the air** (缺陷 #189, widened by
        // B2 on 2026-09-01). Its seat is being drawn twice while the gesture
        // lasts — once on its own card and once as the stand-in — and the second
        // of those may be somewhere else in this column, or in **another
        // window's** column entirely. `carried_pane` is what makes the second
        // case answerable: `stand_in_pane` goes quiet the instant the pointer
        // crosses off this glass, which is exactly when the picture is wanted.
        // It is one tab, for the length of one gesture, and the throttle is the
        // same one every other card pays.
        let in_hand = self
            .carried_pane()
            .or_else(|| self.stand_in_pane())
            .map(|leaf| leaf.tab);
        // And the box the picture will be drawn in, when the window it is going
        // into is the only one that has a card (B2) — see [`DragBroker::guest_mini`].
        let guest_mini = self
            .app
            .drag_broker
            .as_ref()
            .filter(|broker| broker.source == self.window_id())
            .and_then(|broker| broker.guest_mini);
        if geometry.is_none() && !(in_hand.is_some() && guest_mini.is_some()) {
            self.window.focus_thumbs.clear();
            // Nothing on the glass can be behind anything, so the clock stops
            // here rather than carrying a debt out of the mode — gate 1's own
            // sentence, said about the frame schedule.
            self.window.cards.nothing_to_draw();
            return;
        }
        // The counters as they stood before this pass, so that what the pass did
        // can be read off the gates themselves — see [`focus_thumb::CardClock`].
        let gates_before = self.window.focus_thumbs.stats();
        // **Which cards are projected this frame, and into what box.** One list
        // rather than two walks, because a window with no column of its own
        // still owes a picture of the pane it is carrying, and a second walk to
        // say so would be a second copy of everything below — the demands, the
        // unread documents, the pages asked for.
        let mut boxes: Vec<(usize, [f32; 4])> = Vec::new();
        if let Some(geometry) = geometry.as_ref() {
            for (index, card) in geometry.cards.iter().enumerate() {
                let Some(tab) = self.window.tabs.get(index) else {
                    continue;
                };
                if in_hand == Some(tab.id) || geometry.card_is_in_view(index) {
                    boxes.push((index, card.mini));
                }
            }
        }
        // The carried pane's tab, when this window drew no card for it — which
        // is every window not in focus mode, and it is most of them. The box is
        // the target's own, at the target's scale, so what is projected is the
        // size it will be drawn: the source has no card to take a size from, and
        // inventing one would be this window guessing at another's column.
        if let Some(tab) = in_hand
            && let Some((width, height)) = guest_mini
            && let Some(index) = self.window.tabs.iter().position(|state| state.id == tab)
            && !boxes.iter().any(|(at, _)| *at == index)
        {
            boxes.push((index, [0.0, 0.0, width, height]));
        }
        for &(index, mini) in &boxes {
            let Some(tab) = self.window.tabs.get_mut(index) else {
                continue;
            };
            for seat in seats::focus_mini_seats(tab.seats.tree(), mini, scale) {
                if seat.kind == SeatKind::Terminal
                    && let Some(leaf) = tab.sessions.get_mut(&seat.id)
                {
                    let rows = focus_thumb::mini_rows(
                        seat.rect,
                        focus_thumb::MiniMetrics::TERM.line_px(scale),
                        scale,
                    );
                    let demand = focus_thumb::SeatDemand {
                        id: seat.id,
                        columns: focus_thumb::mini_columns(
                            seat.rect,
                            self.window.focus_mini_advance,
                            scale,
                        ),
                        rows,
                        source: focus_thumb::SeatSource::Terminal {
                            session: &leaf.session,
                            skip: leaf.card_skip,
                        },
                    };
                    // **`card walk`'s one product call site** (T-CARD-TRACE,
                    // §7.1.6b′). The reason is this pass's caller's — a frame,
                    // or the notch that re-spends the projection — because it is
                    // the one field nothing inside the station can answer.
                    //
                    // The number goes in by value (T-CARD-NO-PASSIVE-CLAMP): a
                    // frame writes a line about this card and never writes the
                    // card's place, so a grid the window wears for an instant on
                    // its way to another display cannot take the reader's place
                    // with it.
                    self.window.focus_thumbs.trace_card_walk(
                        tab.id,
                        &demand,
                        leaf.card_skip,
                        now,
                        card_trace::Card {
                            window,
                            tab: tab.id,
                            seat: seat.id,
                            why,
                        },
                    );
                }
            }
        }
        let mono_advance = self.window.focus_mini_advance;
        let face_advance = self.window.focus_mini_face_advance;
        let mut visible = BTreeSet::new();
        // What the cards could not draw out of memory, gathered by tab index
        // because the sending needs `&mut self` and the walking holds `&self`.
        let mut unread_documents: Vec<(usize, preview::PreviewSource)> = Vec::new();
        let mut unread_dirs: Vec<(usize, focus_thumb::UnreadDir)> = Vec::new();
        // **And the web seats of the cards that are in the column** (§7.11).
        // Gathered for the same reason and spent the same way: the decision is
        // `web_thumb::WebThumbs::due`'s, the asking needs `&mut self`, and the
        // walk below holds the tab list immutably.
        let mut wanted_pictures: Vec<web_thumb::PageDemand> = Vec::new();
        let pages = &self.window.web;
        let page_pictures = &self.window.web_thumbs;
        // **And the pictures the cards can draw out of memory** (user ruling
        // 2026-08-30; §7.1.6b⁵). Read and never asked for: this is the window's
        // decode cache exactly as it stands, and a seat whose file is not in it
        // is the face it has always been. See [`card_picture_in`].
        let decodes = &self.window.peek_cache;
        // **And the tab holding the pane that is in the air** (缺陷 #189). Its
        // seat is being drawn twice while the gesture lasts — see the walk above,
        // which is where a card's visibility and the pane in the hand are
        // reconciled into the one list this loop is handed.
        for (index, mini) in boxes {
            let Some(tab) = self.window.tabs.get(index) else {
                continue;
            };
            visible.insert(tab.id);
            // The mini boxes are solved here rather than inside the budget
            // because they are *geometry*, and geometry is `seats`'s: what comes
            // back is each seat's own rectangle, and each seat's own rectangle is
            // what its column count is cut from — which is §3.3's "the same
            // session at another width" in the only form a 263px-wide card has
            // room for — and, since 2026-08-20, its vertical twin: how many rows
            // that rectangle holds is how many rows it is handed.
            let demands: Vec<focus_thumb::SeatDemand<'_>> =
                seats::focus_mini_seats(tab.seats.tree(), mini, scale)
                    .into_iter()
                    .filter_map(|seat| {
                        // **The document this seat is on, if nobody has read
                        // it.** Asked here rather than inside `mini_source`,
                        // which must stay a projection: it answers what is in
                        // memory and this is the question of what is not.
                        if seat.kind == SeatKind::Preview
                            && let Some(source) = tab.unread_card_document(seat.id)
                        {
                            unread_documents.push((index, source));
                        }
                        // **The page on this seat, if it is one** (§7.11). Two
                        // separate readings, and the split is the same one
                        // `unread_card_document` makes one line above: the
                        // picture it *has* goes into the projection, and the
                        // picture it is *owed* goes on a list the caller acts
                        // on past the walk. Neither is a question the
                        // projection may ask.
                        let leaf = LeafId {
                            tab: tab.id,
                            seat: seat.id,
                        };
                        if let Some(web) = pages.get(&leaf) {
                            wanted_pictures.push(web_thumb::PageDemand {
                                leaf,
                                url: web.identity().unwrap_or_default().to_owned(),
                                facts: web.capture_facts(),
                                card: Some(web_thumb::CardCell {
                                    size: (
                                        (seat.rect[2] - seat.rect[0]).round().max(0.0) as u32,
                                        (seat.rect[3] - seat.rect[1]).round().max(0.0) as u32,
                                    ),
                                    scale,
                                }),
                            });
                        }
                        // **The picture on this seat, if this window has one**
                        // (§7.1.6b⁵) — the third reading in the same place and
                        // the same split as the two above it: what a seat *has*
                        // goes into the projection, and nothing on this line
                        // asks for what it has not.
                        let picture = tab
                            .card_picture_path(seat.id)
                            .and_then(|path| card_picture_in(decodes, path));
                        let source = tab.mini_source(
                            seat.id,
                            seat.kind,
                            page_pictures.picture(leaf),
                            picture,
                        )?;
                        // **The seat's own face decides both counts.** A row's
                        // height and a column's width are two readings of the
                        // same face, so they are taken from one table
                        // (`focus_thumb::MiniMetrics`) rather than chosen here —
                        // which is what stops a seat being measured for thirteen
                        // rows of one size and drawn in another.
                        let metrics = source.metrics();
                        let advance = if metrics.mono {
                            mono_advance
                        } else {
                            face_advance
                        };
                        Some(focus_thumb::SeatDemand {
                            id: seat.id,
                            columns: focus_thumb::mini_columns(seat.rect, advance, scale),
                            rows: focus_thumb::mini_rows(seat.rect, metrics.line_px(scale), scale),
                            source,
                        })
                    })
                    .collect();
            unread_dirs.extend(
                self.window
                    .focus_thumbs
                    .project(tab.id, &demands, now)
                    .into_iter()
                    .map(|ask| (index, ask)),
            );
        }
        self.window.focus_thumbs.retain_visible(&visible);
        // **The card column's frame debt, settled where the cards were actually
        // drawn** (§7.1.6b′, T-5). A pass that got every seat level owes nothing;
        // a pass gate 4 refused a seat on does, because that gate is a *skip* and
        // not a deferral — the seat keeps its old picture with nothing queued to
        // come back for it, and until this line the only thing that ever came back
        // was an integrated shell's 16 ms breath. See [`focus_thumb::CardClock`].
        self.window
            .cards
            .settled(gates_before, self.window.focus_thumbs.stats());
        self.photograph_pages(wanted_pictures, now);
        dump_focus_thumb_frame(
            visible.len(),
            self.window.focus_thumbs.stats(),
            self.window.web_thumbs.stats(),
        );
        self.arm_card_reads(unread_documents, unread_dirs);
    }

    /// **Go and get what the cards could not draw** — the sending half of
    /// [`Self::refresh_focus_thumbnails`]'s ruling, and the only place in this
    /// window where a background tab's content is asked for.
    ///
    /// Split out because the walk above holds the tab list immutably and both
    /// ledgers are written through `&mut`; the lists it carries are the frame's
    /// own and are empty on every frame after the first sight of a card.
    pub(crate) fn arm_card_reads(
        &mut self,
        documents: Vec<(usize, preview::PreviewSource)>,
        dirs: Vec<(usize, focus_thumb::UnreadDir)>,
    ) {
        for (index, source) in documents {
            let Some(tab) = self.window.tabs.get_mut(index) else {
                continue;
            };
            let id = tab.id;
            // The buffer's own ledger says whether this is the first asking, and
            // says it while filing that it was asked (P151's restore, the
            // glance's card and an opened pane all come through the same door).
            let Some(want) = tab
                .preview_pool
                .get_mut(&source)
                .and_then(preview::PreviewBuffer::claim_head_read)
            else {
                continue;
            };
            if !self.app.preview_worker.request(preview::PreviewRequest {
                window: self.window_id(),
                tab: id,
                source,
                want,
            }) {
                self.disable_preview_worker();
                break;
            }
        }
        for (index, ask) in dirs {
            let Some(tab) = self.window.tabs.get_mut(index) else {
                continue;
            };
            let id = tab.id;
            let Some(root) = tab.files.get(&ask.seat).map(|state| state.root.clone()) else {
                continue;
            };
            // Marked before the send, exactly as `files_trees` marks it, so the
            // walk that runs on this very frame sees a question already asked.
            tab.file_trees
                .entry(ask.seat)
                .or_default()
                .mark_pending(&ask.key);
            let request = files::DirRequest {
                window: self.window_id(),
                host: files::FilesHost::Docked(LeafId {
                    tab: id,
                    seat: ask.seat,
                }),
                path: files::full_path(&root, &ask.key),
                key: ask.key,
            };
            if !self.app.files_worker.request(request) {
                self.disable_files_worker();
                break;
            }
        }
    }

    /// A wheel notch over the rail, turned into vertical motion.
    ///
    /// [`Self::scroll_tab_strip`]'s opposite number, and it takes the same notch
    /// distance for the same reason: "a notch that changed length depending on
    /// what it was over is a distance the hand has to relearn at every surface".
    /// What it does not need is that function's axis translation — the rail is a
    /// vertical scroller and a vertical wheel already says what it means.
    /// **Aim one card's terminal seat**, if the pointer is over one (user ruling
    /// 2026-08-21, §7.1.6b′ 「卡片窗口瞄准」). Reports whether the notch was
    /// spent here.
    ///
    /// # Why the wheel, and why no menu
    ///
    /// The thing being set is *where down a screen a window is pointed*, and a
    /// wheel is what this desktop uses to point a window down a screen. A menu
    /// row would have to name a number, and the number a reader wants is the one
    /// where the furniture stops — which they find by watching the card, not by
    /// counting rows in a dialog. So the gesture is the direct one: turn the
    /// wheel over the seat and watch the picture move.
    ///
    /// **Up lifts the window and down lowers it, stopping at the tail.** That is
    /// the same direction the wheel means over any document: away from you is
    /// back through what has already gone past. Zero is a floor and not a wrap —
    /// a reader who keeps turning down lands on the tail and stays there.
    ///
    /// **One detent is one row — and a detent is what the driver adds up to,
    /// not what one report of it says** (user report 2026-08-21). A
    /// high-resolution wheel and a precision touchpad deliver one detent as a
    /// run of small travels, so the fraction is carried between reports in
    /// [`CardAim`] and the row falls on the piece that completes it. The
    /// consequence for *this* function is the order below: the seat is found
    /// **before** the notch is measured, because a report too small to be a
    /// whole row is still this seat's report and must be kept rather than
    /// dropped through to the column's own scrolling. That reading of "spent
    /// here" is now confined to the modified gesture, where it is the only
    /// honest answer — under `Alt` there is no other claimant for the fraction —
    /// and it is exactly what made the *unmodified* wheel unusable while this
    /// function stood on the plain path: see [`column_notch`].
    ///
    /// **The gesture is `Alt`+wheel** ([`column_notch`], user ruling 2026-08-21;
    /// it asked for no modifier for one day, and the correction is recorded
    /// beside the original in §7.1.6b′). A flick of the wheel already arrives as
    /// a merged burst of notches (`queue_wheel`), so crossing a dozen rows of
    /// status bar is one flick rather than a dozen gestures — with `Alt` held
    /// down for it.
    ///
    /// **Only a terminal seat answers.** A files column, a preview and a face
    /// have no tail to be lifted off — what they draw is read from the top
    /// already — so a notch over one of them falls through to the column, which
    /// is what the reader will read it as: they were scrolling the list and the
    /// pointer happened to be over a card.
    ///
    /// **The click table did not change by a character.** `hit_focus_rail` still
    /// answers `Tab(i)` over the whole card and `TabClose(i)` over the ×, and
    /// this function is not on that path: a mini seat is still not a click
    /// target, and the ruling that says so ("clicking a card is clicking that
    /// tab") is about presses, not about notches.
    pub(crate) fn aim_focus_card_window(
        &mut self,
        now: Instant,
        delta: MouseScrollDelta,
    ) -> Result<bool> {
        // **This window's id, read before any borrow** (`BT_MOUSE_TRACE`,
        // §7.60). Every station past the card walk stands inside a live `&mut`
        // of this window's own tabs, where `Runtime::mouse_trace`'s `&self`
        // cannot be taken; `mouse_trace::window_line` is the same door reached
        // with a number instead.
        let window = u64::from(self.window.window.id());
        // A report with no vertical travel in it is not an aim and has nothing
        // to carry — a horizontal wheel over a card is still the column's.
        let notches = wheel_zoom_notches(delta);
        if notches == 0.0 {
            mouse_trace::window_line(window, || {
                format!("wheel_aim leave=no-travel notches={notches}")
            });
            return Ok(false);
        }
        let Some(position) = self.window.pointer_position else {
            mouse_trace::window_line(window, || {
                format!("wheel_aim leave=no-pointer notches={notches}")
            });
            return Ok(false);
        };
        let Some(geometry) = self.focus_rail_geometry_now(now) else {
            mouse_trace::window_line(window, || {
                format!("wheel_aim leave=no-column notches={notches}")
            });
            return Ok(false);
        };
        let [list_top, list_bottom] = geometry.viewport;
        let point = [position.x as f32, position.y as f32];
        if point[1] < list_top || point[1] >= list_bottom {
            mouse_trace::window_line(window, || {
                format!(
                    "wheel_aim leave=outside-viewport point={},{} viewport={list_top},{list_bottom}",
                    point[0], point[1]
                )
            });
            return Ok(false);
        }
        let scale = self.window.renderer.scale_factor() as f32;
        // Read before the walk rather than inside its `else`: what the refusal
        // has to say is how many cards it looked at, and a count is a number
        // where the list itself would be a borrow held across a trace line.
        let cards = geometry.cards.len();
        let Some((index, card)) = geometry
            .cards
            .iter()
            .enumerate()
            .find(|(_, card)| seats::rect_holds(card.body, point[0], point[1]))
        else {
            mouse_trace::window_line(window, || {
                format!(
                    "wheel_aim leave=no-card point={},{} cards={cards}",
                    point[0], point[1]
                )
            });
            return Ok(false);
        };
        let mini = card.mini;
        let tabs = self.window.tabs.len();
        let Some(tab) = self.window.tabs.get_mut(index) else {
            mouse_trace::window_line(window, || {
                format!("wheel_aim leave=no-tab index={index} cards={cards} tabs={tabs}")
            });
            return Ok(false);
        };
        let tab_id = tab.id;
        let Some(seat) = seats::focus_mini_seats(tab.seats.tree(), mini, scale)
            .into_iter()
            .find(|seat| {
                seat.kind == SeatKind::Terminal && seats::rect_holds(seat.rect, point[0], point[1])
            })
        else {
            mouse_trace::window_line(window, || {
                format!(
                    "wheel_aim leave=no-mini-seat index={index} tab={tab_id:?} mini={} point={},{}",
                    mouse_trace::rect_word(mini),
                    point[0],
                    point[1]
                )
            });
            return Ok(false);
        };
        let seat_id = seat.id;
        let Some(leaf) = tab.sessions.get_mut(&seat_id) else {
            mouse_trace::window_line(window, || {
                format!("wheel_aim leave=no-session index={index} tab={tab_id:?} seat={seat_id:?}")
            });
            return Ok(false);
        };
        let target = LeafId {
            tab: tab_id,
            seat: seat_id,
        };
        // **Read before the spend consumes it** (`BT_MOUSE_TRACE`, §7.60): the
        // identity a carry is filed under, and the fraction filed there, are
        // exactly what tells a notch that was carried from one that was aimed at
        // a target the window had stopped recognising.
        let carried_before = self.window.card_aim;
        let skip_before = leaf.card_skip;
        // Whole detents, once whatever this seat is already holding is added in.
        // A report that does not complete one is **spent here anyway** — it is
        // being carried, and handing it on to the column as well would scroll
        // the list with the same turn of the wheel that is aiming a window in it.
        let steps = CardAim::spend(&mut self.window.card_aim, target, delta);
        let carried_after = self.window.card_aim;
        // One builder, three exits: what a reader needs is the same ten fields
        // whether the notch moved the window or was carried, and the two
        // `card_skip` numbers on the line are what say which.
        let aim_line = |skip_after: usize| {
            mouse_trace::WheelAim {
                index,
                tab: format!("{tab_id:?}"),
                seat: format!("{seat_id:?}"),
                // Spelled `TabId(n)/SeatId(n)` rather than `LeafId`'s own
                // `Debug`, whose braces and spaces would put a value with blanks
                // in it on a key=value line.
                at_before: carried_before.map(|aim| format!("{:?}/{:?}", aim.at.tab, aim.at.seat)),
                carried_before: carried_before.map(|aim| aim.carried.delta()),
                steps,
                carried_after: carried_after.map(|aim| aim.carried.delta()),
                skip_before,
                skip_after,
            }
            .line()
        };
        if steps == 0 {
            mouse_trace::window_line(window, || aim_line(skip_before));
            return Ok(true);
        }
        let rows = focus_thumb::mini_rows(
            seat.rect,
            focus_thumb::MiniMetrics::TERM.line_px(scale),
            scale,
        );
        // **The card this notch is about** (`BT_CARD_TRACE`, T-CARD-TRACE). The
        // three ids were read above, before this `&mut` of the tab's sessions;
        // `wheel` is the reason every station under the aim carries, which is
        // how a card's own file is told apart from the frames around it.
        let card = card_trace::Card {
            window,
            tab: tab_id,
            seat: seat_id,
            why: card_trace::why::WHEEL,
        };
        // The wheel trace records the clamped numeric skip on either exit.
        if !aim_card_window(leaf, rows, steps, card) {
            let unchanged = leaf.card_skip;
            mouse_trace::window_line(window, || aim_line(unchanged));
            return Ok(true);
        }
        let aimed = leaf.card_skip;
        mouse_trace::window_line(window, || aim_line(aimed));
        // **The gesture channel** (`focus_thumb`, 2026-08-21): the hand moved
        // this seat's window, so this seat re-projects on the pass below
        // whatever the 10Hz clock would otherwise have said. The credit is one
        // seat's and it is spent by the pass that uses it — the budget gate 4
        // exists to defend is about a *shell* outrunning a 160px card, and it
        // has nothing to say about a wheel a hand is turning.
        self.window.focus_thumbs.unthrottle(tab_id, seat.id);
        // The picture is behind the number, so the projection is spent again
        // before the chrome is built — the order every frame already uses.
        self.refresh_focus_thumbnails(now, scale, card_trace::why::WHEEL);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        // An aim outlives the window, so it goes into `session.json` on the
        // ordinary debounce rather than once per notch.
        self.mark_session_dirty(now);
        Ok(true)
    }
}

/// Apply the whole rows spent by `CardAim` to the leaf's card window.
///
/// `site` names the card and says the notch is what ran the aim —
/// `BT_CARD_TRACE` and nothing else (§7.1.6b′).
pub(crate) fn aim_card_window(
    leaf: &mut LeafSession,
    rows: usize,
    steps: i32,
    site: card_trace::Card,
) -> bool {
    focus_thumb::aim_card_skip(&leaf.session, &mut leaf.card_skip, rows, steps, site)
}

/// **The F2 budget's own probe** — see [`super::floats::dump_chrome_frame`], which it copies
/// down to the environment variable's shape.
///
/// It prints the four counters and the number of cards that were visible, once
/// per frame that spent the budget. That is what makes the ruling's three claims
/// checkable *on a real machine* rather than only in a test: scroll a card out of
/// the column and `visible` drops while `dropped` ticks; leave the window alone
/// and `projections` stops moving while `unchanged` climbs; press `Exit` and the
/// line stops being written at all, because there is no budget left to spend.
pub(crate) fn dump_focus_thumb_frame(
    visible: usize,
    stats: focus_thumb::ThumbStats,
    pages: web_thumb::WebThumbStats,
) {
    // **The page lane on the same line and not on a second switch** (W2 slice
    // ⑥). It is the same budget seen from the one seat whose content this
    // window cannot compute, and a reader watching a card fill in wants the
    // projection counters and the capture counters against one another: a
    // `captures` that never moves beside a `hidden` that climbs is the whole
    // story of a column full of background tabs, and two files could not say it.
    static FOCUS_THUMB_DUMP: trace::Dump = trace::Dump::new(|files| files.focus_thumb.as_deref());
    FOCUS_THUMB_DUMP.line(|| {
        format!(
            "focus-thumb visible={visible} projections={} unchanged={} throttled={} dropped={} \
             captures={} pictures={} page-frames={} page-hidden={} page-closing={} \
             page-blank={} page-inflight={} page-throttled={} page-unchanged={} \
             page-stale={}",
            stats.projections,
            stats.skipped_unchanged,
            stats.skipped_throttled,
            stats.dropped_offscreen,
            pages.captures,
            pages.pictures,
            pages.frames,
            pages.skipped_hidden,
            pages.skipped_closing,
            pages.skipped_blank,
            pages.skipped_in_flight,
            pages.skipped_throttled,
            pages.skipped_unchanged,
            pages.dropped_stale,
        )
    });
}

/// **The pixels a card may draw for one file, and nothing else** (user ruling
/// 2026-08-30; §7.1.6b⁵).
///
/// The whole of the reading half of "a card draws the real thumbnail", and it is
/// a free function over the cache rather than a method on [`Runtime`] for the
/// borrow the frame walk needs: `refresh_focus_thumbnails` holds the tab list
/// immutably while it asks this of the window's decode cache, and a `&self`
/// method would put both borrows on the same object.
///
/// **It asks for nothing.** `Pending`, `Failed` and an absent entry are one
/// answer here — `None`, which the caller reads as "this seat is a face" — and
/// none of the three inserts a `Pending` or sends a task. That is the ruling
/// stated in code: *不起新解码、不拉新帧*. A file the window has decoded for some
/// other surface is free; a file it has not is a face until something the reader
/// did decodes it.
///
/// The key is [`normalized_local_image_path_key`], which is the key the picture
/// lane, the glance card and the animation clock all use — so a card, a pane and
/// a hover over one file are one decode and one texture.
pub(crate) fn card_picture_in<'a>(
    cache: &'a PeekCache,
    path: &Path,
) -> Option<focus_thumb::CardPicture<'a>> {
    match cache.get(&normalized_local_image_path_key(path))? {
        PeekCacheEntry::Ready {
            key,
            rgba,
            width_px,
            height_px,
            ..
        } => Some(focus_thumb::CardPicture {
            key,
            rgba,
            width_px: *width_px,
            height_px: *height_px,
        }),
        PeekCacheEntry::Pending { .. } | PeekCacheEntry::Failed(_) => None,
    }
}

/// **U8, R3 — a mid-FLIP terminal pane's viewport and scissor.**
///
/// `body` is what `pane_body_viewport` answered: the pane's content rectangle at
/// its **final** size, and this function does not change that size — not by a
/// pixel, not at any point of the flight. The extent comes from the solve and
/// only the corner comes from the tween, which is R2 made structural rather than
/// promised: a grid reflow is a function of the extent, so an extent no
/// animation can touch is a ConPTY resize no animation can cause.
///
/// The mock-up gets the same result by composing two transforms — `scale(s)` on
/// the pane and `scale(1/s)` on an inner wrapper (6584-6586) — because CSS can
/// only move a laid-out box by transforming it, so the outer scale is
/// unavoidable and the inner one is there to cancel it. Multiplied out, the pair
/// says exactly what these two rectangles say. Transcribing the CSS literally —
/// scaling this crate's viewport by `s` — would reflow the grid on every frame
/// of the animation and hand ConPTY a resize storm, which is precisely what the
/// counter-scale exists to prevent.
///
/// The viewport can legitimately hang off the surface: a pane growing leftward
/// out of a closed sibling is laid out at its final width from the corner it
/// still occupies, so its right edge is past the window's until it lands. That
/// is legal for a viewport and not for a scissor, which is why only the second
/// of the pair is clamped, and it is clamped where it is validated.
///
/// At rest the transform is the identity, both rectangles are `body`, and every
/// value the renderer sees is the one that was there before U8.
pub(crate) fn animated_pane_viewports(
    body: bt_render::SeatViewport,
    pane: [f32; 4],
    transform: PaneTransform,
) -> (bt_render::SeatViewport, bt_render::SeatViewport) {
    let viewport = bt_render::SeatViewport {
        x: (body.x as f32 + transform.dx).round().max(0.0) as u32,
        y: (body.y as f32 + transform.dy).round().max(0.0) as u32,
        width: body.width,
        height: body.height,
    };
    let content = [
        viewport.x as f32,
        viewport.y as f32,
        (viewport.x + viewport.width) as f32,
        (viewport.y + viewport.height) as f32,
    ];
    // The animating box, cropped to the content it is cropping. Intersecting
    // rather than taking the box whole is what makes this equal `body` at rest:
    // the box is the *whole* pane, head included, and the head is chrome that
    // this seat does not draw.
    //
    // A box that misses its own content entirely is not a state the arithmetic
    // can reach — the box always contains the corner the content is drawn from —
    // so the fallback is the content itself rather than an empty rectangle the
    // scissor would reject.
    let clip = seats::box_intersection(transform.applied_to(pane), content).unwrap_or(content);
    let left = clip[0].round().max(0.0);
    let top = clip[1].round().max(0.0);
    (
        viewport,
        bt_render::SeatViewport {
            x: left as u32,
            y: top as u32,
            width: (clip[2].round() - left).max(1.0) as u32,
            height: (clip[3].round() - top).max(1.0) as u32,
        },
    )
}

/// What a pseudoconsole's answer to "clear your own buffer" means for this window.
///
/// A free function so the one decision in `Runtime::clear_pane_screen` that cannot be seen from
/// the screen is testable on its own: **`Ok(false)` is not a failure**, it is the operating
/// system's inbox pseudoconsole saying it exports no clear at all, and an error is a call that
/// really was made and really did not work. Both mean the same thing to the screen — there is a
/// buffer out there still laid out the old way — and both must therefore stop this window moving
/// any rows.
pub(crate) fn host_screen_after(answer: Result<bool, bt_pty::PtyError>) -> bt_term::HostScreen {
    match answer {
        Ok(true) => bt_term::HostScreen::Cleared,
        Ok(false) => bt_term::HostScreen::Untold,
        Err(error) => {
            // Said rather than swallowed: this window's screen is still going to be right, and
            // the only thing lost is the host's agreement about the rows above.
            crate::diagnostics::stderr_line(format!("BT_CLEAR host buffer not cleared: {error}"));
            bt_term::HostScreen::Untold
        }
    }
}

/// Who owns the rectangle the seats are about to be solved into (user ruling 2026-08-08).
///
/// Judged on **the rectangle the solver actually receives**, never on the `Resized` payload. The
/// two are not the same number: winit announces a transient outer size during startup — measured
/// here as an 826x1271 delivery while the window's own inner size was still 800x1200 — and reading
/// authority off that made a freshly restored session look like a window somebody had dragged.
/// The presentation rectangle is the one the layout is a function of, so it is the one provenance
/// has to be a function of too.
///
/// Three rules, and they are deliberately not symmetric:
///
/// - `None` means the program has a rectangle *pending*: it has just asked the window to be some
///   size and does not yet know what the OS made of the request. The next rectangle to arrive is
///   that answer, whatever it turns out to be, and it is the program's. Claiming this way rather
///   than by recording a size up front is what keeps the rule free of tolerances — the program
///   never has to predict how Windows will round its own request.
/// - A rectangle that differs from the claimed one was not asked for by this process, and taking
///   hold of the window frame is how a user says "this is my size now".
/// - Sovereignty, once taken, is returned only by another claim — never by a later rectangle that
///   happens to land back on the claimed size. A hand that drags a window back through its opening
///   size has not changed its mind about owning it.
///
/// Pure, so the rule is pinned by tests rather than by whatever order a given Windows build
/// chooses to deliver `ScaleFactorChanged` and `Resized` in.
pub(crate) fn size_authority_for_rectangle(
    current: SizePolicy,
    claimed: Option<PhysicalSize<u32>>,
    rectangle: PhysicalSize<u32>,
) -> (SizePolicy, Option<PhysicalSize<u32>>) {
    match claimed {
        None => (current, Some(rectangle)),
        Some(claimed) if claimed != rectangle => (SizePolicy::Sovereign, Some(claimed)),
        Some(claimed) => (current, Some(claimed)),
    }
}

/// **The title of the question a gate puts over these tabs** — the request's own
/// ([`restore::GateRequest::title`]), but for a close that would lose no edits,
/// only saves that have not landed (S-6b): its words are then that, since
/// "discard unsaved changes" would not be true of a clean buffer.
pub(crate) fn gate_title_over(
    tabs: &[TabState],
    active_tab: usize,
    request: &restore::GateRequest,
) -> &'static str {
    match tabs_a_close_loses(tabs, active_tab, request) {
        Some(losing)
            if !losing
                .iter()
                .any(|tab| tab.preview_pool.a_close_loses_edits()) =>
        {
            restore::gate_save_not_landed_title()
        }
        _ => request.title(),
    }
}

/// A pointer put inside a pane's body, answered in that body's own coordinates.
///
/// This says *which pane's coordinates*, and stops there. Which cell those
/// coordinates land on — including what a point in the grid's margin or past the
/// last column means — is [`bt_render::CellMetrics::clamped_hit_test_frame`]'s
/// answer to give, because that is where the padding, the cell width and the row
/// map live. Clamping to a cell here would need a second copy of all three.
///
/// The box is half-open, the same way [`Runtime::pane_hit_context`] tests it: the
/// far edges belong to the next pane, so the last point still inside this one is
/// a unit short of them. A body of no width at all — which no solved pane has —
/// collapses to its origin rather than inverting the range.
pub(crate) fn clamp_into_body(body: bt_render::SeatViewport, x: f64, y: f64) -> (f64, f64) {
    let last_x = f64::from(body.width.saturating_sub(1));
    let last_y = f64::from(body.height.saturating_sub(1));
    (
        (x - f64::from(body.x)).clamp(0.0, last_x),
        (y - f64::from(body.y)).clamp(0.0, last_y),
    )
}

/// `.cmdrail.srch-mode` — whether one pane's rail is carrying search results
/// rather than its command ledger (§7.1.5d, S4).
///
/// **Two conditions, and the second is not the prototype's.** `srchRail()` marks
/// the rail the moment a capsule exists on the pane, so its commands recede at
/// `Ctrl+F`, before a letter is typed — a rail going grey to answer a question
/// nobody has asked, which a reader reads as their history having gone away. An
/// empty field has asked nothing; the takeover begins with the first character.
///
/// A free function so the four cases can be stated without a window: the search is
/// a singleton, so "which pane" is a comparison and not a lookup, and the whole of
/// the mode switch is these two clauses.
pub(crate) fn rail_is_searching(search_seat: Option<SeatId>, query: &str, seat: SeatId) -> bool {
    search_seat == Some(seat) && !query.is_empty()
}

/// Which presentation row of `frame` is showing `anchor`, or `None` when the
/// content it names is not on the glass at all.
///
/// Answered out of the frame's own `cell_anchors`, which is the list the paint and
/// every pointer verb already read: the row a flash lights has to be the row the
/// user is looking at, and a second derivation of "where that content went" is a
/// second thing that can be off by one. An unorderable pair — the alternate
/// screen's isolated namespace — is skipped rather than guessed at, which is the
/// same silence `command_mark_at_or_before` keeps for it.
pub(crate) fn frame_row_of_anchor(
    frame: &ViewportFrame,
    anchor: &bt_doc::ContentAnchor,
) -> Option<bt_viewport::FrameVisualRow> {
    let columns = frame.columns.get() as usize;
    let mut found = None;
    for row in 0..frame.drawable_rows() {
        let start = &frame.cell_anchors.get(row * columns)?.start;
        match bt_doc::compare_anchors(start, anchor) {
            Ok(order) if order.is_le() => found = Some(row),
            // The first row that begins after the anchor ends the search: rows are
            // in document order, so nothing further down can contain it.
            Ok(_) => break,
            Err(_) => continue,
        }
    }
    let row = found?;
    // The greatest row that *begins* at or before the anchor is only the row
    // showing it if the anchor is also within that row's own span. Without this,
    // an anchor below the last drawable row would light the last one.
    let end = &frame.cell_anchors.get(row * columns + columns - 1)?.end;
    bt_doc::compare_anchors(anchor, end)
        .ok()
        .filter(|order| order.is_le())?;
    frame.row_map.get(row).copied()
}

/// **The list first** (user ruling 2026-08-21, the same day's correction of
/// §7.1.6b′ ③).
///
/// The first landing of the aim asked for no modifier and stood at the top of
/// the column's wheel door, on the sentence this house answers everywhere else
/// — *the innermost scroller under the pointer takes the notch*. That sentence
/// was quoted about the wrong pair of things. **Aiming is not a scroller**: it
/// points one seat's window at a row, and it can neither run out of travel nor
/// tell the reader it has. The thing on this surface with content out of sight
/// is the **column**, and a card's mini seat covers all but the few pixels of
/// gap between one card and the next — so the reading that reached the build
/// spent every notch a reader could aim at the list on something else, and the
/// fourth card of four could not be brought on screen at all (user report,
/// with a photograph, 2026-08-21).
///
/// So the plain wheel is the column's, unconditionally and in the same words as
/// every other list in this window, and the aim moves onto a modifier.
///
/// **A column with nothing below the fold does nothing.** The plain notch is not
/// quietly re-read as an aim when the list has no travel left: a gesture that
/// changed meaning with the number of tabs open would be a gesture the reader
/// has to count furniture to predict, and the reader who scrolls a short column
/// is telling us they wanted the list.
///
/// # Why `Alt`
///
/// It is the one modifier this window has not already spent on the wheel.
/// `Shift`+wheel is taken twice over — it is the preview body's *other axis*
/// (`scroll_preview_body`) and it is [`super::mouse::wheel_route`]'s explicit "this notch is
/// mine, not the program's" override, which is §7.1.5f's sentence in the wheel's
/// own words. `Ctrl`+wheel is the pair Windows reports AltGr as when it is
/// joined by `Alt`, and it is also the chord every desktop reserves for scale;
/// this build spends it on nothing today, and leaving it unspent is what keeps
/// that door open. `Alt`+wheel is claimed by nothing in this build: no chrome
/// scroller reads it, and the one place `Alt` reaches a shell at all is the
/// modifier bit of an SGR mouse report — which is a *pane's* business, and the
/// column is chrome, so the two can never be the same notch.
///
/// **`Ctrl+Alt` is not carved out of it.** Discipline ② of the shortcut table
/// forbids `Ctrl+Alt` chords because Windows reports AltGr as that pair and a
/// binding there steals a character from every layout that composes with it. A
/// wheel notch composes no character, so there is nothing for it to steal, and
/// an exception here would only mean that a reader resting a hand on the wrong
/// key gets a gesture that silently stops working.
///
/// # On a Mac it is `⌥`, and it is read off the hand
///
/// **What this is handed is `WindowRuntime::modifiers_held`, not
/// `modifiers`** (T-MAC-LIVE, §13.33 ①). The two differ on exactly one machine
/// and for exactly one key: M1-7 ruled that `Option` is text, so
/// [`crate::input::effective_modifiers`] takes the `Alt` winit reports for it out of
/// the state every chord and every text box reads (§13.13 ③). Handed that
/// state, this function answered `List` to every notch a Mac could produce —
/// the aim was unreachable there from the day the port landed, and no setting
/// the reader could change would have revealed it, because the setting they
/// would have had to change is about what `⌥a` **types**.
///
/// A notch types nothing. So the reading here is the plain one: the aim is the
/// notch turned with `Option` under the hand, whatever the keyboard has been
/// told to make of that key, which is also the only reading that lets the
/// gesture be written down once for both dialects.
pub(crate) fn column_notch(modifiers: ModifiersState) -> ColumnNotch {
    if modifiers.contains(cardhint::CARD_AIM_MODIFIER) {
        ColumnNotch::Aim
    } else {
        ColumnNotch::List
    }
}

/// **U8, P177 — the veil an arriving pane fades in from behind.**
///
/// `if (!b) { … el.style.transition = "opacity .18s ease"; el.style.opacity = "";
/// … return; }` (mock-up 6570-6575). A pane with no "before" rectangle does not
/// FLIP, it fades, and the early `return` on 6573 makes the two *alternatives*
/// rather than stages: a pane that came from somewhere slides, a pane that did
/// not exist appears. [`crate::PaneTween::FadeIn`] already models it and
/// [`PaneMotion::opacity_of`] already answers it; this is the drawing.
///
/// **Ruling — the fade is carried by a veil in the ground colour, not by
/// per-fragment alpha.** The arriving pane is drawn exactly as it is drawn at
/// rest, and its rectangle is then covered by [`ChromePalette::seat_body`] — the
/// palette's own "exactly `--termbg`" — at `1 - opacity`, decaying to nothing.
///
/// Over the ground that is *exactly* CSS `opacity`: the composite
/// `termbg·(1-a) + pane·a` is what an element at opacity `a` over a `--termbg`
/// backdrop produces, and the ground under a pane's box is `--termbg` — it is
/// the terminal pass's own clear colour, and A1 rules that `--panel` shows only
/// in the gap a divider drag opens.
///
/// The literal thing — a uniform alpha on the pane's own draws — is refused, and
/// not for effort. A terminal seat's glyph colours live inside its cosmic-text
/// buffers, and glyphon's `TextArea` carries no uniform alpha multiplier, so a
/// per-seat opacity would mean rebuilding every glyph buffer on every frame of a
/// 180ms fade: a text-relayout storm traded for a resize storm, which is the
/// same bug wearing a different coat, and U8 exists partly to refuse the first
/// one.
///
/// **Where it diverges, stated rather than hidden.** Where a *still-moving
/// neighbour* is crossing the arriving pane's box, the veil covers that
/// neighbour too, whereas CSS would blend the arriving pane over it. What that
/// reads as is "this ground has not settled yet", which is true; what it is not
/// is a pixel-exact transcription. It is also bounded: a fade lasts 180ms of the
/// FLIP's 200ms, only a neighbour whose own flight crosses the new pane's box
/// can be caught by it, and what shows through is the colour that box is about
/// to be anyway.
///
/// One fill per arriving pane, on the seat's own solved rectangle. An arriving
/// pane does not FLIP, so its box *is* the final rect — checked rather than
/// assumed, because a veil hung on a moving box would be a hole travelling
/// across the layout.
pub(crate) fn pane_fade_veil_layers(
    motion: &PaneMotion,
    panes: &[(SeatId, [f32; 4])],
    now: Instant,
    preference: Motion,
    palette: ChromePalette,
) -> Vec<marks::OverlayLayer> {
    let quads: Vec<bt_render::OverlayQuad> = panes
        .iter()
        .filter_map(|&(seat, rect)| {
            let alpha = 1.0 - motion.opacity_of(seat, now, preference);
            if alpha <= 0.0 {
                return None;
            }
            debug_assert_eq!(
                motion.transform_of(seat, now, preference),
                PaneTransform::IDENTITY,
                "a fading pane is never also displaced (mock-up 6573 returns), so \
                 its veil may be hung on the rectangle the solver gave it"
            );
            Some(bt_render::OverlayQuad {
                rect,
                color: palette.seat_body,
                alpha: alpha.clamp(0.0, 1.0),
            })
        })
        .collect();
    if quads.is_empty() {
        return Vec::new();
    }
    vec![marks::OverlayLayer {
        quads,
        ..marks::OverlayLayer::default()
    }]
}

pub(crate) fn risen_frame(frame: [f32; 4], fade: float::FloatFade) -> [f32; 4] {
    [
        frame[0],
        frame[1] - fade.rise,
        frame[2],
        frame[3] - fade.rise,
    ]
}

pub(crate) fn row_arrival_seat(kind: RowPayloadKind) -> bt_layout::Seat {
    bt_layout::Seat::new(bt_layout::SeatId(0), kind.leaf_kind())
}

/// How many panes a persisted tab held — the number its badge would show.
fn persisted_pane_count(node: &LayoutNodeV1) -> usize {
    match node {
        LayoutNodeV1::Leaf(_) => 1,
        LayoutNodeV1::Split(split) => split.children.iter().map(|c| persisted_pane_count(c)).sum(),
    }
}

/// **Which surface each popup is an extension of**, given the surface the tab
/// list is currently drawn on.
///
/// Exhaustive on [`Popup`] for [`crate::PopupsUp::holds`]'s reason: a ninth popup does
/// not compile until it has said where it grows from.
pub(crate) fn popup_owner(popup: Popup, tabs: TabSurface) -> PopupOwner {
    match popup {
        // The `+`'s own `˅`, wherever the `+` is — the strip's, the rail's, or
        // the focus column's. `profile_menu_anchor` hangs the box off that same
        // answer, so the menu and its owner can never name two different
        // buttons.
        Popup::Profile => PopupOwner::Tabs(tabs),
        // **And the menu a right press on a tab raises** (丙2), for the same
        // reason and by the same derivation: it is raised *on the tab list*,
        // wherever that list is being drawn, so a hand that has reached it has
        // not left the rail — and a rail that read the menu beside it as "the
        // pointer has gone" would retract out from under the very list it just
        // grew. That is the 2026-08-25 report exactly, and this line is why it
        // cannot happen a second time.
        Popup::Tab => PopupOwner::Tabs(tabs),
        // Everything else is raised by a press somewhere in the panes: a files
        // column's root button and its rows, a pane head's `⌄`, the commit
        // graph's branch filter, a preview head's switcher, a repository row,
        // and a terminal's own right press.
        //
        // **The palette is on this side despite being raised by no press at
        // all** (DESIGN.md §7.55). It is not an extension of the tab list — it
        // is a box over the stage that happens to be able to *name* tabs — and
        // a rail that held itself out because the palette was up would be
        // holding out for a surface that is nowhere near it.
        Popup::Root
        | Popup::File
        | Popup::Pane
        | Popup::GraphFilter
        | Popup::Preview
        | Popup::GitMenu
        | Popup::TermMenu
        | Popup::Palette => PopupOwner::Stage,
    }
}

/// What one birth is due for now, if anything: a row-waiting birth whose rows `rows_decide`, a
/// locale-waiting birth once `locale_settled` (a reading has finished), a shell birth whose shell
/// has answered.
fn birth_due(
    birth: &PaneBirth,
    rows_decide: &dyn Fn(&LeafSeed) -> bool,
    locale_settled: bool,
) -> Option<BirthDue> {
    match &birth.waiting {
        BirthWait::Programs { .. } => rows_decide(&birth.seed).then_some(BirthDue::Rows),
        BirthWait::Locale => locale_settled.then_some(BirthDue::Rows),
        BirthWait::Shell(landing) => landing.shell.answered().then_some(BirthDue::Shell),
    }
}

/// **The one scan of a pane's births** (round 2): both places a birth can sit — the pane's own
/// and a *Restart shell* successor's — in each of its waits, so no birth is one no landing pass
/// looks at.
pub(crate) fn births_due(
    leaf: &LeafSession,
    rows_decide: &dyn Fn(&LeafSeed) -> bool,
    locale_settled: bool,
) -> Vec<(BirthAt, BirthDue)> {
    [BirthAt::Pane, BirthAt::Successor]
        .into_iter()
        .filter_map(|at| {
            leaf.birth_at(at)
                .and_then(|birth| birth_due(birth, rows_decide, locale_settled))
                .map(|due| (at, due))
        })
        .collect()
}

/// Land the successor's shell, if it answered ([`land_shell_birth`]); a landed or refused
/// successor leaves the pane, which then has no restart in flight.
pub(crate) fn land_successor(
    leaf: &mut LeafSession,
    note: impl FnMut(&str),
) -> Result<SuccessorLanding> {
    let Some(successor) = leaf.successor.as_deref_mut() else {
        return Ok(SuccessorLanding::Waiting);
    };
    if !successor.shell_answered() {
        return Ok(SuccessorLanding::Waiting);
    }
    let refusal = land_shell_birth(successor, note)?;
    let Some(landed) = leaf.successor.take() else {
        return Ok(SuccessorLanding::Waiting);
    };
    Ok(match refusal {
        Some(refusal) => SuccessorLanding::Refused(refusal),
        None => SuccessorLanding::Landed(landed),
    })
}

/// **The ConPTY source the startup trace records for a pane that has just landed** — its
/// shell's, or `not-started` for a birth that failed (round 2: a failed identity shell is a
/// launch shell the trace no longer waits for, and is not a probe's direct input).
pub(crate) fn landed_source(leaf: &LeafSession, refused: bool) -> Option<String> {
    if refused {
        return Some("not-started".to_owned());
    }
    conpty_source_of(Some(leaf))
}

/// **Which of a seat move's re-keyings a page has to follow** (§7.10 ④‴).
///
/// `moves` is the whole transaction — every `(was, now)` pair the gesture
/// produced, in the order it produced them — and this picks out the ones the
/// window's page table has an entry for. A pair whose two halves are equal is
/// not a move at all and is dropped here rather than at
/// [`webhost::WebSeat::rehost`]'s door, which answers `AddressOnly` for it: the
/// point of asking is that a pane which did not travel must not be taken out of
/// the table and put back, because a *trade* interleaves two journeys and the
/// second one's key is the first one's old key.
///
/// A pure function so the rule can be read without a browser: what a page is
/// owed by a move is a fact about the two addresses, and the only thing that
/// needs an engine is the handing over itself.
///
/// **The recording table asks it too** (§7.44 ⑮), which is why `hosted` is a set
/// of leaves and not the page map itself: what this decides is a fact about two
/// addresses and a list of the ones somebody is holding something for, and both
/// window-level tables a pane can be behind — the browser and the decoder — ask
/// exactly that. See [`Runtime::carry_the_recordings_of_moved_panes`].
#[must_use]
pub(crate) fn pages_that_move_with_their_panes(
    hosted: &BTreeSet<LeafId>,
    moves: &[(LeafId, LeafId)],
) -> Vec<(LeafId, LeafId)> {
    moves
        .iter()
        .copied()
        .filter(|(was, now)| was != now && hosted.contains(was))
        .collect()
}

/// **R2's zone, as a decision instead of a method** — everything the icon rail
/// consults to answer "should I be open", with no window and no clock in it.
///
/// `position` is the pointer in physical pixels, `rail_right` the physical x the
/// rail the zone is *aiming* at ends on, and `rail_top` the title bar's lower
/// edge — the rail begins there, so a pointer up in the caption run is not in
/// the rail however far left it is.
///
/// `peek_origin` is the live transient peek's trigger, or `None` when no peek is
/// up. **The trigger and not a flag**, and that is the whole of this function's
/// reason to exist:
///
/// **G102 — the rail has unfinished business.** A rectangle can only ever answer
/// "is the pointer near the rail", and a transient peek is the case where that
/// is the wrong question: a peek hanging off a *rail row* floats out past the
/// rail's own edge, so moving onto the thing the rail just opened would
/// otherwise read as having left it — and the rail would retract out from under
/// it, taking the anchor with it.
///
/// **That clause belongs to [`float::FloatTrigger::Tab`] alone.** A rail row is
/// a tab, and in the vertical layout `hit_rail_chrome` is the only thing that
/// produces `ChromeTarget::TabFiles` — so under an icon rail (the only posture
/// [`Runtime::drive_rail_zone`] runs in) a `Tab` peek *is* a peek hanging off a
/// rail row. A [`float::FloatTrigger::Pane`] peek is the folder button on a
/// terminal pane's own head — or, on a lone pane, the one in its top-right
/// corner (2026-08-25) — out in the panes and nowhere near the rail; it
/// overhangs nothing of the rail's, so the rail has no business being held open
/// by it. Asking only "is a peek up" made every pane head a second, invisible
/// rail trigger: hovering a pane's folder icon opened its flyout *and* rolled
/// the rail out from the left edge, with the pointer never having gone near it
/// (user report, 2026-08-15).
///
/// A **pinned** window holds nothing open whatever it was summoned from, and the
/// mock-up is explicit about why (8735-8738): it has been torn off into a
/// free-floating window, so the rail is free to retract normally. That falls out
/// of the argument rather than being tested here — the caller reads
/// [`float::FloatHost::peek`], which is the transient slot and only ever that.
pub(crate) fn rail_zone_wants_open(
    position: Option<PhysicalPosition<f64>>,
    rail_right: f64,
    rail_top: f64,
    peek_origin: Option<float::FloatTrigger>,
    popup_from_the_rail: bool,
) -> bool {
    let inside = position.is_some_and(|position| position.y >= rail_top && position.x < rail_right);
    inside || matches!(peek_origin, Some(float::FloatTrigger::Tab(_))) || popup_from_the_rail
}

/// **Whether this change of posture leaves the rail's own popups hanging.**
///
/// §7.1.6e″'s third clause — the one that keeps rule ① honest in the other
/// direction. A popup the rail grew holds the panel out for as long as it is up,
/// so the *hover* zone can no longer retract out from under one; what can still
/// take the panel away underneath it is a change of posture, which does not go
/// past the zone at all:
///
/// * **The fold** (`.panel-toggle`, `Ctrl+B`). The rail leaves the screen
///   entirely and `rail_geometry` stops answering, so `profile_menu_anchor` has
///   no box — a menu that is open, draws nothing, and swallows the keyboard.
///   Only the folding direction: un-folding strands nothing.
/// * **The tab list moving house** — `Tab layout` from vertical to horizontal,
///   or `Sidebar` from expanded to icons. The first hands the `+` to the strip
///   and the second rebuilds the rail parked at the top of its list
///   (`set_rail_state`'s "born parked"), and a menu that was the sidebar's is
///   then hanging off a button in another surface or another place.
///
/// Focus mode is deliberately not here: the card column keeps the `+` and its
/// `˅` (§7.1.6b′), so the menu changes owner rather than losing one.
pub(crate) fn rail_change_strands_its_popups(from: seats::RailState, to: seats::RailState) -> bool {
    (from.layout, from.mode) != (to.layout, to.mode) || (to.collapsed && !from.collapsed)
}

/// The rail's own level of the overlay stack — [`seats::WindowChrome::rail`]
/// lifted into the overlay's vocabulary, or nothing when there is no rail.
///
/// **The lift keeps the surface class, and that is the whole of §7.1.6c-4f's
/// second amendment.** The rail is not drawn in the chrome pass at all — R1 puts
/// it on a floating level, because an open icon rail overlaps the terminal for
/// 174 of its 220 pixels and a rail drawn under the panes would simply vanish
/// the moment it opened. So `ChromeSurface` never reached the pipeline that
/// reads it: every rail quad came out of here as an `OverlayQuad { alpha: 1.0 }`,
/// and the one-translucency ruling of 2026-08-18 landed on every band in the
/// window except the one this function carries. Measured at 30% over a bright
/// desktop, the tab strip let it through and the rail did not — a dark column
/// down the left edge of a window you could see the desk through.
///
/// A ground therefore travels the ground channel and everything else stays a
/// quad. The comment this replaces was right about the ink and wrong about only
/// one thing: "chrome fills are opaque by construction" is true of a hairline,
/// a hover pill and a landing wash, and false of the panel they are struck on.
///
/// **The surface's opacity is still the fold** — `.rail { transition: …
/// opacity .18s ease }` with `.window.rail-collapsed .rail { opacity: 0 }`
/// (mock-up 814/823). CSS `opacity` on the element the layer *is*, so the panel
/// and everything standing in it leave together rather than the ground going
/// while the icons stay — and since ticket 46 literally together: the band is
/// one [`bt_render::OverlayGroup`], drawn whole and put back once. Its ground
/// is put back by cross-fade rather than over — see [`bt_render::OverlayGroup`]
/// — which is what lets a *fading* panel still be glass instead of going opaque
/// on its way out.
///
/// Red gate: send the grounds through the quad channel as this used to and the
/// pin below finds the rail's panel among the fills; drop the fold and a
/// collapsing rail pops instead of fading.
/// **How opaque the panel on the left is drawn** — the fold's own number, and a
/// flat `1.0` while the card column is up (§7.1.6b′).
///
/// Its own function because it is a *rule* and not a reading: the fold is the
/// `Sidebar` row's business, the card column is not wired to that row, and the
/// place those two facts meet has to be somewhere a test can stand.
///
/// Red gate: return `fold` unconditionally — which is what this used to do — and
/// [`the_card_column_is_never_faded_by_a_fold_that_is_not_its_own`] goes red, as
/// the screen did: a window whose sidebar was folded away entered focus mode and
/// drew nothing in a card-wide column that was nevertheless solved for, hit-
/// tested and clickable.
pub(crate) fn panel_opacity(state: seats::RailState, fold: f32) -> f32 {
    if state.draws_focus_rail() { 1.0 } else { fold }
}

/// The whole geometry decision [`Runtime::resize_leaves_to_layout`] makes, with
/// no renderer and no session in it: seats and a solved layout in, one rectangle
/// per terminal leaf out.
///
/// `focused` enters the answer **only as a flag**. It says which leaf's carry
/// goes through the ConPTY quiet window; it does not and must not reach any
/// `body`. Sizing a shell is a question about a
/// rectangle, and which pane holds the keyboard is not one of that rectangle's
/// inputs — the version of this code that let the two mix is the one that told a
/// narrow focused pane it had the primary pane's columns.
///
/// **A seat that is not being shown as a pane is not in the plan at all**, which
/// is [`seats::shell_body_viewport`]'s whole subject: an unplaced seat has no
/// rectangle, and a seat the concession ladder folded into a bar has one that is
/// chrome. Either way the leaf keeps the width it last really had until a later
/// solve puts a pane back under it.
pub(crate) fn leaf_resize_plan(
    seats: &seats::Seats,
    layout: &SeatLayout,
    focused: SeatId,
    scale: f32,
) -> Vec<LeafResizeTarget> {
    seats
        .terminals()
        .into_iter()
        .filter_map(|seat| {
            seats::shell_body_viewport(seats, layout, seat, scale).map(|body| LeafResizeTarget {
                seat,
                body,
                focused: seat == focused,
            })
        })
        .collect()
}

/// One scroll offset, measured at one scale, said again in another's pixels —
/// see [`Runtime::restate_panel_scroll`] for what it is for and why it is a
/// ratio.
///
/// Its own function so the property can be tested against the very arithmetic
/// the window runs: what a column looks like after a display change is a fact
/// about this number and the geometry it is handed to, and a test that
/// multiplied by its own ratio would be a test of itself.
pub(crate) fn restated_scroll(scroll: f32, measured_at: f64, now_at: f64) -> f32 {
    scroll * (now_at / measured_at) as f32
}

/// **N157/K123 — tear a pane out of its tab and into one of its own.**
///
/// `detachLeaf` and `extractPaneToTab` as one verb, because they are one gesture:
/// the leaf leaves the tree, its sibling is promoted and the run it left is
/// re-balanced (G79/G80, all of which `close_seat` already does), and the pane
/// arrives in the strip as a tab holding the shell it was already running.
///
/// **N158 — the new tab is unpinned, and the ruling's own argument is why.** Pin
/// follows content only when the content was moved *for* you; here you aimed at
/// the strip and made a new tab, so the new tab is simply unpinned. It is the
/// exact opposite of [`Runtime::absorb_tab`]'s ejected pane (N160②), which was
/// displaced rather than aimed, and the distinction is the ruling's rather than
/// this function's.
///
/// The [`Seat`] is cloned *before* the tree edit, because after `close_seat` the
/// tree no longer has it to be asked about and everything durable about the pane
/// — its kind, its fixed extent — lives on it (§5).
///
/// **The tab it returns is already wearing `.landing`** (N157, mock-up 3514:
/// `renderWithPaneFlip(before)` and then the 200ms wash on the tab that just
/// appeared). It is started here rather than at the window's insertion because
/// it is a fact about the gesture, not about the plumbing — and because this is
/// the arrival that most needs announcing: every other tab that appears in the
/// strip was dragged there under your eye, whereas N157 deliberately leaves you
/// in the layout you were in, so without the wash a tab merely *exists* that did
/// not a frame ago, in a run you are not looking at.
///
/// `None` when the seat is not in this tree, or when it is the last one (G84 —
/// `close_seat` refuses to empty a tree, so the gesture is a no-op rather than a
/// tab that closes behind your back). The tree is untouched on both paths.
///
/// There used to be a third: "or when it holds no session". §7.1.6h removed it
/// along with the premise — a pane with no session is now a tab with no session,
/// and the only kind still turned away is turned away one step further upstream,
/// by [`pane_can_become_a_tab`], before the gesture is ever offered.
pub(crate) fn tear_pane_into_tab(
    from: &mut TabState,
    metrics: &SeatMetrics,
    seat: SeatId,
    id: TabId,
    now: Instant,
    motion: Motion,
    solve: impl FnOnce(&seats::Seats) -> (SeatLayout, Option<seats::FitOverflow>),
) -> Option<TabState> {
    let seat = from.seats.tree().find_seat(seat)?.clone();
    if !from.seats.close_seat(metrics, seat.id) {
        return None;
    }
    let pinned = seed::PinMigration::DraggedOutToNewTab.resulting_pin(from.pinned);
    let mut torn = pane_into_new_tab(from, &seat, id, pinned, solve)?;
    torn.landing.start(now, motion);
    Some(torn)
}

pub(crate) fn rail_overlay_layer(rail: &seats::ChromeGroup, fold: f32) -> marks::Band {
    if rail.quads.is_empty()
        && rail.labels.is_empty()
        && rail.sprites.is_empty()
        && rail.images.is_empty()
    {
        return marks::Band::default();
    }
    let (grounds, quads): (Vec<bt_render::ChromeQuad>, Vec<bt_render::ChromeQuad>) = rail
        .quads
        .iter()
        .partition(|quad| quad.surface == bt_render::ChromeSurface::Ground);
    let panel = marks::OverlayLayer {
        grounds: grounds
            .into_iter()
            .map(|quad| bt_render::OverlayGround {
                rect: quad.rect,
                color: quad.color,
            })
            .collect(),
        quads: quads
            .into_iter()
            .map(|quad| bt_render::OverlayQuad {
                rect: quad.rect,
                color: quad.color,
                alpha: 1.0,
            })
            .collect(),
        labels: rail.labels.clone(),
        sprites: rail.sprites.clone(),
        // The cards' page pictures, which are not marks and are never
        // rasterized — see `seats::ChromeGroup::images`. They join the icon
        // channel *after* the column's own marks, which is the order they must
        // paint in: a picture is content and the marks around it are chrome.
        images: rail.images.clone(),
        ..marks::OverlayLayer::default()
    };
    // The fold is the panel's CSS `opacity`, so it is the surface's: the panel
    // and everything standing in it drawn whole and put back once (ticket 46).
    // The ground cross-fades on the way back — it is the window — and the shade
    // it casts on the panes is laid over them; see `bt_render::OverlayGroup`.
    marks::Band::surface(vec![panel], fold.clamp(0.0, 1.0), [0.0, 0.0])
}

/// **The ending step: every road that ends a pane retires its shell and expires its attention
/// exactly once.**
///
/// Five doors take it, and nothing else ends a pane: a pane closed on its own (`close_pane`, with
/// `only` naming it), a tab closed (`close_tab`), a window closed or retired by the quit
/// (`let_go_of_this_window`), a window a refused move had opened
/// (`abandon_a_window_nothing_arrived_in`), and the launch placeholder taken away once the restored
/// tabs stand (`answer_restore`). Each leaf of `tabs` — every one, or the one `only` names, a pane
/// still in birth included — is handed `LeafGone` and then has its shell retired, in that order.
///
/// **Exactly once** is two rules, both kept here. A leaf a walk has already ended is not handed
/// `LeafGone` again ([`LeafSession::attention_ended`]): a window closed and then retired by a quit
/// before it is reaped is walked twice. And a leaf `moving` names — the cargo of a cross-window
/// handover from this window that is decided and not yet performed — is not ending, so its place
/// stays; its `LeafGone` comes from whichever road ends it in the window it lands in. Its shell is
/// still retired, as it was before this step existed (finding F-2 of
/// T-ATTENTION-WINDOW-EXPIRY, left to the shell's own lifecycle).
///
/// Answers the places it gave up, in walk order.
pub(crate) fn end_leaves_of<'t>(
    tabs: impl IntoIterator<Item = (usize, &'t mut TabState)>,
    only: Option<SeatId>,
    moving: Option<&DragSource>,
    reach: impl Fn(usize) -> attention::Reach,
    next_place: &mut attention::Places,
    now: Instant,
    trace: Option<&attention_trace::Trace>,
) -> Vec<attention::Site> {
    let mut ended = Vec::new();
    for (index, tab) in tabs {
        let id = tab.id;
        let reach = reach(index);
        for (seat, leaf) in tab.leaves_mut() {
            if only.is_some_and(|only| only != *seat) {
                continue;
            }
            let moves = moving.is_some_and(|cargo| match cargo {
                DragSource::Tab(tab) => *tab == id,
                DragSource::Pane(pane) => pane.tab == id && pane.seat == *seat,
                DragSource::Row(_) => false,
            });
            if !moves && !leaf.attention_ended {
                let at = attention::Site {
                    tab: index,
                    seat: *seat,
                };
                expire_leaf_attention(at, reach, leaf, next_place, now, trace);
                leaf.attention_ended = true;
                ended.push(at);
            }
            if let Some(pty) = leaf.pty.take() {
                bt_pty::retire_session(pty);
            }
        }
    }
    ended
}
