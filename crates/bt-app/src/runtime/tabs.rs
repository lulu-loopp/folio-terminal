//! `tabs` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{
    BlankPage, BlankPageReturn, Drag, DragCarry, DragHandover, DragSource, DropLanding, Fading,
    FolderPick, FormulaSwitches, HandoverInto, LeafId, LeafSeed, MathHoverExit, MenuPaint,
    NewWindowPlan, PaneArrival, PaneMotion, Popup, PreviewRestore, PreviewSurface, RenameExit,
    RenameSubject, RowPayload, RowPayloadKind, Runtime, TabCarry, TabClick, TabCloseAction, TabId,
    TabMenuState, TabPress, TabRename, TabSeed, TabState, TabSurface, TablePaint, TearOut,
    absorb_tab_into_layout, absorb_tab_into_strip, attention, blank_page_return, create_tab_state,
    expire_leaf_attention, float, native_window, new_tab_cwd, notify, pane_can_become_a_tab,
    pane_into_tab, pane_strip_landing, presentation_physical_size, profiles,
    recoverable_wheel_scroll_amount, restore, row_strip_landing, scrollback_quota, seats, seed,
    settling, solve_seats, stepped_tab, strip_insert_slot, tab_close_action, tab_surface,
    tear_pane_into_tab, two_tabs_mut, webnav,
};
use anyhow::Context;
use anyhow::{Result, anyhow};
use bt_layout::SeatId;
use bt_math::{MathRaster, MathRenderError};
use bt_render::{FrameSource, FrameTrigger, Travel};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
use winit::dpi::PhysicalPosition;
use winit::event::{KeyEvent, MouseScrollDelta};
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowId;

impl Runtime<'_> {
    /// The `+`'s verb: a tab on the default profile, which is what the button's
    /// own tooltip promises in the mock-up (`New tab (${defaultProfile().title})`).
    ///
    /// `Ctrl+Shift+N` is the same verb through the same door (mock-up 6034,
    /// `docs/DESIGN.md` §247): the chord opens the default and never the picker,
    /// so there is one sentence about what "new tab" means rather than a button's
    /// and a key's.
    pub(crate) fn new_tab(&mut self) -> Result<()> {
        let profile = self.default_profile_id();
        self.new_tab_with_profile(&profile, None)
    }

    /// **A tab, seeded from a leaf the caller names** (丙2, `Duplicate tab`).
    ///
    /// Every door onto a new tab used to be [`Self::new_tab_with_profile`], and
    /// that function took its seed from *the pane you are looking at* — which is
    /// the right question for the `+`, for a picker row and for
    /// `New terminal in folder…`, because all three are asked from where the
    /// reader is standing. `Duplicate tab` is asked about a tab that is very
    /// often **not** the one on screen: a right press on a tab does not activate
    /// it (that is the ruling this menu is built on), so the seed has to travel
    /// with the subject rather than be re-derived from the focus.
    ///
    /// So the seeding source is a parameter and the old signature is a wrapper
    /// that supplies the focused leaf — one implementation of "make a tab",
    /// which is §7.1.6e's rule, rather than a second one that would drift the
    /// day the seed grows a third field.
    ///
    /// **The pair travels together and is taken from one leaf**, which is the
    /// rule [`new_tab_cwd`] already states: a profile from one pane and a folder
    /// from another describes a pane that does not exist.
    pub(crate) fn new_tab_seeded_from(
        &mut self,
        profile: &str,
        place: Option<PathBuf>,
        source_profile: &str,
        source_cwd: Option<PathBuf>,
    ) -> Result<()> {
        // No assertion that the table still holds this id, and that is the point
        // of the id: `Duplicate tab` names the profile the source pane is
        // *running*, and a reader may have deleted that row while the pane went
        // on running it. The seed carries the id either way and the spawn
        // degrades on it once, where the reader can be told (`startable_profile`).
        let render_physical =
            presentation_physical_size(self.window.renderer.presentation_geometry());
        let wake = &self.window.pty_wake;
        let id = self.app.tab_ids.mint();
        let cwd = new_tab_cwd(
            profile,
            place.as_deref(),
            source_profile,
            source_cwd.as_deref(),
        );
        // A lone terminal is `Seats::lone_terminal`'s own seat id, so the map is
        // that one entry — or empty, when the shell you are looking at has never
        // named a folder.
        let seats = seats::Seats::lone_terminal();
        let leaves = BTreeMap::from([(
            seats.identity(),
            LeafSeed {
                profile: profile.to_owned(),
                cwd,
                unknown_profile_id: None,
                card_skip: 0,
                prefill: None,
            },
        )]);
        let (tab, _) = create_tab_state(
            id,
            seats,
            &self.window.renderer,
            render_physical,
            wake,
            None,
            &leaves,
            // A new tab is one terminal and nothing else — no files leaf to
            // root, so nothing to say about one, and no preview pane either.
            &BTreeMap::new(),
            &PreviewRestore::default(),
            TabSeed::default(),
            &self.app.profile_programs,
            &self.default_profile_id(),
            self.window.size_policy,
            // The posture and not the stored preference, for
            // [`Self::resolve_seat_layout`]'s reason: a tab born while the card
            // column is up is born into a stage that starts a column-width
            // further in.
            self.rail_posture(),
            self.platform_chrome(),
            FormulaSwitches::from_settings(self.app.settings_store.loaded()),
            scrollback_quota(self.app.settings_store.loaded().scrollback_lines),
            self.app.settings_store.loaded().line_wrapping,
        )?;
        self.window.tabs.push(tab);
        self.apply_window_min_inner_size()?;
        self.activate_tab(self.window.tabs.len() - 1, true)
    }

    /// Scroll the strip until `index` is wholly on screen, and report whether
    /// anything moved.
    ///
    /// The verb that needs it is activation: a tab you have just switched to, or
    /// have just made, is the one thing in the strip that must not be off-screen.
    /// Everything else the strip does about scrolling, it does because the wheel
    /// asked.
    pub(crate) fn reveal_tab(&mut self, index: usize) -> bool {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let width = self
            .window
            .renderer
            .presentation_geometry()
            .swapchain_size
            .0 as f32;
        let scrolled = seats::tab_scroll_to_reveal(
            width,
            scale,
            self.platform_chrome(),
            self.window.tabs.len(),
            self.window.active_tab,
            self.window.tab_scroll,
            index,
        );
        let moved = scrolled != self.window.tab_scroll;
        self.window.tab_scroll = scrolled;
        moved
    }

    pub(crate) fn activate_tab(&mut self, index: usize, force: bool) -> Result<()> {
        if index >= self.window.tabs.len() || (!force && index == self.window.active_tab) {
            return Ok(());
        }
        // **A formula's hover goes with the tab it was in** (audit 2026-09-15,
        // RB-3), and **before** the assignment below: `set_hovered_math` sweeps
        // the *active* tab's leaves, so a clear run after this line would sweep
        // the tab arriving and leave the block in the tab departing still lit —
        // it would be wearing its ground on the day the reader came back to it.
        //
        // Above `hover_pane = None` a dozen lines down for the same reason it is
        // above the assignment: the sweep asks which pane the pointer is in.
        self.leave_hovered_math(Instant::now(), MathHoverExit::BandLeftTheScreen)?;
        self.window.active_tab = index;
        // Looking at a tab is what answers every claim it was making, so the
        // dot goes out here — the unread mark, the bell and the failure all at
        // once, because "the user has now seen this tab" is one event and not
        // three. Ordered with the assignment above, not with the drawing below:
        // the strip is rebuilt from this state at the end of this function, and
        // a tab that became active while still counting as unread would flash
        // its own dot on the way in.
        self.window.tabs[index].mark_seen();
        // **And the same look, put to the ledger, which answers it with nothing** (`attention` plan
        // §10.9, red line 11). It is written rather than left out because the two mechanisms are
        // one sentence apart and the difference between them is the whole ruling: a look spends a
        // *latch*, and a standing request is not a latch — it is a sentence the program is still
        // saying, and only the program or an answer ends it. 看一眼阻塞的 agent 不解除阻塞.
        self.mark_attention_seen(index);
        // Ordered after the assignment on purpose: the tab being revealed is the
        // active one, and an active tab is measured with the skirt only an active
        // tab has.
        self.reveal_tab(index);
        let _ = self.window.pending_frames.take();
        self.window.last_presented_frame = None;
        self.window.preedit = None;
        self.window.mouse_route = None;
        self.window.divider_drag = None;
        self.window.seat_pointer = seats::ChromePointer::default();
        self.window.hyperlink_hover.clear();
        self.window.peek_hover.clear();
        self.window.renderer.set_peek_overlay(None);
        self.window.hover_pane = None;
        self.window.underlined_image_reference = None;
        // **A tab switch takes every menu with it** (user ruling 2026-08-25,
        // B10). A popup is a question about the surface it was raised over, and
        // this line is that surface leaving the glass.
        //
        // It had to be said out loud because §7.1.5a′ had already made the
        // symptom invisible: each of the eight folds to `None` off screen, so a
        // `⌄` left open on the tab you stepped away from drew nothing, took no
        // key, and was still open when you came back. The state outlived the
        // gesture that raised it and there was no frame in between that said so.
        self.close_every_popup();
        // **A tab switch closes the capsule and keeps what was typed** (D-8,
        // user ruling 2026-08-16, which is the prototype's behaviour said out
        // loud rather than a change to it).
        //
        // Closes, because "one search, one pane" cannot survive a capsule left
        // standing on a pane that is no longer on screen: its hits would go on
        // being rebuilt against a transcript nobody is looking at, and coming
        // back to the tab would find a count that had moved for reasons the
        // reader never saw. Keeps the query, because `close` does not touch the
        // field — so `Ctrl+F` on the way back is a continuation and not a fresh
        // start, which is exactly what a "staying state" owes a reader who
        // stepped away for a moment.
        let _ = self.close_search();
        // **U8 — you do not glide a layout you were not looking at.**
        //
        // A FLIP is the difference between where a pane *was drawn* and where it
        // belongs, and the tab arriving was not drawn: whatever split, close or
        // drop happened in it happened off screen, possibly minutes ago and
        // possibly at another window size. Carrying the tween across would open
        // the tab on rectangles nobody has ever seen and glide them to the ones
        // the solver has. The revision is adopted rather than reset, so the next
        // structural edit in *this* tab is the next thing that animates.
        self.window.pane_motion = PaneMotion::default();
        self.window.pane_motion_revision = self.seats.structure_revision();
        let render_physical =
            presentation_physical_size(self.window.renderer.presentation_geometry());
        self.resolve_seat_layout(render_physical);
        self.resize_leaves_to_layout(Instant::now(), "resize activated tab to its seat layout")?;
        self.sync_math_layout_key();
        self.window.window.set_title(&self.display_title());
        self.refresh_chrome();
        self.mark_session_dirty(Instant::now());
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })
    }

    pub(crate) fn close_tab(&mut self, index: usize) -> Result<()> {
        if index >= self.window.tabs.len() {
            return Ok(());
        }
        // **Gate ② (P124)** — "the tab owns its buffer pool; the pool dies with
        // it", and the mock-up's note beside it records that this hole predates
        // the pool: closing a tab used to discard a dirty preview without a word.
        if self.raise_dirty_gate(restore::GateRequest::CloseTab(index))? {
            return Ok(());
        }
        // Item 6, asked on the way *in* rather than on the way out. There is no
        // tab left afterwards to ask, and the fact worth catching is that the tab
        // being taken apart was whole when it got here — `retire_all_shells`
        // walks `sessions`, so a shell that had come adrift from the tree would
        // be a ConPTY closed for a pane nobody could see, or one left running.
        debug_assert!(
            self.window.tabs[index].sessions_match_terminals(),
            "item 6: a tab closes with its shells still matching its tree"
        );
        // A tab that is going away takes its editor and its press with it. The
        // name is committed first rather than dropped, because closing is a blur
        // like any other and the seed the vault is about to record reads
        // `manual_name` — "输入到一半关掉,新名字进 Recent" is the same promise
        // §7.1.4 makes about closing the window.
        if self.window.rename.as_ref().is_some_and(|editor| {
            self.window
                .tabs
                .get(index)
                .is_some_and(|tab| Some(tab.id) == editor.tab())
        }) {
            self.finish_rename(RenameExit::Blur)?;
        }
        if self.window.tab_press.is_some_and(|press| {
            self.window
                .tabs
                .get(index)
                .is_some_and(|tab| tab.id == press.tab)
        }) {
            self.window.tab_press = None;
        }
        if self.window.drag.as_ref().is_some_and(|drag| {
            self.window
                .tabs
                .get(index)
                .is_some_and(|tab| drag.tab() == Some(tab.id))
        }) {
            self.window.drag = None;
            // F2: the payload stopped existing, so the application's pointer has
            // nothing left to broker either.
            self.app.drag_broker = None;
        }
        match tab_close_action(self.window.tabs.len(), self.window.active_tab, index) {
            TabCloseAction::CloseWindow => {
                let native = native_window(&self.window.window)?;
                bt_platform::request_window_close(native)
                    .map_err(|error| anyhow!(error))
                    .context("request close after the final tab")?;
            }
            TabCloseAction::Keep { active_tab } => {
                let was_active = index == self.window.active_tab;
                // The one regular write path into the vault: closing is what
                // fills Recent (mock-up 3929). It happens before the tab is
                // taken apart, because the seed is read off the live session.
                //
                // A tab whose identity pane is a leaf this build cannot read
                // seeds nothing ([`TabState::seed`]) and therefore records
                // nothing: an unwritable row is not written rather than written
                // as a guess.
                let pages = self.window.tabs[index].preview_pages();
                if let Some(seed) = self.window.tabs[index].seed() {
                    self.app.recent.record(seed, pages, SystemTime::now());
                }
                let mut removed = self.window.tabs.remove(index);
                // Every place this tab's panes held, given up before its shells are — the same
                // door `close_pane` uses, asked of every leaf because a tab closes all of them.
                let reach = notify::desktop_reach(was_active, self.window.place());
                let now = Instant::now();
                for (seat, leaf) in removed.leaves_mut() {
                    expire_leaf_attention(
                        attention::Site {
                            tab: index,
                            seat: *seat,
                        },
                        reach,
                        leaf,
                        &mut self.window.attention_next_place,
                        now,
                    );
                }
                // Every leaf's shell, not the focused one's. Reaching for
                // `removed.pty` went through the deref and closed exactly one of
                // them — see [`TabState::retire_all_shells`] for the ConPTY a
                // two-pane tab used to leak on the way out, and for why closing
                // this tab does not wait for any of them to die.
                removed.retire_all_shells();
                self.window.active_tab = active_tab;
                self.apply_window_min_inner_size()?;
                if was_active {
                    self.activate_tab(active_tab, true)?;
                } else {
                    self.refresh_chrome();
                    self.mark_session_dirty(Instant::now());
                    self.present_chrome_change()?;
                }
            }
        }
        Ok(())
    }

    /// Fit the open editor's draft into the box the strip has for it.
    ///
    /// The last step of building the strip rather than part of the loop above,
    /// because it is the one piece of tab content that depends on the strip's
    /// own geometry: the draft scrolls to keep its caret in sight, and "in
    /// sight" is a width that only exists once every tab has been given its
    /// share of the run. The measuring is here, beside the renderer, for exactly
    /// the reason the badge's is — only the font knows how wide a word is.
    pub(crate) fn measure_open_rename(
        &mut self,
        tabs: &mut [seats::TabContent],
        scale: f32,
        width: f32,
    ) {
        let Some(tab_id) = self.window.rename.as_ref().and_then(TabRename::tab) else {
            return;
        };
        let Some(index) = self.window.tabs.iter().position(|tab| tab.id == tab_id) else {
            return;
        };
        let trailers = tabs.iter().map(|tab| tab.trailer).collect::<Vec<_>>();
        // **The box, on whichever axis the tab list is on.** The editor's window
        // onto its own draft is measured against the run it will actually be
        // drawn in, and the two axes give it very different ones: a strip tab is
        // as wide as its share of the run, a rail row is as wide as the panel
        // minus its trailing cluster. Measured against the strip's box while the
        // rail was on screen, the caret walked out of a box that was not there.
        //
        // Both branches answer the same pair — **the box the editor is drawn in**
        // and what size its text is set in — so everything below is one
        // arithmetic that never learns which axis it ran on. The whole box and
        // not its width, because the caret published to the IME needs the box's
        // top and bottom as well: the candidate list has to stand under the tab,
        // not over the letters.
        let measured = match self.window.rail.layout {
            seats::TabLayoutMode::Vertical => {
                let (_, height) = self.window.renderer.presentation_geometry().swapchain_size;
                let pinned = seats::pinned_run_len(&trailers);
                seats::rail_geometry(
                    height as f32,
                    scale,
                    self.platform_chrome(),
                    &trailers,
                    pinned,
                    self.window.rail_scroll,
                    self.sampled_rail(Instant::now()),
                )
                .and_then(|geometry| {
                    let row = geometry.tabs.get(index)?;
                    let content = tabs.get(index)?;
                    let right = seats::rail_title_right(
                        row,
                        content.pane_count,
                        content.badge_text_width,
                        scale,
                    );
                    (right > row.title[0]).then_some((
                        [row.title[0], row.body[1], right, row.body[3]],
                        bt_render::RAIL_TAB_FONT_LOGICAL_PX * scale,
                    ))
                })
            }
            seats::TabLayoutMode::Horizontal => {
                let geometry = seats::tab_strip_geometry(
                    width,
                    scale,
                    self.platform_chrome(),
                    &trailers,
                    self.window.active_tab,
                    self.window.tab_scroll,
                );
                geometry.tabs.get(index).and_then(|geometry_tab| {
                    let content = tabs.get(index)?;
                    let title_box = seats::tab_title_box(
                        geometry_tab,
                        content.pane_count,
                        content.badge_text_width,
                        scale,
                    )?;
                    Some((title_box, bt_render::WINDOW_TAB_FONT_LOGICAL_PX * scale))
                })
            }
        };
        let Some(content) = tabs.get_mut(index) else {
            return;
        };
        let Some((title_box, font_px)) = measured else {
            // A squeezed tab draws no title, so there is no box to be the editor
            // and nothing to show. The draft is not lost — it is still in
            // `self.rename`, and widening the window brings it back mid-word. A
            // rail row whose trailing cluster has eaten the whole run is the same
            // sentence on the other axis.
            content.edit = None;
            // And an editor with no box has no caret to publish: the IME is told
            // so rather than left holding the last rectangle this tab had.
            self.window.rename_caret_line = None;
            return;
        };
        let box_width = title_box[2] - title_box[0];
        let caret_width = (seats::TAB_RENAME_CARET_LOGICAL_PX * scale)
            .round()
            .max(1.0);
        // Disjoint fields, split by hand: the editor owns where its window
        // starts and the renderer owns how wide a string is, and this is the one
        // place the two have to meet.
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let Some(editor) = self.window.rename.as_mut() else {
            return;
        };
        let mut shape = |text: &str| renderer.chrome_text_advances(gpu, text, font_px);
        let fitted = editor.fit(box_width, caret_width, true, &mut shape);
        let caret_px = fitted.caret_px;
        content.edit = Some(seats::TabEdit {
            placeholder: content
                .edit
                .take()
                .map(|edit| edit.placeholder)
                .unwrap_or_default(),
            caret_lit: self.window.rename_blink.visible(),
            ..fitted
        });
        // **Written here because here is the only place the box exists.** The
        // editor's rectangle is a function of the strip's own solve, which is
        // built and thrown away inside this pass; every other field in this
        // window can re-derive its caret from a layout the window keeps, and this
        // one cannot. So the painter records the line it drew and the IME reads
        // it — one derivation still, taken at the moment it is true.
        let x = (title_box[0] + caret_px).min(title_box[2] - caret_width);
        self.window.rename_caret_line = Some([x, title_box[1], x + caret_width, title_box[3]]);
    }

    /// **K124/N157 — the pane's stand-in in the strip**, and the slot it takes.
    ///
    /// `showDropPreview` (mock-up 6507-6546), which dresses the stand-in as the
    /// tab the pane *would become* — its own mark, its own short name — rather
    /// than as a blank gap. That is the whole reason the ghost goes transparent
    /// over the strip ([`DropLanding::shows_itself`]): the thing under the
    /// pointer and the thing in the slot would otherwise be two labels saying one
    /// name, and only one of them is saying where.
    ///
    /// It wears no pin, no `×` and no pane badge, and none of that is suppressed
    /// here: a stand-in is never the active tab and never hovered, and the strip
    /// already draws those three only for tabs that are. What it does say is
    /// [`seats::TabContent::landing`] at full strength, and that is a reuse rather
    /// than a coincidence — `.drop-preview` and `@keyframes tab-land`'s `from`
    /// are the same two declarations in the mock-up, an accent wash at 9% behind
    /// an inset accent ring at 45%. The slot the drop will fill and the tab that
    /// has just filled it are the same picture, which is what makes the landing
    /// read as the thing you were dragging coming to rest.
    pub(crate) fn strip_stand_in(&self) -> Option<(usize, seats::TabContent)> {
        // **F2 — a visitor's stand-in, dressed out of the label that travelled.**
        //
        // The same slot, the same wash, the same picture; the only difference is
        // that the mark and the name arrive on the broker
        // ([`GhostFace`]) rather than being read out of a tab this window holds,
        // because the tab this window is about to be given is still in another
        // window's hands. Checked first because the two are never both set: the
        // pointer is over exactly one window.
        if let Some(foreign) = self.window.foreign.as_ref() {
            let DropLanding::StripExtract { slot } = foreign.landing? else {
                return None;
            };
            return Some((
                slot.min(self.window.tabs.len()),
                seats::TabContent {
                    title: foreign.face.text.clone(),
                    // Zero for a tab that does not exist here yet, exactly as the
                    // local stand-in answers zero — both draw nothing, and zero is
                    // the honest one.
                    pane_count: 0,
                    mark_kind: foreign.face.mark,
                    ..seats::TabContent::default()
                },
            ));
        }
        let drag = self.window.drag.as_ref()?;
        let DropLanding::StripExtract { slot } = drag.landing? else {
            return None;
        };
        // **§7.1.1 — a row's stand-in is dressed as the tab it is about to
        // become**, which for a path is the whole of what there is to say: the
        // file's or folder's own name, and the mark of the one leaf that tab
        // will hold. `pane_mark` is read here for the same reason the pane arm
        // reads it — the ghost above the slot reads the very same call
        // ([`Runtime::drag_label`]), so the two are one picture of one payload.
        if let DragSource::Row(payload) = &drag.source {
            return Some((
                slot.min(self.window.tabs.len()),
                seats::TabContent {
                    title: payload.name.clone(),
                    pane_count: 0,
                    mark_kind: seats::pane_mark(
                        payload.kind.leaf_kind(),
                        None,
                        bt_render::chrome_palette(),
                    )
                    .0,
                    ..seats::TabContent::default()
                },
            ));
        }
        let DragSource::Pane(leaf) = drag.source else {
            return None;
        };
        // **Through the pane's own tab** (§7.1.6k): the spring may have moved the
        // view, and a stand-in dressed out of the tab now on screen would be
        // wearing a stranger's name — or, more likely, nothing at all.
        let tab = self.tab_state(leaf.tab)?;
        let seat = leaf.seat;
        let kind = tab.seats.tree().find_seat(seat)?.kind;
        // The name of the seat being torn out, from that seat's *own* shell.
        // Read through the tab it is leaving, and by id: the stand-in is dressed
        // as the tab this pane would become, and a pane that came back wearing a
        // sibling's name would be the drag pointing at the wrong room.
        let name = tab.terminal_name(seat);
        // And the same question asked of a files column, which answers with its
        // root rather than with its kind — otherwise a tree being torn out is
        // dressed as the word "Files" while the pane it came from says where it
        // is rooted, and the stand-in stops being a picture of the thing.
        let files_name = tab.files_head_name(seat);
        // And a preview's, by the same id and the same argument: a pane torn out
        // wearing a *sibling* preview's file name is the drag pointing at the
        // wrong document.
        let title = tab.preview_head_name(seat);
        Some((
            slot.min(self.window.tabs.len()),
            seats::TabContent {
                title: seats::seat_short_caption(
                    kind,
                    title.as_deref(),
                    name.as_deref(),
                    files_name.as_deref(),
                )
                .to_owned(),
                // A pane torn into its own tab holds exactly one pane, and the
                // badge is for tabs that hold more than one (A2/C27). Zero rather
                // than one only because the count is of a tab that does not exist
                // yet; both answers draw nothing, and zero is the honest one.
                pane_count: 0,
                // "Its own mark", in `showDropPreview`'s own words — the shell
                // this pane is running, not the shell its old tab was named
                // after. The ghost riding the pointer reads the very same
                // `pane_mark` (see [`Runtime::drag_label`]), and the two are one
                // picture of one pane: a stand-in wearing a different mark from
                // the ghost above it would be the strip and the pointer naming
                // two different shells for one drag.
                mark_kind: seats::pane_mark(
                    kind,
                    self.sessions
                        .get(&seat)
                        .map(|leaf| profiles::mark(profiles::index_of_id(&leaf.profile))),
                    bt_render::chrome_palette(),
                )
                .0,
                ..seats::TabContent::default()
            },
        ))
    }

    /// **How many slots this window's tab list is holding for something that is
    /// not a tab** (缺陷 #189) — nought or one, and one exactly while
    /// [`Runtime::strip_stand_in`] is dressing a guest.
    ///
    /// Read off that function rather than off the drag, and that is the whole
    /// reason it exists: the stand-in has four ways not to be there — no drag
    /// and no visitor, a landing that is not [`DropLanding::StripExtract`], a
    /// source that is not a pane, a tab or a seat that has gone under the
    /// gesture — and a second reading of any of them is a scroller that reserves
    /// room for a card nobody drew, or draws a card the scroller cannot reach.
    /// The dressing costs one short string on the frames a drag is actually in
    /// flight and nothing at all on every other frame, because `strip_stand_in`
    /// returns at its first two lines when no hand is over this window.
    pub(crate) fn strip_guests(&self) -> usize {
        usize::from(self.strip_stand_in().is_some())
    }

    /// **Which surface this window is drawing its tab list on right now** —
    /// [`tab_surface`] asked of this window's posture.
    ///
    /// Of the *posture* and not of the stored preference, for
    /// [`Self::rail_posture`]'s own reason: a window in focus mode carries the
    /// `+` on the card column whatever `Tab layout` says.
    pub(crate) fn tab_surface_now(&self) -> TabSurface {
        let posture = self.rail_posture();
        tab_surface(posture.draws_focus_rail(), posture.layout)
    }

    /// Point the "Tables" switch at `enabled` (2026-08-18).
    ///
    /// Presentation, so it is [`Self::apply_display_formulas`] exactly: every pane in every tab,
    /// an immediate write, and one frame's cost. Every session keeps the tables it has proven —
    /// the switch only decides whether a proven one is allowed to stand over its own pipe text —
    /// so turning it off shows the text again and turning it on brings the same blocks back with
    /// nothing re-scanned.
    pub(crate) fn apply_tables(&mut self, enabled: bool) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.tables = enabled;
        if !self.app.settings_store.store(settings) {
            return Ok(false);
        }
        for tab in &mut self.window.tabs {
            for (_, leaf) in tab.leaves_mut() {
                leaf.session.set_table_bands(enabled);
            }
        }
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        Ok(true)
    }

    /// The stamp a table picture laid out now would carry.
    fn table_paint_stamp(
        &self,
        palette: &bt_render::ChromePalette,
    ) -> (u32, [u8; 3], [u8; 3], [u8; 3]) {
        (
            self.window.renderer.metrics().font_size_px.to_bits(),
            palette.preview_grid_line,
            palette.preview_table_head_text,
            palette.files_row_hover,
        )
    }

    /// Make the renderer's table pictures agree with the tables actually on the glass.
    ///
    /// Three things at once, and they are one pass because they are one question — *which pictures
    /// does the next frame need, and are the ones we have still the right ones*: a table no frame
    /// shows any more is dropped, a table whose type size or palette has moved under it is laid
    /// out again, and a table nothing has drawn yet is laid out for the first time. The renderer is
    /// only told when something actually moved, so a window full of unchanged tables costs one map
    /// walk a frame and no allocation at all.
    pub(crate) fn refresh_table_paints(&mut self, sources: &[String]) {
        let palette = bt_render::chrome_palette();
        let stamp = self.table_paint_stamp(&palette);
        let mut changed = false;
        if self.window.table_paints.len() != sources.len()
            || sources
                .iter()
                .any(|source| !self.window.table_paints.contains_key(source))
        {
            self.window
                .table_paints
                .retain(|source, _| sources.contains(source));
            changed = true;
        }
        let stale: Vec<String> = sources
            .iter()
            .filter(|source| {
                self.window
                    .table_paints
                    .get(*source)
                    .is_none_or(|held| held.stamp != stamp)
            })
            .cloned()
            .collect();
        for source in stale {
            let Some(block) = self.build_table_block(&source) else {
                continue;
            };
            self.window.table_paints.insert(
                source,
                TablePaint {
                    paint: block.paint,
                    stamp,
                },
            );
            changed = true;
        }
        if changed {
            let paints = self
                .window
                .table_paints
                .iter()
                .map(|(source, held)| (source.clone(), held.paint.clone()))
                .collect();
            self.window.renderer.set_table_blocks(paints);
        }
    }

    /// Every rendered table a set of frames is showing, by source.
    pub(crate) fn table_sources<'a>(
        frames: impl Iterator<Item = &'a bt_viewport::ViewportFrame>,
    ) -> Vec<String> {
        let mut sources: Vec<String> = frames
            .flat_map(|frame| frame.math_blocks.iter())
            .filter(|placement| {
                placement.artifact.kind == bt_viewport::RgbaArtifactKind::Table
                    && placement.display == bt_viewport::MathBlockDisplay::Rendered
            })
            .map(|placement| placement.artifact.source.clone())
            .collect();
        sources.sort();
        sources.dedup();
        sources
    }

    /// A proven table's "raster": its extent, and no pixels.
    ///
    /// The decoration pipeline asks a worker for a picture and gets back a size and some bytes;
    /// this answers the size half honestly and the bytes half with nothing, which is exactly what
    /// a table is — see [`bt_render::TableBlockPaint`]. The size is what the block reports as its
    /// artifact extent, so it is what decides how many transcript rows the block covers, and it is
    /// measured with the same shaper that will draw it.
    pub(crate) fn table_raster(
        &mut self,
        source: &str,
    ) -> std::result::Result<MathRaster, MathRenderError> {
        let block = self
            .build_table_block(source)
            .ok_or(MathRenderError::NotDetected)?;
        Ok(MathRaster {
            rgba: Vec::new(),
            width_px: block.width_px,
            height_px: block.height_px,
            content_height_px: block.height_px,
            ascent_px: 0.0,
            descent_px: 0.0,
            baseline_px: 0.0,
            render_time: Duration::ZERO,
            inline_runs: Vec::new(),
        })
    }

    /// Ctrl+Tab / Ctrl+Shift+Tab, wrapping at both ends.
    pub(crate) fn step_tab(&mut self, forward: bool) -> Result<()> {
        match stepped_tab(self.window.tabs.len(), self.window.active_tab, forward) {
            Some(index) => self.activate_tab(index, false),
            None => Ok(()),
        }
    }

    /// **How far each tab's chrome has changed hands this frame** — the ground,
    /// the ring, the title and the small marks of the tab that has just become
    /// the active one, and of the one that has just stopped being it
    /// ([`bt_render::TAB_ACTIVATION`]).
    ///
    /// **Keyed by [`TabId`] inside and by index outside**, and the translation
    /// happens here because this is the one place that can do it: a strip is a
    /// list of positions and a fade belongs to a *tab*, so a register keyed by
    /// index would hand the colour to whichever tab slid into the slot when one
    /// was closed. The index is resolved from the same `self.window.tabs` the
    /// chrome is about to be built from, on this frame, so the two cannot
    /// disagree.
    ///
    /// **A tab that has been closed mid-handover is forgotten rather than
    /// eased**: there is no row left on the glass for a fade home to be drawn
    /// on, which is `settle_git_pending`'s argument one surface along.
    pub(crate) fn settled_tab_ink(&mut self, now: Instant) -> Vec<(usize, f32)> {
        let motion = self.app.motion;
        let active = self
            .window
            .tabs
            .get(self.window.active_tab)
            .map(|tab| tab.id);
        let places: BTreeMap<TabId, usize> = self
            .window
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| (tab.id, index))
            .collect();
        let mut wanted: Vec<TabId> = self
            .window
            .settling
            .held()
            .into_iter()
            .filter_map(|key| match key {
                Fading::TabActive(tab) => Some(tab),
                _ => None,
            })
            .collect();
        for gone in wanted
            .iter()
            .copied()
            .filter(|tab| !places.contains_key(tab))
            .collect::<Vec<_>>()
        {
            self.window.settling.forget(&Fading::TabActive(gone));
        }
        wanted.retain(|tab| places.contains_key(tab));
        if let Some(tab) = active
            && !wanted.contains(&tab)
        {
            wanted.push(tab);
        }
        wanted
            .into_iter()
            .map(|tab| {
                let ink = self.window.settling.settle(
                    &Fading::TabActive(tab),
                    0.0,
                    settling::Toward::eased(
                        f32::from(u8::from(active == Some(tab))),
                        bt_render::TAB_ACTIVATION,
                    ),
                    now,
                    motion,
                );
                (places[&tab], ink)
            })
            .collect()
    }

    /// What the tab under the menu can answer for, as of now.
    ///
    /// Read once, when the menu is raised, and carried in [`TabMenuState`] from
    /// then on — see [`TermMenuState`] for why a snapshot rather than a live
    /// read. It is sharper here than it is there: the strip *reorders itself*
    /// while a menu stands over it (a drag, a pin, a tab arriving from another
    /// window), and a row that changed its word or its greying under a
    /// descending hand would be this menu rewriting the sentence the reader is
    /// halfway through answering.
    fn tab_menu_subject(&self, tab: TabId) -> profiles::TabMenuSubject {
        let state = self.tab_state(tab);
        profiles::TabMenuSubject {
            pinned: state.is_some_and(|state| state.pinned),
            // **The same question `Duplicate tab` will ask when it runs**, asked
            // of the same map: a tab made of a folder or a file has no shell
            // behind it (§7.1.6h), so there is no profile and no working folder
            // for a copy to be seeded from. One door, so a greyed row and a
            // declined verb can never disagree about which tabs can be copied.
            can_duplicate: state.is_some_and(|state| state.focused().is_some()),
        }
    }

    /// Raise the menu a right press on a tab asked for.
    ///
    /// E61 first: the opener closes every other popup. **And nothing else** —
    /// in particular the tab is neither focused nor activated, which is the
    /// whole of what makes the menu's subject worth carrying. A right press is
    /// not a way of choosing a tab; it is a way of asking about one.
    ///
    /// A tab that is not in this window's strip raises nothing. The gesture
    /// cannot produce one today — the target came from a hit test over the strip
    /// this window is drawing — and it is refused here rather than trusted,
    /// because everything downstream is written against an id that may stop
    /// naming a tab at any moment and this is the one place that can say the
    /// menu never should have opened at all.
    pub(crate) fn open_tab_menu_at(
        &mut self,
        tab: TabId,
        position: PhysicalPosition<f64>,
    ) -> Result<()> {
        if self.tab_slot_of(tab).is_none() {
            return Ok(());
        }
        self.close_popups_except(Popup::Tab);
        self.window.tab_menu = Some(TabMenuState {
            point: [position.x as f32, position.y as f32],
            tab,
            subject: self.tab_menu_subject(tab),
            hover: None,
            submenu_open: false,
            pointer_was: None,
            submenu_hold_until: None,
        });
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Where the tab menu is, if one is up.
    ///
    /// [`Runtime::term_menu_layout`]'s twin, anchored the same way: at the point
    /// the pointer was at, which no re-layout, no strip scroll and no reorder can
    /// move or destroy.
    pub(crate) fn tab_menu_layout(&mut self) -> Option<profiles::TabMenuLayout> {
        let menu = self.window.tab_menu.as_ref()?;
        let (point, subject, submenu_open) = (menu.point, menu.subject, menu.submenu_open);
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let windows = self.other_window_rows();
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        Some(profiles::tab_menu_layout(
            point,
            (width as f32, height as f32),
            scale,
            subject,
            submenu_open,
            &windows,
            &mut measure,
        ))
    }

    /// The tab menu's own level of the overlay stack, or nothing when none is
    /// up.
    pub(crate) fn tab_menu_layer(&mut self) -> MenuPaint {
        let Some(layout) = self.tab_menu_layout() else {
            return MenuPaint::none();
        };
        let Some(hover) = self.window.tab_menu.as_ref().map(|menu| menu.hover) else {
            return MenuPaint::none();
        };
        let travel = layout.travel();
        // The child's own direction, or the parent's for the frames where there
        // is no child to have one — an empty band reads neither.
        let child_travel = layout.submenu_travel().unwrap_or(Travel::Right);
        let windows = self.other_window_rows();
        let programs = &self.app.profile_programs;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        let mut layers = profiles::tab_menu_build(&layout, hover, &windows, programs, &mut measure);
        // `push_submenu`'s seam again: the menu on the first layer, the child on
        // the ones after it. Splitting there is what gives the two their own
        // clocks.
        let child = layers.split_off(1);
        MenuPaint {
            menu: layers,
            travel,
            child: Some((child, child_travel)),
        }
    }

    pub(crate) fn close_tab_menu(&mut self) -> Result<bool> {
        if self.window.tab_menu.take().is_none() {
            return Ok(false);
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// Open or shut this menu's window list, and report whether anything moved.
    ///
    /// [`Runtime::set_term_submenu`]'s twin on the third door, with that
    /// function's own guard against opening a child the parent has no heading
    /// for: with no other window there is no `Move to window ▸` row at all
    /// (`profiles::TabMenuRow::rows`), so a `→` that opened one anyway would hang
    /// a list off a row that is not on the glass.
    pub(crate) fn set_tab_submenu(&mut self, open: bool) -> Result<bool> {
        let elsewhere = !self.other_window_ids().is_empty();
        let Some(menu) = self.window.tab_menu.as_mut() else {
            return Ok(false);
        };
        if menu.submenu_open == open || (open && !elsewhere) {
            return Ok(false);
        }
        menu.submenu_open = open;
        menu.submenu_hold_until = None;
        // The highlight follows the surface it is on: opening lands on the first
        // window, closing takes it back to the heading it came from, so `←`
        // leaves the keyboard somewhere rather than nowhere.
        menu.hover = Some(if open {
            profiles::TabMenuHover::Submenu(0)
        } else {
            profiles::TabMenuHover::Row(profiles::TabMenuRow::MoveToWindow)
        });
        // **The ring goes out with the list** (B9): a highlighted window with no
        // menu naming it is a window wearing a mark nobody can explain. Armed
        // again by the first hover inside the list that has just opened.
        self.aim_at_window(None);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// **The tab menu's hover, with the safety triangle in it** (#53) — the pane
    /// menu's own four steps, on the third door, and with B9's ring on top of
    /// them.
    pub(crate) fn drive_tab_menu_hover(&mut self, position: PhysicalPosition<f64>) -> Result<bool> {
        let Some(layout) = self.tab_menu_layout() else {
            return Ok(false);
        };
        let hit = profiles::tab_menu_hit(&layout, position.x, position.y);
        let submenu = layout.submenu_frame();
        let to = [position.x as f32, position.y as f32];
        let now = Instant::now();
        let heading = profiles::TabMenuHit::Row(profiles::TabMenuRow::MoveToWindow);
        let Some(menu) = self.window.tab_menu.as_mut() else {
            return Ok(false);
        };
        // **On the child at all**, which is a question about its frame and not
        // about its rows — `profiles::TabMenuLayout::on_submenu` carries the
        // 2026-08-19 report that made it one. The heading keeps its place beside
        // it: a hand back on the row the child hangs from has not left the child
        // either.
        let hovering_child = layout.on_submenu(to[0], to[1]) || hit == Some(heading);
        let was_open = menu.submenu_open;
        let mut held = false;
        if let Some(submenu) = submenu
            && !hovering_child
        {
            let from = menu.pointer_was.unwrap_or(to);
            if profiles::safe_triangle_holds(from, to, submenu) {
                let until = *menu
                    .submenu_hold_until
                    .get_or_insert(now + profiles::SUBMENU_SAFE_HOLD);
                held = now < until;
            }
            if !held {
                menu.submenu_open = false;
                menu.submenu_hold_until = None;
            }
        } else {
            menu.submenu_hold_until = None;
        }
        // The apex only moves when the triangle is not holding — a triangle
        // re-drawn from each intermediate position narrows to nothing and the
        // whole rule evaporates halfway across.
        if !held {
            menu.pointer_was = Some(to);
        }
        let hovered = match hit {
            Some(profiles::TabMenuHit::Row(row)) => Some(profiles::TabMenuHover::Row(row)),
            Some(profiles::TabMenuHit::Submenu(index)) => {
                Some(profiles::TabMenuHover::Submenu(index))
            }
            // The padding, the rule, a greyed row, and everywhere outside:
            // nothing is lit. A menu whose last-hovered row stayed lit while the
            // pointer sat in its own margin would be a menu Enter could fire from
            // a place that looks idle.
            Some(profiles::TabMenuHit::Surface) | None => None,
        };
        let mut changed = menu.submenu_open != was_open;
        if !held && menu.hover != hovered {
            menu.hover = hovered;
            changed = true;
        }
        // Resting on the heading opens the child, on the same 250ms every `⌄` in
        // the house takes. Armed here and matured in `advance_tab_menu`.
        if hit == Some(heading) && !menu.submenu_open {
            menu.submenu_hold_until
                .get_or_insert(now + profiles::CHEVRON_HOVER_OPEN_DELAY);
        }
        // **The ring is the hover, seen from the other window** (B9). Read off
        // the highlight rather than off the hit, so the keyboard's walk lights
        // the same window the pointer's would.
        let ring = match menu.hover {
            Some(profiles::TabMenuHover::Submenu(at)) => Some(at),
            _ => None,
        };
        let inside = hit.is_some();
        let aim = ring.and_then(|at| self.other_window_ids().get(at).copied());
        self.aim_at_window(aim);
        if changed && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(inside)
    }

    /// The tab menu's own clocks, matured — [`Runtime::advance_term_menu`]'s
    /// twin, and two clocks in one slot for its reason: a menu cannot be both
    /// waiting to open its child and holding it open against the rows.
    pub(crate) fn advance_tab_menu(&mut self, now: Instant) -> Result<()> {
        let Some(menu) = self.window.tab_menu.as_ref() else {
            return Ok(());
        };
        let Some(due) = menu.submenu_hold_until else {
            return Ok(());
        };
        if now < due {
            return Ok(());
        }
        if menu.submenu_open {
            // The cap ran out with the hand still short of the child. The
            // highlight is then owed to whatever the pointer is actually over,
            // which is asked of the geometry rather than remembered, because the
            // hold was deliberately keeping the answer stale.
            self.set_tab_submenu(false)?;
            if let Some(position) = self.window.pointer_position {
                self.drive_tab_menu_hover(position)?;
            }
            return Ok(());
        }
        self.set_tab_submenu(true)?;
        Ok(())
    }

    /// The tab menu's next wake-up, for the loop's set.
    pub(crate) fn tab_menu_deadline(&self) -> Option<Instant> {
        self.window.tab_menu.as_ref()?.submenu_hold_until
    }

    /// Do what one entry of the tab menu says, and put the menu away.
    ///
    /// The menu closes *first*, on [`Runtime::run_pane_menu_row`]'s order and
    /// for a sharper version of its reason: one of these verbs opens a text
    /// editor over the very tab the menu is standing on, two of them take the
    /// tab out of this window, and one destroys it.
    ///
    /// **The subject is an id and it is resolved here**, at the instant the verb
    /// runs — never at the instant the menu opened. Between the press that raised
    /// this and the one that ran it the strip can have been dragged into a new
    /// order, re-partitioned by a pin, or joined by a tab from another window; an
    /// index taken at raise time would by then name whatever had slid into that
    /// slot. A tab that has gone in the meantime is not a fault: nothing happens,
    /// which is the same answer `run_pane_verb` gives for a seat whose shell has
    /// exited.
    pub(crate) fn run_tab_menu_row(&mut self, hit: profiles::TabMenuHit) -> Result<()> {
        // The menu's own padding, the rule, a greyed row. A press there is the
        // menu swallowing it — decided in `mouse_input` — so there is nothing to
        // spend and, in particular, no menu to take away.
        if hit == profiles::TabMenuHit::Surface {
            return Ok(());
        }
        // **The child's row is resolved while the child still exists** (B9): a
        // `Submenu` hit counts rows on the glass, and what those rows are about
        // is a fact about the layout — which is built out of the menu state the
        // take below removes. Resolved *before* the take for the second half of
        // the same reason, so `other_window_ids` reads the same directory the
        // list was drawn from.
        let bound_for = match hit {
            profiles::TabMenuHit::Submenu(at) => {
                let of = self
                    .tab_menu_layout()
                    .and_then(|layout| layout.submenu_row(at));
                match of {
                    Some(of) => Some(self.other_window_ids().get(of).copied()),
                    // A press on a child that is no longer there — the ordinary
                    // case rather than a fault.
                    None => return Ok(()),
                }
            }
            _ => None,
        };
        let Some(menu) = self.window.tab_menu.take() else {
            return Ok(());
        };
        let tab = menu.tab;
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
            return self.move_tab_to_window(tab, window);
        }
        match hit {
            profiles::TabMenuHit::Row(row) => match row {
                // The same editor a double click on the tab body opens, through
                // the same door — §7.1.6e's rule, and the reason this row is
                // owed at all: a double click is not something a list can tell
                // you about.
                profiles::TabMenuRow::Rename => self.open_rename(tab),
                // Both faces of the one row run the one verb: `toggle_pin` is
                // the `.pin` button's own door, so the row and the button can
                // never disagree about what pinning does or about where the
                // strip's partition is enforced.
                profiles::TabMenuRow::Pin => {
                    let Some(index) = self.tab_slot_of(tab) else {
                        return Ok(());
                    };
                    self.toggle_pin(index)
                }
                profiles::TabMenuRow::Duplicate => self.duplicate_tab(tab),
                profiles::TabMenuRow::MoveToNewWindow => self.move_tab_to_new_window(tab),
                // The heading is not a verb: pressing it opens the window list,
                // and that press never reaches here. Listed so the match is
                // exhaustive over a closed set rather than over a wildcard that
                // would silently swallow a row added later.
                profiles::TabMenuRow::MoveToWindow => Ok(()),
                // The `×`'s own verb, reached through the `×`'s own door — so
                // the dirty gate a destruction has to pass is passed once and
                // not twice.
                profiles::TabMenuRow::Close => {
                    let Some(index) = self.tab_slot_of(tab) else {
                        return Ok(());
                    };
                    self.close_tab(index)
                }
            },
            // Both already answered above.
            profiles::TabMenuHit::Submenu(_) | profiles::TabMenuHit::Surface => Ok(()),
        }
    }

    /// **`Duplicate tab`** — a tab seeded from *this* tab's own active pane.
    ///
    /// The profile and the folder come off one leaf and that leaf is **this
    /// tab's**, not the focused window's: a tab menu is very often raised on a
    /// tab that is not on screen (a right press does not activate anything), and
    /// a duplicate seeded from whatever happened to be in front would be a copy
    /// of a different tab wearing this one's row. `TabState` carries its own
    /// `sessions` and its own `focused_leaf`, so a background tab's active pane
    /// is reachable without activating it, and [`TabState::focused`] is the one
    /// reader that pairs them.
    ///
    /// **A tab with no shell is refused here as well as greyed there.** The row
    /// is drawn unavailable (`profiles::TabMenuSubject::can_duplicate`) and the
    /// hit test refuses it, so this early return is unreachable through the menu;
    /// it is stated anyway, because the guard in the geometry is about a
    /// *snapshot* and the tab can have lost its shell since — and "duplicate a
    /// tab that has nothing to duplicate" has no honest answer, only arbitrary
    /// ones.
    ///
    /// It goes through [`Self::new_tab_seeded_from`] rather than through anything
    /// of its own, which is what keeps `Duplicate tab` and every other door onto
    /// a new tab one implementation.
    fn duplicate_tab(&mut self, tab: TabId) -> Result<()> {
        let Some(state) = self.tab_state(tab) else {
            return Ok(());
        };
        let Some(leaf) = state.focused() else {
            return Ok(());
        };
        let profile = leaf.profile.clone();
        let cwd = leaf.session.working_directory().map(Path::to_path_buf);
        // The source profile *is* the target profile, so `cwd_for_spawn` has no
        // namespace to cross and the folder arrives exactly as the shell reported
        // it. That is the sentence this row promises — the same shell, in the
        // same place — said in the one function that knows how to say it.
        self.new_tab_seeded_from(&profile, None, &profile, cwd)
    }

    /// **`Move tab to new window`** — the row 丙2 exists for.
    ///
    /// The tear-out onto the desktop and this row are one verb behind two doors,
    /// and until this menu the drag was the *only* door: a tab dragged past this
    /// window's glass becomes a window of its own, with no landing box, no
    /// visible ghost and no cursor change to say so. The audit's own prescription
    /// was a menu row with the same name, and this is it.
    ///
    /// **It is [`Runtime::move_pane_to_new_window`] with the first half deleted.**
    /// That verb promotes a pane to a tab and then hands the tab over; a tab is
    /// already a tab, so there is nothing to promote — which is also why the
    /// errand's `promoted` is `false` rather than a flag this row has to
    /// remember: nothing was made here that a refusal would have to report.
    ///
    /// **The window has to exist before the tab can move into it**, and only the
    /// loop's own door may create one, so the errand is written down here and
    /// spent in [`FolioApp::open_pending_window`] — in the same turn, before any
    /// frame is drawn. [`App::pending_new_windows`]'s standing shape and its
    /// standing reason.
    fn move_tab_to_new_window(&mut self, tab: TabId) -> Result<()> {
        if self.tab_slot_of(tab).is_none() {
            return Ok(());
        }
        let errand = TearOut {
            tab,
            from: self.window_id(),
            promoted: false,
            // A menu row names no place — see [`TearOut::at`]. The window opens
            // where every other new window opens, because a reader who pressed a
            // *verb* pointed at a verb and not at a rectangle.
            at: None,
        };
        let like = self.window_id();
        self.app
            .pending_new_windows
            .push(NewWindowPlan::receiving(like, errand));
        Ok(())
    }

    /// **Move this tab into a window that is already open** — the third exit's
    /// submenu, and [`Runtime::move_pane_to_window`] with the promotion dropped.
    ///
    /// It writes the *drag's* errand rather than reaching for the transfer
    /// itself, which is that function's own ruling and the reason it holds here
    /// too: only [`FolioApp`] can see two windows at once, a menu row arrives at
    /// one `Runtime` that can see neither, and `settle_drag_handover` is where
    /// the transfer, the refusal card and the landing already live. A
    /// [`DragSource::Tab`] is exactly what that road takes for cargo that is
    /// already a tab.
    ///
    /// The landing is the end of the target's strip. A drag lands where the hand
    /// let go and this row has no hand over that window at all — so the honest
    /// answer is the one every append in this program gives.
    fn move_tab_to_window(&mut self, tab: TabId, window: WindowId) -> Result<()> {
        if window == self.window_id() || self.tab_slot_of(tab).is_none() {
            return Ok(());
        }
        let slot = self
            .app
            .windows_open
            .iter()
            .find(|open| open.id == window)
            .map_or(0, |open| open.tabs);
        self.app.pending_handover = Some(DragHandover {
            cargo: DragSource::Tab(tab),
            from: self.window_id(),
            into: HandoverInto::Window {
                window,
                landing: DropLanding::StripExtract { slot },
            },
        });
        Ok(())
    }

    /// One key, with the tab menu holding the keyboard.
    ///
    /// [`Runtime::term_menu_key`]'s four rules verbatim — Esc unwinds one layer,
    /// the arrows walk, Enter/Space run, everything else is swallowed — because
    /// §7.1.3's 「可键盘化」 is a promise about context menus rather than about
    /// any particular one, and a menu that could not be walked would be the one
    /// list in this window reachable only by a pointer. Which would be a poor
    /// joke on a menu that exists because a *gesture* was unreachable.
    pub(crate) fn tab_menu_key(&mut self, event: &KeyEvent) -> Result<()> {
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                // One press, one layer — §7.1.5's ladder read inside a single
                // popup. A key that closed both would make the child unclosable
                // without also losing the parent.
                if !event.repeat && !self.set_tab_submenu(false)? {
                    self.close_tab_menu()?;
                }
            }
            Key::Named(NamedKey::ArrowRight)
                if matches!(
                    self.window.tab_menu.as_ref().and_then(|menu| menu.hover),
                    Some(profiles::TabMenuHover::Row(row)) if row.has_submenu()
                ) =>
            {
                self.set_tab_submenu(true)?;
            }
            Key::Named(NamedKey::ArrowLeft)
                if matches!(
                    self.window.tab_menu.as_ref().and_then(|menu| menu.hover),
                    Some(profiles::TabMenuHover::Submenu(_))
                ) =>
            {
                self.set_tab_submenu(false)?;
            }
            // Repeats on the travel keys and nowhere else: holding an arrow down
            // is one continuous "further", and holding Enter is not one
            // continuous "again".
            //
            // **The walk stays on the parent while the child is up**, which is
            // the terminal menu's own arrangement: the window list is a pointer
            // surface with an `←` out of it, and that is the same promise this
            // menu's other rows keep.
            Key::Named(NamedKey::ArrowDown) | Key::Named(NamedKey::ArrowUp) => {
                let forwards = matches!(event.logical_key, Key::Named(NamedKey::ArrowDown));
                // **The rows this menu is showing**, read off the picture rather
                // than off the enum (the ruling of 2026-08-25), so the walk
                // cannot stop on a `Move to window ▸` this session has no second
                // window for — nor on a `Duplicate tab` greyed on a folder tab.
                let shown: Vec<profiles::TabMenuItem> = self
                    .tab_menu_layout()
                    .map(|layout| layout.items().to_vec())
                    .unwrap_or_default();
                if let Some(menu) = self.window.tab_menu.as_mut() {
                    let current = match menu.hover {
                        Some(profiles::TabMenuHover::Row(row)) => Some(row),
                        Some(profiles::TabMenuHover::Submenu(_)) | None => None,
                    };
                    menu.hover = profiles::tab_menu_step(current, forwards, &shown)
                        .map(profiles::TabMenuHover::Row);
                }
                if self.refresh_overlay() {
                    self.present_chrome_change()?;
                }
            }
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                if !event.repeat
                    && let Some(hover) = self.window.tab_menu.as_ref().and_then(|menu| menu.hover)
                {
                    match hover {
                        // The heading opens its child rather than running, which
                        // is what `→` does and what a click does.
                        profiles::TabMenuHover::Row(row) if row.has_submenu() => {
                            self.set_tab_submenu(true)?;
                        }
                        profiles::TabMenuHover::Row(row) => {
                            self.run_tab_menu_row(profiles::TabMenuHit::Row(row))?;
                        }
                        profiles::TabMenuHover::Submenu(index) => {
                            self.run_tab_menu_row(profiles::TabMenuHit::Submenu(index))?;
                        }
                    }
                }
            }
            // Everything else is swallowed rather than passed down. With a menu
            // on screen there is nothing to type into.
            _ => {}
        }
        Ok(())
    }

    /// **`New terminal in folder…`, from the new-tab menu** — ask the system for
    /// a folder and open a tab there (user ruling 2026-08-20).
    ///
    /// [`Runtime::browse_for_split_root`]'s sibling and its whole difference is
    /// the container: that row gives the pane you are in a neighbour, this one
    /// gives the window a tab. Both queue rather than show, for the reason
    /// written out at length on [`Runtime::browse_for_root`] — `IFileDialog::Show`
    /// runs a nested message loop under the `&mut` borrow that started it.
    ///
    /// **The menu is already gone**: the picker's press router closes it before
    /// it dispatches any row, which is §7.1.6e's requirement and not something
    /// this verb has to remember. A popup left standing behind a system modal is
    /// a popup nothing can dismiss.
    ///
    /// Where the chooser opens is the same courtesy every other door shows: the
    /// folder the pane you are looking at last reported (OSC 7), and failing
    /// that the place a tab of the default profile would have started in anyway.
    /// The second half is asked of the profile rather than of this process,
    /// because "wherever Folio happens to be running from" is
    /// `C:\WINDOWS\system32` for an installed shortcut, which is not a place
    /// anybody meant.
    pub(crate) fn browse_for_new_tab_root(&mut self) {
        let start = self
            .focused()
            .and_then(|leaf| leaf.session.working_directory().map(Path::to_path_buf))
            .or_else(|| {
                // `working_directory` and not the whole place: this is a Windows
                // dialog, and that field is by construction the half of a
                // `SpawnPlace` a Windows process can be handed. A profile that
                // names its home to a launcher instead has no Windows spelling
                // for it, and `None` there is the honest answer rather than a
                // path the chooser would reject.
                profiles::spawn_place(
                    self.default_profile(),
                    None,
                    &bt_pty::SystemShellEnvironment,
                )
                .working_directory
            });
        match self.window.folder_picker.request(start.as_deref()) {
            Ok(true) => self.window.folder_pick = Some(FolderPick::NewTabIn),
            // Already queued or already open: a second request while one is up is
            // one dialog, not two, and whoever asked first keeps the answer.
            Ok(false) => {}
            Err(error) => eprintln!("recoverable folder chooser failure: {error}"),
        }
    }

    /// **`Move pane to new tab`** — the pane leaves this tab and becomes the
    /// only pane of one of its own.
    ///
    /// **The leaf moves; it is not respawned.** This is the same verb, through
    /// the same function, that dropping a pane on the tab strip already runs
    /// (N157/K123) — [`tear_pane_into_tab`] takes the `Seat` out of the tree and
    /// carries the session across under its new id, so the shell keeps its
    /// scrollback, its child processes and its working directory. Two gestures
    /// that look the same on screen and differ in whether the user's work
    /// survives are not interchangeable, and a menu row that quietly killed a
    /// shell and started a fresh one in a new tab would be exactly that.
    ///
    /// The new tab lands at the end of the strip rather than beside the one it
    /// came from, and is **not activated** — N157's rule, and it is the right one
    /// here for the same reason it is right for the drag: the gesture was "put
    /// this over there", not "take me there".
    ///
    /// **§7.1.6k left this row alone, and the ruling says so in as many words**
    /// (*"「Move pane to new tab」两步路保留"*). The spring-loaded drop is a
    /// second door onto *moving a pane*, not a replacement for the menu's — a
    /// hand that would rather point twice than carry something across the window
    /// keeps the way it already knows, and the two never disagree because the row
    /// and the drag call one verb.
    pub(crate) fn move_pane_to_new_tab(&mut self, seat: SeatId) -> Result<()> {
        let slot = self.window.tabs.len();
        let leaf = LeafId {
            tab: self.window.tabs[self.window.active_tab].id,
            seat,
        };
        // The id the tear-out hands back belongs to the row below this one,
        // which has a second half to give it to. This row's whole story is that
        // the pane is a tab now, and a lone pane's `None` is §7.1.6i's recorded
        // no-op rather than a failure.
        let _ = self.extract_pane_into_new_tab(leaf, slot)?;
        Ok(())
    }

    /// The tabs whose own triggers opened the floats on screen — `.vtab.shown`'s
    /// rows (Q173), as positions in the strip.
    ///
    /// A **list** since 2026-08-12, for the plain reason that there can now be
    /// several windows and each of them came from somewhere: with one slot, two
    /// pinned trees from two tabs would have left one of those rows dark while
    /// the window it belongs to was still on screen, which is the very thing
    /// Q173 exists to prevent.
    ///
    /// A float torn out of a *pane* head or popped out of a column contributes
    /// nothing: neither has a tab-level trigger, so there is no row entitled to
    /// say "this one is mine".
    pub(crate) fn float_shown_tabs(&self) -> Vec<usize> {
        let mut rows: Vec<usize> = self
            .window
            .float
            .live_windows()
            .filter_map(|win| match win.origin? {
                float::FloatTrigger::Tab(id) => {
                    self.window.tabs.iter().position(|tab| tab.id == id)
                }
                float::FloatTrigger::Pane(_) | float::FloatTrigger::Reference { .. } => None,
            })
            .collect();
        rows.sort_unstable();
        rows.dedup();
        rows
    }

    /// **The earliest settle any pane the reader can see still owes.**
    ///
    /// Every leaf of the tab on screen, for the reason the synchronized-update
    /// deadline walks every leaf: a stability window is a property of one pane's
    /// rows, and the pane that has just printed a `$$…$$` block is very often not
    /// the pane holding the keyboard. Read through the tab's `Deref` this asked
    /// the focused leaf alone, so a sibling that printed a block and then went
    /// quiet had nothing to wake the loop on its behalf.
    ///
    /// Only the tab on screen, because the artifacts this settles are scheduled
    /// off a frame and a tab nobody is looking at makes none; switching to it
    /// projects it and asks the whole question again.
    ///
    /// A tab with no shell has no live output to stabilise and answers `None`,
    /// which is the no-op §7.1.6h asks for.
    pub(crate) fn live_stability_deadline(&self) -> Option<Instant> {
        self.window.tabs[self.window.active_tab]
            .leaves()
            .filter_map(|(_, leaf)| leaf.session.live_stability_deadline())
            .min()
    }

    /// What every tab hangs off its trailing end, in strip order.
    ///
    /// The clock is read once, here, so the whole strip lays out against a single
    /// instant — sampling per tab would let two tabs in the same frame disagree
    /// about what time it is, and a reveal is a function of time.
    pub(crate) fn tab_trailers(&self, now: Instant) -> Vec<seats::TabTrailer> {
        let motion = self.app.motion;
        let audible = self.audible_tabs();
        // **F57 asked at the one place both axes read the run.** The strip and
        // the rail are two drawings of `self.tabs` and both are built from here,
        // so a partition broken by any write path — including one written after
        // this line — is caught on the first frame that tries to draw it, naming
        // F57 instead of arriving as a screenshot nobody can explain. This is the
        // sentence `pins_are_normalized` was written to be held to, and it is a
        // question rather than a repair: the fixing belongs to
        // [`settle_pin_partition`] at the write, and a read that quietly sorted
        // would hide the very path that needs finding.
        debug_assert!(
            seed::pins_are_normalized(&self.window.tabs, |tab| tab.pinned),
            "F57: the pinned run leads the strip"
        );
        self.window
            .tabs
            .iter()
            .map(|tab| seats::TabTrailer {
                pinned: tab.pinned,
                reveal: tab.pin_reveal.sample(now, motion).0,
                // H107, asked of the tree and not of a cached flag: "exactly one
                // pane, and it is a terminal" is a fact about the layout, and the
                // layout is the only thing entitled to answer it.
                files: tab.seats.is_lone_terminal(),
                files_lit: tab.files_lit.sample(now, motion).0,
                audible: audible.contains(&tab.id),
            })
            .collect()
    }

    /// **Which tabs of this window have a page making a sound** (user ruling
    /// 2026-08-27; `docs/DESIGN.md` §7.23 ⑩).
    ///
    /// Built once per strip rather than asked per tab, because `window.web` is
    /// keyed by leaf and a per-tab question would walk the whole map once for
    /// every tab — and because the two strips are two drawings of one answer,
    /// exactly as [`Self::tab_trailers`] is one builder for both.
    ///
    /// **Any page is enough.** A tab is a container, the mark says the sound is
    /// inside it, and one page is as much inside it as two.
    pub(crate) fn audible_tabs(&self) -> std::collections::BTreeSet<TabId> {
        let mut tabs: std::collections::BTreeSet<TabId> = self
            .window
            .web
            .iter()
            .filter(|(_, web)| web.playing_audio())
            .map(|(leaf, _)| leaf.tab)
            .collect();
        // **And the recordings, which are where a sound comes from now** (route
        // B slice ②, 2026-08-28; §7.44 ②, closing §7.42 ⑪ ⓓ).
        //
        // The line above asked a browser `IsDocumentPlayingAudio` — a question
        // about a *document*, answered by an engine, arriving as an event this
        // window had to subscribe to and remember. The new judgement is the
        // ruling's own and needs no memory: `playing && !muted`, read off the
        // engine at the instant the strip is drawn, plus `has_audio` so that a
        // silent screen capture does not point a reader at a tab with nothing to
        // hear. See `video_seat::VideoSeat::is_sounding`.
        //
        // **A float's and a card's recording belong to no tab**, and neither is
        // in this set: the mark says *which tab* the sound is in, and a floating
        // window is not in one. That is not a gap — a float is on the glass in
        // front of the reader, which is the thing the mark exists to help them
        // find.
        tabs.extend(
            self.window
                .video
                .iter()
                .filter(|(_, seat)| seat.is_sounding())
                .filter_map(|(surface, _)| match surface {
                    PreviewSurface::Seat(leaf) => Some(leaf.tab),
                    PreviewSurface::Float(_) | PreviewSurface::Peek => None,
                }),
        );
        tabs
    }

    /// Which tab the pointer is on. Every trailing control belongs to its tab,
    /// so hovering the `×` or the pin is still hovering the tab — the mock-up's
    /// `.tab:hover .pin` is a descendant rule and holds the pin open while the
    /// pointer is anywhere inside.
    pub(crate) fn hovered_tab(&self) -> Option<usize> {
        match self.window.seat_pointer.hover {
            // The folder is a control *of* its tab, so hovering it is hovering
            // the tab — the mock-up's `.tab:hover .tab-files` is a descendant
            // rule, and a trigger that stopped counting as its own tab's hover
            // would go dark the instant the pointer reached it.
            Some(
                seats::ChromeTarget::Tab(index)
                | seats::ChromeTarget::TabClose(index)
                | seats::ChromeTarget::TabPin(index)
                | seats::ChromeTarget::TabFiles(index),
            ) => Some(index),
            _ => None,
        }
    }

    /// Repaint the chrome when the pointer moves onto or off a divider, a close
    /// affordance or a collapsed bar.
    /// **The tab list, on whichever axis this window is wearing it** — the
    /// strip, the icon rail, or the focus column, asked as one surface.
    ///
    /// Lifted out of [`Self::chrome_target_at`] so that a second caller can ask
    /// the very same question: [`Self::web_page_at`] has to subtract it, and a
    /// subtraction written a second time there would be a second answer to
    /// "where is the tab list" for the two to drift apart on. It is the head of
    /// the chrome ladder and nothing else moved.
    ///
    /// **Total over its own rectangle and `None` everywhere else** — all three
    /// hit tests promise that (`hit_focus_rail`, `hit_rail_chrome`,
    /// `hit_tab_chrome` each answer `None` before they answer anything), which
    /// is what makes `is_some()` a usable reading of "the list covers this
    /// pixel".
    pub(crate) fn tab_list_target_at(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<seats::ChromeTarget> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (width, height) = (width as f32, height as f32);
        let now = Instant::now();
        let (trailers, pinned) = self.rail_list(now);
        let rail = self.sampled_rail(now);
        // **R1 — the tab list answers first, on whichever axis it is on.**
        //
        // The two are exclusive by construction and not by luck: `hit_rail_chrome`
        // returns `None` for a horizontal layout and `tab_strip_geometry` draws
        // nothing in a vertical one, so this is one list asked in one place rather
        // than two lists racing. It answers *before* `hit_chrome` because an open
        // icon rail lies over the panes (Q179) — asking the panes first would hand
        // every click on an open rail to whatever is underneath it.
        //
        // Both branches hand back the same `ChromeTarget::Tab/TabClose/TabPin`,
        // so every click handler downstream — activation, the middle-click close,
        // the press that becomes a drag — is untouched by the axis.
        //
        // **Focus mode is a third branch of the same one place** (§7.1.6b′): the
        // card column is the tab list while it is up, and it answers the very
        // same `Tab`/`TabClose` targets — which is what makes "clicking a card is
        // clicking that tab" true without a second handler anywhere below here.
        match if rail.draws_focus_rail() {
            None
        } else {
            Some(rail.layout)
        } {
            None => seats::hit_focus_rail(
                height,
                scale,
                self.platform_chrome(),
                &trailers,
                self.strip_guests(),
                self.window.rail_scroll,
                rail,
                position.x,
                position.y,
            ),
            Some(seats::TabLayoutMode::Vertical) => seats::hit_rail_chrome(
                height,
                scale,
                self.platform_chrome(),
                &trailers,
                pinned,
                self.window.rail_scroll,
                rail,
                position.x,
                position.y,
            ),
            Some(seats::TabLayoutMode::Horizontal) => seats::hit_tab_chrome(
                width,
                scale,
                self.platform_chrome(),
                &trailers,
                self.window.active_tab,
                self.window.tab_scroll,
                position.x,
                position.y,
            ),
        }
    }

    /// A left press on a tab's body (J105, and the first half of J99).
    ///
    /// It arms the promise and nothing else — no view changes here, which is the
    /// whole mechanism. The second press of a double click arms nothing either:
    /// the first one already put the view on this tab, so there is nothing left
    /// for it to owe.
    pub(crate) fn press_tab(
        &mut self,
        index: usize,
        position: PhysicalPosition<f64>,
    ) -> Result<()> {
        let now = Instant::now();
        let tab = self.window.tabs[index].id;
        // Deliberately *not* counted here. A click is complete when the button
        // comes back up, which is why `dblclick` is a release-time event;
        // counting the press as well would pair each click with itself and turn
        // the very first one into a double.
        // One button, one press: whichever source the router chose, the others
        // are not being held.
        self.window.pane_press = None;
        self.window.row_press = None;
        self.window.tab_press = Some(if index == self.window.active_tab {
            TabPress::settled(tab, position, now)
        } else {
            TabPress::armed(tab, position, now)
        });
        Ok(())
    }

    /// The left button coming back up over `target`.
    pub(crate) fn release_tab_press(
        &mut self,
        mut press: TabPress,
        target: Option<seats::ChromeTarget>,
    ) -> Result<()> {
        let over = match target {
            Some(seats::ChromeTarget::Tab(index)) => self.window.tabs.get(index).map(|tab| tab.id),
            _ => None,
        };
        if press.released_over(over) {
            self.activate_tab(self.tab_index(press.tab), false)?;
        }
        // The editor opens on the *second* release, which is where `dblclick`
        // fires: down, up, click, down, up, click, and only then `dblclick`
        // (mock-up 5737). The first click has already activated the tab by the
        // time this runs a second time, which is why the editor never has to
        // activate anything itself.
        let Some(clicked) = over.filter(|tab| *tab == press.tab) else {
            // Down here and up there is not a click on either, and it is not the
            // first half of one either.
            self.window.tab_clicks.interrupt();
            return Ok(());
        };
        if self.window.tab_clicks.register(clicked, Instant::now()) == TabClick::Double {
            self.open_rename(clicked)?;
        }
        Ok(())
    }

    /// The strip's live geometry — the slots every drag judgement is made
    /// against.
    fn strip_geometry(&self, now: Instant) -> seats::TabStripGeometry {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let (width, _) = self.window.renderer.presentation_geometry().swapchain_size;
        seats::tab_strip_geometry(
            width as f32,
            scale,
            self.platform_chrome(),
            &self.tab_trailers(now),
            self.window.active_tab,
            self.window.tab_scroll,
        )
    }

    /// **The tab run the drag is happening in, whichever axis it is on** (R3).
    ///
    /// The one place the layout mode is read by the drag engine. Everything
    /// downstream of this — the grip, the carry, the reorder, the tear-out's
    /// caret, the FLIP deltas — takes a [`seats::TabRun`] and cannot tell which
    /// surface it got, which is the whole point: the mock-up's `stripEl()`
    /// (6505) is one function for the same reason.
    ///
    /// **`None` is a real answer.** A collapsed rail puts no tab list on screen,
    /// and with no list there is nothing to drag along and nothing for a torn-out
    /// pane to be dropped into — so every caller's "there is no run" branch is
    /// the honest behaviour rather than a failure to handle. The horizontal strip
    /// has no such state: it is always drawn, even holding one tab.
    pub(crate) fn tab_run(&self, now: Instant) -> Option<seats::TabRun> {
        // **In focus mode the panel holds a card column, and the column is a
        // tab run like the other two** (§7.1.6b′ ④, 2026-08-20). It offers a card
        // every slot the list has, and since the user's ruling of 2026-08-29 it
        // offers a pane both of the things the other two runs offer one — the
        // hand-over onto a card, and the tear-out into the blank. The one drag
        // it still turns away is a file row (③), and it turns that away through
        // the run rather than beside it, so this branch stays a choice of
        // surface and never becomes a second set of drag rules. `None` here
        // would be the one wrong answer: it would hand every drop over the
        // column to the pane behind it.
        if self.window.focus_mode {
            return self
                .focus_rail_geometry_now(now)
                .as_ref()
                .map(seats::focus_rail_run);
        }
        match self.window.rail.layout {
            seats::TabLayoutMode::Horizontal => {
                let scale = self.window.renderer.metrics().scale_factor as f32;
                Some(seats::strip_run(&self.strip_geometry(now), scale))
            }
            seats::TabLayoutMode::Vertical => {
                self.rail_geometry_now(now).as_ref().map(seats::rail_run)
            }
        }
    }

    /// **Which of this window's two scroll offsets the run in
    /// [`Runtime::tab_run`] was solved at** (缺陷 #188).
    ///
    /// The card column and the vertical rail share `rail_scroll` because they
    /// share the panel — the same sentence [`Runtime::scroll_rail`] is built on
    /// — and the horizontal strip has `tab_scroll` of its own. This pair mirrors
    /// [`Runtime::tab_run`]'s branch and must go on mirroring it: reading the
    /// offset off one surface while the run came from another would auto-scroll
    /// a list nobody is pointing at.
    ///
    /// It is a pair and not a `&mut f32` so that the write stays a write to a
    /// named field on the window rather than a borrow handed out of this
    /// function, which is what lets the caller re-survey the drag in the same
    /// breath.
    pub(crate) fn tab_run_scroll(&self) -> f32 {
        if self.window.focus_mode || self.window.rail.layout == seats::TabLayoutMode::Vertical {
            self.window.rail_scroll
        } else {
            self.window.tab_scroll
        }
    }

    /// [`Runtime::tab_run_scroll`]'s other half — put the run's list where the
    /// auto-scroll says it now stands.
    pub(crate) fn set_tab_run_scroll(&mut self, scroll: f32) {
        if self.window.focus_mode || self.window.rail.layout == seats::TabLayoutMode::Vertical {
            self.window.rail_scroll = scroll;
        } else {
            self.window.tab_scroll = scroll;
        }
    }

    /// K111 and J106 — the press has travelled 6px, so it is a drag now.
    ///
    /// The activation is not a side effect: "reordering IS commitment to the
    /// strip context: the tab in hand shows itself, whether or not the press
    /// timer had fired" (mock-up 6832-6833). Committing it here is also what
    /// pays the press's promise for good, so a drag that is later cancelled has
    /// nothing left to owe (J108).
    ///
    /// The grip is measured from the press's own origin rather than from the
    /// pointer's position now. That is what `startDrag` does (6484-6486) and it
    /// is the difference between a tab that stays where your fingers put it and
    /// one that snaps 6px sideways the instant it comes free.
    pub(crate) fn begin_tab_drag(
        &mut self,
        press: TabPress,
        position: PhysicalPosition<f64>,
    ) -> Result<()> {
        let Some(index) = self.window.tabs.iter().position(|tab| tab.id == press.tab) else {
            return Ok(());
        };
        // N163's `homeWs`, read before the activation below can change the
        // answer: this is the tab whose layout the user was looking at when the
        // gesture started, and the one they mean when they aim below the strip.
        let home = self.window.tabs[self.window.active_tab].id;
        self.activate_tab(index, false)?;
        // Re-read the run: activating may have scrolled it to reveal the tab,
        // and a grip measured against the old scroll would be wrong by exactly
        // that much.
        let index = self.tab_index(press.tab);
        let now = Instant::now();
        let Some(grab) = self.tab_run(now).and_then(|run| {
            run.start(index)
                .map(|start| run.pos(press.latch.origin.x, press.latch.origin.y) - start)
        }) else {
            return Ok(());
        };
        self.begin_drag(
            DragSource::Tab(press.tab),
            DragCarry::Tab(TabCarry {
                grab,
                origin: index,
                offset: 0.0,
                moved: false,
                home,
            }),
            position,
            // A tab is carried *inside* the strip, so "back where it came from"
            // is a slot rather than a rectangle, and J120's settle already puts
            // it there. A home rectangle would be a second, coarser answer to a
            // question the run answers exactly.
            None,
        )
    }

    /// The tab list's arm of [`Runtime::survey_drop`] (K123-K125), on whichever
    /// axis it is on.
    ///
    /// The two sources ask the run for different things and measure it
    /// differently, which is why they are two arms rather than one with a flag.
    /// A tab already in the run is a *body* sliding along it and it swaps with a
    /// neighbour once it has covered half of it ([`seats::reorder_target`]); a
    /// pane arriving from the layout has no body in the run yet, so the only
    /// operand is the pointer against the slot midpoints
    /// ([`seats::insert_index_at`], K125).
    ///
    /// Neither arm knows which surface it is judging, because a
    /// [`seats::TabRun`] does not say: the mids, the half and the pointer's
    /// coordinate all arrive already projected onto the axis that run is on.
    pub(crate) fn survey_strip(
        &self,
        source: &DragSource,
        run: &seats::TabRun,
        position: PhysicalPosition<f64>,
        seam: &mut Option<usize>,
    ) -> Option<DropLanding> {
        let slot_mids = run.mids();
        // **The seam band, asked once for both arriving payloads** (user ruling
        // 2026-09-06).
        //
        // Read here rather than inside each arm because it is one sentence about
        // one pointer, and because the two arms below must not be able to
        // disagree about which seam the hand is in: the latch is a single number
        // and a second reader that re-derived it would be a second hysteresis
        // with its own history.
        //
        // **A tab already in the run is not asked it at all.** That arm is a
        // reorder — a body sliding along the list, judged against its neighbours
        // rather than against the pointer ([`seats::reorder_target`]) — and it
        // has no "into that tab" verb for the seam to have priority over. The
        // ruling is about the two verbs a list offers something that is *not* in
        // it yet, so a tab's drag leaves the latch empty rather than filling it
        // with a number nothing downstream will read.
        let aim = (!matches!(source, DragSource::Tab(_)))
            .then(|| {
                run.aim(
                    position.x,
                    position.y,
                    self.window.renderer.metrics().scale_factor as f32,
                    *seam,
                )
            })
            .flatten();
        *seam = match aim {
            Some(seats::StripAim::Insert(index)) => Some(index),
            Some(seats::StripAim::Into(_)) | None => None,
        };
        // **What the pointer names, in the two shapes the arms below want it in.**
        //
        // `over` is the entry being aimed at, and the band's whole effect is that
        // it is `None` inside a seam: an insertion and a hand-over are the two
        // pictures this surface can draw and the ruling says they are exclusive
        // (「插入线与并入预览互斥」 — the line named there was struck the same
        // day, §7.1.6k⁶, and the picture left standing for an insertion is the
        // stand-in's own slot), so the seam does not merely outrank the
        // hand-over, it withholds the fact the hand-over is built out of.
        //
        // `insert_at` is where a new entry would land. Inside a seam it is the
        // seam's own index — the two numberings are one numbering, which is why
        // [`seats::TabRun::seams`] counts the head and the tail — and outside one
        // it is the midpoint walk that has always answered here, unchanged, so
        // every pointer the band does not claim gets exactly the answer it got
        // before this ruling.
        let over = match aim {
            Some(seats::StripAim::Into(index)) => self.window.tabs.get(index).map(|tab| tab.id),
            Some(seats::StripAim::Insert(_)) | None => None,
        };
        let insert_at = match aim {
            Some(seats::StripAim::Insert(index)) => index,
            Some(seats::StripAim::Into(_)) | None => {
                seats::insert_index_at(&slot_mids, run.pos(position.x, position.y))
            }
        };
        match source {
            DragSource::Tab(tab) => {
                let tab = *tab;
                let index = self
                    .window
                    .tabs
                    .iter()
                    .position(|candidate| candidate.id == tab)?;
                let half = run.half(index)?;
                let mid = *slot_mids.get(index)?;
                let offset = self.track_grabbed(position)?;
                Some(DropLanding::StripReorder {
                    slot: seats::reorder_target(
                        &slot_mids,
                        &self
                            .window
                            .tabs
                            .iter()
                            .map(|tab| tab.pinned)
                            .collect::<Vec<_>>(),
                        index,
                        mid + offset,
                        half,
                    ),
                })
            }
            // K124 — only while both halves would be tabs. G84 is one reason and
            // it is a rule of the tree rather than of the gesture: a tree may not
            // be emptied, so the last pane has nowhere to be torn to.
            //
            // I106 is the second. The mock-up's guard here is `paneCount > 1`
            // because in the mock-up a pane is only a subtree; here it is a
            // subtree *and* a shell, so the question is not "is anything left"
            // but "is the thing leaving something the strip can hold". Drawing
            // an insertion caret in the strip for a tear-out that the release
            // cannot perform is the silent refusal M147 forbids, in the one place
            // that has no dashed box to wear instead.
            //
            // N158 clamps the slot here rather than at the commit, so the caret
            // the user watches and the slot the release inserts at are one
            // number — see [`strip_insert_slot`].
            // **[`seats::PaneOffers`] is §7.1.6b′ ② and it is asked of the
            // run**, so this arm goes on not knowing which surface it is
            // judging. A card column is the tab list of *one window* drawn
            // beside the tab it is showing, and what it withholds has to be a
            // fact about the surface rather than a second reading of
            // `window.focus_mode` here — R3's whole shape is that
            // `Runtime::tab_run` is the one place the surface is chosen and
            // everything under it takes a [`seats::TabRun`] — and it has to be
            // its own field rather than "has no slots", because since 2026-08-20
            // the column has slots.
            //
            // The user's 2026-08-23 ruling split that field into two offers and
            // the ruling of 2026-08-29 granted the column both of them, so every
            // surface in this build now answers `BOTH`. This arm did not have to
            // learn either ruling, which is the point of it living on the run —
            // a card column's blank makes a tab by walking the very same
            // `insert_index_at` and `strip_insert_slot` below.
            //
            // **Two offers, and which one this pointer is asking for is
            // [`pane_strip_landing`]'s** (§7.1.6k). The run answers both questions
            // off the slots it already carries, and since 2026-09-06 it answers
            // them as one — [`seats::TabRun::aim`] above, which is `slot_at`
            // ("whose tab is under my hand") and `insert_index_at` ("between
            // which two would a new one land") with the seam band deciding which
            // of the two the pointer is asking.
            DragSource::Pane(leaf) => {
                pane_strip_landing(
                    run.pane_offers,
                    over,
                    leaf.tab,
                    // **§7.1.6k″ — where the stage is, which the spring moves.**
                    // Read here rather than remembered on the drag for the
                    // reason every other reading of it is: `active_tab` is the
                    // one place this window says which tree is on screen, and a
                    // survey that kept its own copy would go on answering about
                    // a stage that had already moved under it.
                    self.window.tabs[self.window.active_tab].id,
                    // Asked of every pointer move rather than at the release, and
                    // that ordering is M147's: refuse at the release and the tab
                    // stays lit right up until the hand opens on nothing.
                    over.is_some_and(|tab| self.pane_adopt_fits(*leaf, tab)),
                    self.tear_out_is_hostable(*leaf).then(|| {
                        // N158 clamps the slot here rather than at the commit, so
                        // the caret the user watches and the slot the release
                        // inserts at are one number — see [`strip_insert_slot`].
                        strip_insert_slot(
                            insert_at,
                            &self
                                .window
                                .tabs
                                .iter()
                                .map(|tab| tab.pinned)
                                .collect::<Vec<_>>(),
                        )
                    }),
                )
            }
            // **P85/S3's tab-strip verbs, landed** (user ruling 2026-07-17;
            // recorded as unbuilt 2026-08-13, revised with §7.1.6h, built
            // 2026-08-30).
            //
            // The mock-up's `fileToTab` makes "a fresh workspace holding one
            // preview pane" and `folderToTab` makes one holding a files column.
            // The note that stood here listed the three layers that once made
            // neither representable — a hundred `Deref` sites resolving to *the
            // focused shell*, a `Seats` carrying a mandatory `terminal: SeatId`,
            // and a strip whose identity paths were all built out of a shell —
            // and recorded that the sessionless-tab slice had taken all three
            // down, leaving only the **gesture** missing. This is the gesture.
            //
            // It is read off the very same two questions a pane's is
            // ([`pane_strip_landing`]), asked of the same run, so a card column
            // gets both verbs without a line of its own: `slot_at` for "whose
            // tab is under my hand" and `insert_index_at` for "between which two
            // would a new one land". What the two mean for a path is
            // [`row_strip_landing`]'s, and what a room will take is
            // [`Runtime::row_adopt_fits`]'s — asked on every pointer move rather
            // than at the release, which is M147's ordering: refuse at the
            // release and the tab stays lit right up until the hand opens on
            // nothing.
            DragSource::Row(payload) => row_strip_landing(
                over,
                over.is_some_and(|tab| self.row_adopt_fits(payload.kind, tab)),
                // N158's clamp, here rather than at the commit, so the caret
                // the user watches and the slot the release inserts at are
                // one number — see [`strip_insert_slot`].
                strip_insert_slot(
                    insert_at,
                    &self
                        .window
                        .tabs
                        .iter()
                        .map(|tab| tab.pinned)
                        .collect::<Vec<_>>(),
                ),
            ),
        }
    }

    /// **N157's precondition**: whether tearing this pane out would leave two
    /// tabs this window can hold.
    ///
    /// Both halves are asked, and both have to answer, because a tear-out makes
    /// two tabs rather than moving one pane: the pane that leaves becomes a tab
    /// on its own, and what stays behind has to go on being one.
    ///
    /// **What stays** is `tear_out`'s question and it is G84's: a tree may not be
    /// emptied, so the last pane in a tab has nowhere to be torn to and `tear_out`
    /// answers `None` for it. The mock-up's `paneCount > 1` is inside that answer
    /// rather than beside it. Everything else survives by construction now that
    /// panes own sessions — the shells of the leaves that stayed stay with them.
    ///
    /// **What leaves** is [`pane_can_become_a_tab`]'s question. It was I106's —
    /// a strip entry needs a shell, so a Terminal pane may go and a files column
    /// or a preview may not — and §7.1.6h retired that: all three may go now, and
    /// only a leaf this build cannot read is turned away. This used to be
    /// `tab_can_host` asked of both halves, and it was false for *every* pane —
    /// with one shell to a tab, either the pane leaving was the terminal and what
    /// stayed had none, or it was not and what left had none. Panes own sessions
    /// today, so the honest question is no longer about the trees at all but
    /// about the one seat that is moving.
    ///
    /// **Asked of the pane's own tab and never of the tab on screen** (§7.1.6k).
    /// The spring can leave those two apart for the rest of a gesture, and the
    /// question "may this pane become a tab" is about the tree it is standing in
    /// rather than about the tree being drawn.
    fn tear_out_is_hostable(&self, leaf: LeafId) -> bool {
        let Some(tab) = self.tab_state(leaf.tab) else {
            return false;
        };
        tab.seats
            .tear_out(&self.seat_metrics(), leaf.seat)
            .is_some()
            && tab
                .seats
                .tree()
                .find_seat(leaf.seat)
                .is_some_and(|seat| pane_can_become_a_tab(seat.kind))
    }

    /// **What this window's tab list offers a payload another window is
    /// holding** (multiwindow slice F2).
    ///
    /// [`Runtime::survey_strip`]'s counterpart for a pointer this window will
    /// never hear about, and it is deliberately the *same* table: the run's own
    /// [`seats::PaneOffers`], the run's own `slot_at`, and
    /// [`pane_strip_landing`]. Whatever a card column offers its own window's
    /// panes it offers a visitor's, and it does not have to learn that this
    /// slice happened — which is why the ruling of 2026-08-29 reached the other
    /// window's column for free: [`Runtime::tab_run`] hands this function the
    /// focus column in a window that is in focus mode, exactly as it hands
    /// [`Runtime::survey_strip`] one.
    ///
    /// **The tab list is the whole door, and the body offers nothing.** That is
    /// F2's own shape — 「拖到另一扇 Folio 窗的 tab 条 = 移入」 — and the
    /// alternative was rejected where the plan asked for it to be written down
    /// (*"拖到目标窗正文(= 无落点)"*): a pointer over a foreign terminal that tore
    /// a window off would be one gesture meaning two things depending on which
    /// window happened to be underneath.
    ///
    /// **A tab arriving reads as [`DropLanding::StripExtract`]**, and that is a
    /// reuse rather than a pun: what that landing has always named is *this run
    /// gains an entry at this slot*, drawn as the stand-in
    /// ([`Runtime::strip_stand_in`]) the reader is about to see become a tab.
    /// What differs is only who performs it, and no foreign landing is ever
    /// handed to [`release_verdict`] — [`broker_verdict`] is the verdict for a
    /// hand that opened somewhere else.
    pub(crate) fn foreign_strip_landing(
        &self,
        cargo: &DragSource,
        cargo_tree: Option<&bt_layout::LayoutNode>,
        screen: (f64, f64),
    ) -> Option<DropLanding> {
        // **Four stations, because this answer has four ways to be nothing and a
        // photograph can tell them apart from none of them** (user report
        // 2026-08-27 ②, *"跨窗落点恒为末尾"*). A reader watching a tab land in the
        // wrong slot is watching the end of a chain whose every hop is invisible:
        // this window has no origin to subtract, or no run to ask, or the pointer
        // is outside its band, or the walk answered a number. `BT_MOUSE_TRACE` is
        // forensic apparatus and nothing else — see [`mouse_trace`] — and these
        // are the three numbers the chain is made of: the midpoints, the raw walk
        // and the clamped slot. The two stations further down the road
        // ([`Runtime::hand_over_across_windows`],
        // [`FolioApp::settle_arrival`]) complete it.
        let Some(position) = self.screen_to_client(screen) else {
            self.mouse_trace(|| "foreign_strip_landing at=no-client-origin".to_owned());
            return None;
        };
        let Some(run) = self.tab_run(Instant::now()) else {
            self.mouse_trace(|| "foreign_strip_landing at=no-run".to_owned());
            return None;
        };
        if !run.contains(position.x, position.y) {
            self.mouse_trace(|| {
                format!(
                    "foreign_strip_landing at=outside-band client=({:.1},{:.1}) band={:?}",
                    position.x, position.y, run.band
                )
            });
            return None;
        }
        let mids = run.mids();
        let pinned = self
            .window
            .tabs
            .iter()
            .map(|tab| tab.pinned)
            .collect::<Vec<_>>();
        // N158's clamp, on this window's own partition: the slot the visitor
        // watches is the slot the hand-over inserts at.
        let raw = seats::insert_index_at(&mids, run.pos(position.x, position.y));
        let slot = strip_insert_slot(raw, &pinned);
        self.mouse_trace(|| {
            format!(
                "foreign_strip_landing at=slot screen=({:.1},{:.1}) client=({:.1},{:.1}) \
                 axis={:.1} mids={mids:?} tabs={} raw={raw} slot={slot}",
                screen.0,
                screen.1,
                position.x,
                position.y,
                run.pos(position.x, position.y),
                pinned.len()
            )
        });
        match cargo {
            // A whole tab has one offer here and it does not depend on the run's
            // pane bits: those say what a *pane* may do to a tab list, and a tab
            // arriving in a tab list is the thing a tab list is for. A card
            // column takes it for the same reason it takes a reorder — the column
            // *is* the tab list of that window (§7.1.6b′ ①).
            DragSource::Tab(_) => Some(DropLanding::StripExtract { slot }),
            DragSource::Pane(leaf) => {
                let over = run
                    .slot_at(position.x, position.y)
                    .and_then(|index| self.window.tabs.get(index))
                    .map(|tab| tab.id);
                pane_strip_landing(
                    run.pane_offers,
                    over,
                    // The pane's own tab, which cannot be in *this* window's run
                    // — so the "resting on its own tab" arm is unreachable from
                    // here, and reachable from `survey_strip` exactly as before.
                    // §7.1.6k″'s `showing` is handed in for the same reason and
                    // is equally unreachable: `over` can never equal `holding`
                    // here, so neither half of that condition is ever the one
                    // deciding. This window's own stage is the honest answer to
                    // "which tree is on screen" all the same.
                    leaf.tab,
                    self.window.tabs[self.window.active_tab].id,
                    over.is_some_and(|tab| {
                        cargo_tree.is_some_and(|tree| self.arrival_fits(tab, tree))
                    }),
                    // **A lone pane may cross a boundary even though it may not
                    // be torn out where it stands.** G84 refuses to empty a tree,
                    // which is why `tear_out_is_hostable` says no at home; over
                    // another window there is nothing to empty, because the tab
                    // it is the whole of travels with it. That is F1c's own
                    // sentence — 「独 pane 跳过升格那半段」 — and this is the
                    // surface it is said on.
                    Some(slot),
                )
            }
            // **§7.1.1's two tab-strip verbs, over somebody else's glass** — and
            // it is the same table asked the same way, because the payload is a
            // *path*: nothing has to travel for this window to open it, and
            // `cargo_tree` is `None` for a row precisely because there is no tree
            // to bring.
            DragSource::Row(payload) => {
                let over = run
                    .slot_at(position.x, position.y)
                    .and_then(|index| self.window.tabs.get(index))
                    .map(|tab| tab.id);
                row_strip_landing(
                    over,
                    over.is_some_and(|tab| self.row_adopt_fits(payload.kind, tab)),
                    slot,
                )
            }
        }
    }

    /// The tab with this id, by identity.
    pub(crate) fn tab_state(&self, tab: TabId) -> Option<&TabState> {
        self.window
            .tabs
            .iter()
            .find(|candidate| candidate.id == tab)
    }

    /// Where in the strip the tab with this id stands.
    pub(crate) fn tab_slot_of(&self, tab: TabId) -> Option<usize> {
        self.window
            .tabs
            .iter()
            .position(|candidate| candidate.id == tab)
    }

    /// **K126/N163 — the hand has left the strip.**
    ///
    /// Two things happen at that boundary and the mock-up does both in one place
    /// (6909-6919), because they are one sentence: the tab stops being carried
    /// along the run and the *view* goes back to where the gesture set out from.
    ///
    /// `releaseGrabbed(true)` is the first half — the tab falls into whatever
    /// slot the live reorder left it in, sliding rather than jumping, and the
    /// ghost takes over as the thing under the pointer. Nothing is committed by
    /// it: the reorder was already applied, and letting go over open air still
    /// takes it home (J120).
    ///
    /// The second half is press-activation's counterpart. Pressing a background
    /// tab activates it so you can see what you have picked up, which means the
    /// layout on screen is now the *dragged* tab's — and a tab cannot be merged
    /// into itself (K129). Aiming below the strip says "now place A somewhere in
    /// the layout I was in", so the view flips back to
    /// [`TabCarry::home`] and the merge gets a target. Without this the whole of
    /// K's lower half is unreachable for a tab: every pointer below the strip
    /// would be over the dragged tab's own layout, and K129 would answer nothing
    /// every time.
    ///
    /// Idempotent by construction, which matters because a pointer moves many
    /// times outside the strip and this runs on all of them: the slide home only
    /// has an offset to run down once, and the flip's own condition is false the
    /// moment it has happened.
    pub(crate) fn leave_strip(
        &mut self,
        drag: &mut Drag,
        position: PhysicalPosition<f64>,
    ) -> Result<()> {
        let (Some(tab), Some(mut carry)) = (drag.tab(), drag.tab_carry()) else {
            return Ok(());
        };
        // A collapsed rail answers no run at all, and that reads correctly here:
        // with no tab list on screen the pointer is outside it wherever it is,
        // so the tab slides home and the view flips back exactly as it does for
        // a pointer over the terminal.
        if self
            .tab_run(Instant::now())
            .is_some_and(|run| run.contains(position.x, position.y))
        {
            return Ok(());
        }
        if carry.offset != 0.0
            && let Some(index) = self
                .window
                .tabs
                .iter()
                .position(|candidate| candidate.id == tab)
        {
            self.window.tabs[index]
                .flip
                .displace(carry.offset, Instant::now(), self.app.motion);
            carry.offset = 0.0;
            drag.carry = DragCarry::Tab(carry);
        }
        if carry.home != tab
            && self.window.tabs[self.window.active_tab].id == tab
            && let Some(home) = self
                .window
                .tabs
                .iter()
                .position(|candidate| candidate.id == carry.home)
        {
            self.activate_tab(home, false)?;
        }
        Ok(())
    }

    /// The live half of [`DropLanding::StripReorder`]: put the strip in the order
    /// the hand is asking for, and answer with where the tab now sits.
    ///
    /// The reorder is *applied*, not previewed. That is the mock-up's
    /// `reorderWhileDragging` (6835) and the reason it can be: the strip has one
    /// axis and one kind of occupant, so the arrangement the drop would produce
    /// is a thing the strip can simply be in while you are still deciding.
    pub(crate) fn settle_strip_reorder(
        &mut self,
        tab: TabId,
        mut carry: TabCarry,
        to: usize,
        position: PhysicalPosition<f64>,
    ) -> TabCarry {
        let Some(index) = self
            .window
            .tabs
            .iter()
            .position(|candidate| candidate.id == tab)
        else {
            return carry;
        };
        let Some(offset) = self.track_grabbed(position) else {
            return carry;
        };
        if to == index {
            carry.offset = offset;
            return carry;
        }
        self.move_tab_with_flip(index, to, Instant::now(), Some(tab));
        carry.moved = true;
        // Its slot has moved, so the distance from the slot to the hand has
        // changed with it (mock-up 6727-6729).
        carry.offset = self.track_grabbed(position).unwrap_or(offset);
        carry
    }

    /// Move a tab between slots and let every tab the move displaced slide into
    /// its new one — K117's FLIP.
    ///
    /// The order changes first and the animation is derived from the difference,
    /// which is what makes this FLIP rather than a hand-written slide: nothing
    /// here has to know *why* the strip re-laid out, only that it did.
    ///
    /// `skip` is the tab in hand, which does not take part: it is already
    /// somewhere of its own choosing, and inverting it back to a slot it is not
    /// in would tear it out from under the pointer (K117).
    pub(crate) fn move_tab_with_flip(
        &mut self,
        from: usize,
        to: usize,
        now: Instant,
        skip: Option<TabId>,
    ) {
        if from == to || from >= self.window.tabs.len() || to >= self.window.tabs.len() {
            return;
        }
        let motion = self.app.motion;
        let active = self.window.tabs[self.window.active_tab].id;
        let before = self.slot_starts(now);
        let was = self
            .window
            .tabs
            .iter()
            .map(|tab| tab.id)
            .collect::<Vec<_>>();
        let tab = self.window.tabs.remove(from);
        self.window.tabs.insert(to, tab);
        // Everything keyed on a slot has to be re-derived from identity after the
        // order changes — the active tab most of all, because its index is what
        // the session file records.
        self.window.active_tab = self.tab_index(active);
        let after = self.slot_starts(now);
        for (old_index, id) in was.into_iter().enumerate() {
            if skip == Some(id) {
                continue;
            }
            let new_index = self.tab_index(id);
            let (Some(old_start), Some(new_start)) = (before.get(old_index), after.get(new_index))
            else {
                continue;
            };
            let delta = old_start - new_start;
            if delta != 0.0 {
                self.window.tabs[new_index]
                    .flip
                    .displace(delta, now, motion);
            }
        }
    }

    /// **N159/K124 and N161/K125 — the strip's half of a tab merging into a
    /// layout.**
    ///
    /// [`absorb_tab_into_layout`] has the tab-to-tab half, N160's two halves
    /// included, and is a free function so that it can be run over two
    /// constructed `TabState`s in a test. This is what only the window can do:
    /// find the source tab, hand the pair over, take the source's entry out of
    /// the strip, and — for N161 — put the tab the displaced pane became into the
    /// strip at the slot the source is vacating.
    ///
    /// **The source tab is removed without [`Runtime::close_tab`], and that is a
    /// ruling rather than a shortcut.** `close_tab` records the tab into Recent,
    /// shuts its shells down and can close the window when it was the last one.
    /// Every one of those is wrong here: the tab did not close, it was **absorbed**
    /// — its shells are alive in another tab, reopening it from Recent would
    /// reopen something that is still running, and the window is not emptier than
    /// it was. So the entry is taken out of the run and nothing else happens, with
    /// the assertion that it is leaving nothing behind (T226).
    ///
    /// **L139 — the displaced pane takes the slot the source tab is vacating.**
    /// One tab left the strip and one arrives, in the same place, which is what
    /// makes a replace read as a trade rather than as two unrelated events. The
    /// slot goes through [`strip_insert_slot`] like every other cross-boundary
    /// insertion, because the strip the arrival lands in is the one the removal
    /// just shortened and its pinned run may now start somewhere else.
    ///
    /// `active_tab` is adjusted for both the removal and the insertion rather
    /// than re-derived, for the reason `move_tab_with_flip` states: it is an
    /// index into a run that just changed length, and the tab it must go on
    /// naming is the one the user is looking at — the target, which does not move.
    pub(crate) fn absorb_tab(
        &mut self,
        source: TabId,
        arrived: &[(SeatId, SeatId)],
        displaced: Option<bt_layout::Seat>,
    ) -> Result<()> {
        let Some(index) = self.window.tabs.iter().position(|tab| tab.id == source) else {
            return Ok(());
        };
        // K129 already forbids a tab being dropped on its own layout, and
        // `leave_strip` is what makes a cross-tab drop reachable at all (J107).
        debug_assert_ne!(
            index, self.window.active_tab,
            "K129: a tab cannot merge into the layout it is already showing"
        );
        // **Minted whether or not the merge ejects anything** (F1b). This used to
        // be a reservation that only became a number when a pane was pushed out;
        // under an allocator that never reuses, a number handed back is a number
        // that can arrive twice, so an unspent one is simply spent. A gap in the
        // numbering is invisible and free.
        let id = self.app.tab_ids.mint();
        let render_physical =
            presentation_physical_size(self.window.renderer.presentation_geometry());
        let renderer = &self.window.renderer;
        // Copied out before the two-tab borrow: the merged layout lands in this
        // same window, so it answers to whoever owns this window's size — and to
        // whatever the window's rail is currently keeping clear.
        let policy = self.window.size_policy;
        let rail = self.rail_posture();
        let chrome = self.platform_chrome();
        let (from, into) = two_tabs_mut(&mut self.window.tabs, index, self.window.active_tab);
        let ejected =
            absorb_tab_into_layout(from, into, arrived, displaced.as_ref(), id, |seats| {
                let (layout, overflow, _, _) =
                    solve_seats(seats, renderer, render_physical, policy, rail, chrome);
                (layout, overflow)
            });
        debug_assert!(
            self.window.tabs[index].sessions.is_empty(),
            "T226: the absorbed tab is leaving with shells still filed under it"
        );
        // **And every page the merged tab was holding** (§7.10 ④‴). `arrived` is
        // the renumbering the plan wrote down, which is exactly the list this
        // needs; the two tabs are the one being absorbed and the one on screen.
        // Without it, a tab merged into another one's layout has its browsers
        // closed a frame later by `advance_web_page`, because it asks whether the
        // page's own tab still has that seat and that tab has just left the strip.
        let into_id = self.window.tabs[self.window.active_tab].id;
        let moved: Vec<(LeafId, LeafId)> = arrived
            .iter()
            .map(|(was, now)| {
                (
                    LeafId {
                        tab: source,
                        seat: *was,
                    },
                    LeafId {
                        tab: into_id,
                        seat: *now,
                    },
                )
            })
            .collect();
        self.carry_the_pages_of_moved_panes(&moved)?;
        self.carry_the_recordings_of_moved_panes(&moved);
        absorb_tab_into_strip(
            &mut self.window.tabs,
            &mut self.window.active_tab,
            index,
            ejected,
        );
        Ok(())
    }

    /// **Let go over the run's padding** — [`DragRelease::Extract`]'s two hands.
    ///
    /// One verdict and two payloads, dispatched here rather than inside
    /// [`release_verdict`], because the verdict is a fact about the *landing* and
    /// this is a fact about the *hand*: the run gains an entry at this slot
    /// either way, and what that entry is made of is the payload's business.
    pub(crate) fn commit_strip_extract(&mut self, drag: &Drag, slot: usize) -> Result<bool> {
        match &drag.source {
            DragSource::Row(payload) => {
                let payload = payload.clone();
                self.commit_row_into_new_tab(&payload, slot)
            }
            DragSource::Pane(_) | DragSource::Tab(_) => self.commit_pane_extract(drag, slot),
        }
    }

    /// **Let go on one of the run's own entries** — [`DragRelease::Adopt`]'s two
    /// hands, and [`Runtime::commit_strip_extract`]'s argument one landing over.
    pub(crate) fn commit_strip_adopt(&mut self, drag: &Drag, target: TabId) -> Result<bool> {
        match &drag.source {
            DragSource::Row(payload) => {
                let payload = payload.clone();
                self.commit_row_into_tab(&payload, target)
            }
            DragSource::Pane(_) | DragSource::Tab(_) => self.commit_pane_adopt(drag, target),
        }
    }

    /// **§7.1.1 — a file row let go on the tab strip becomes a tab** (user
    /// ruling 2026-07-17: 「文件/图片拖到标签条 = 成为新 tab(新工作区含单个预览
    /// pane,缓冲生于新 tab 自己的池,插位钳在 pinned 分区之后)」), and a folder row
    /// becomes a files tab rooted there (「拖到标签条=新 files tab(即刻激活)」).
    ///
    /// **The tab is built through [`create_tab_state`], which is the only door
    /// there is.** The two shapes it is handed are the two `reopen_recent`
    /// already hands it for a `Seed::Preview` and a `Seed::Files` — a lone
    /// Preview leaf, or a lone Files leaf carrying `FILES_W` and the root the
    /// payload names — so "a file tab" and "a folder tab" mean here exactly what
    /// they mean when one comes back out of Recent, rather than nearly that.
    ///
    /// **The buffer is born in the new tab's own pool**, which is the ruling's
    /// own clause and is true by construction rather than by arrangement: a pool
    /// belongs to a `TabState`, this tab is a new one, and the page is opened
    /// after the tab is on the strip and activated — through
    /// [`Runtime::open_preview_onto`], the same door a double-click in the tree
    /// takes. Nothing is taken out of the tab the row was dragged from, and there
    /// is nothing there to take: a row is a *path*, and the tree it came out of
    /// holds no buffer for it.
    ///
    /// **The new tab is activated, and that overrides half of a sentence.**
    /// §7.1.1 says a folder dropped here makes a files tab 「即刻激活」 and says
    /// nothing about a file's; N157's tear-out, the nearest precedent, explicitly
    /// does *not* activate. The user's ruling of 2026-08-30 makes the two one:
    /// dragging a row out of a tree onto the strip is asking to look at that
    /// file, and a tab that opened somewhere behind you is a gesture that
    /// appears to have done nothing. N157's own argument is untouched — a pane
    /// torn out is already on screen, so "take me there" would be taking you
    /// nowhere.
    ///
    /// The slot arrives already clamped past the pinned partition (N158, through
    /// [`strip_insert_slot`]); the `min` here is the ordinary bound on an
    /// insertion index and not a second opinion about the partition, exactly as
    /// it is in [`Runtime::extract_pane_into_new_tab`].
    pub(crate) fn commit_row_into_new_tab(
        &mut self,
        payload: &RowPayload,
        slot: usize,
    ) -> Result<bool> {
        let id = self.app.tab_ids.mint();
        // The id in the literal is a placeholder and the one that comes back is
        // the tab's: [`seats::Seats::lone_seat`] mints its own from 1, and the
        // content below has to be filed under the seat that will actually be
        // drawn. It is the same discipline `plan_drop`'s `arrived` enforces one
        // surface over — never spend a name the tree may not adopt.
        let (seats, seat, files) = match payload.kind {
            RowPayloadKind::File => {
                let (seats, seat) = seats::Seats::lone_seat(&bt_layout::Seat::new(
                    bt_layout::SeatId(1),
                    bt_layout::SeatKind::Preview,
                ));
                (seats, seat, BTreeMap::new())
            }
            RowPayloadKind::Folder => {
                let (seats, seat) = seats::Seats::lone_seat(
                    &bt_layout::Seat::new(bt_layout::SeatId(1), bt_layout::SeatKind::Files)
                        .with_fixed_extent(bt_layout::FILES_W),
                );
                (
                    seats,
                    seat,
                    BTreeMap::from([(
                        seat,
                        seats::FilesLeafState {
                            root: payload.path.display().to_string(),
                            ..seats::FilesLeafState::default()
                        },
                    )]),
                )
            }
        };
        let render_physical =
            presentation_physical_size(self.window.renderer.presentation_geometry());
        let (tab, _) = create_tab_state(
            id,
            seats,
            &self.window.renderer,
            render_physical,
            &self.window.pty_wake,
            None,
            &BTreeMap::new(),
            &files,
            // The page is opened after the tab is standing, through the door
            // every other open goes through — see this function's own note on
            // where the buffer is born.
            &PreviewRestore::default(),
            TabSeed {
                manual_name: None,
                // N158's argument, unchanged: a tab that has just been made by a
                // gesture is not a tab the user has promised to bring back every
                // time.
                pinned: false,
            },
            &self.app.profile_programs,
            &self.default_profile_id(),
            self.window.size_policy,
            self.rail_posture(),
            self.platform_chrome(),
            FormulaSwitches::from_settings(self.app.settings_store.loaded()),
            scrollback_quota(self.app.settings_store.loaded().scrollback_lines),
            self.app.settings_store.loaded().line_wrapping,
        )?;
        let slot = slot.min(self.window.tabs.len());
        self.window.tabs.insert(slot, tab);
        if slot <= self.window.active_tab {
            self.window.active_tab += 1;
        }
        self.apply_window_min_inner_size()?;
        self.activate_tab(slot, true)?;
        if payload.kind == RowPayloadKind::File {
            let surface = self.preview_here(seat);
            self.open_preview_onto(surface, payload.path.clone())?;
        }
        Ok(true)
    }

    /// **§7.1.1's second tab-strip verb — a row let go *on* a tab opens there**
    /// (user ruling 2026-08-30, and §7.1.6k's spring is what carries the hand to
    /// it: rest a quarter second on an entry and the view goes there with the
    /// payload still in the air).
    ///
    /// **Neither half of this is written here.** A file goes through
    /// [`Runtime::open_preview_file`] — the tree's own double-click — which is
    /// precisely the ruling's two cases in one door: the tab's landing preview
    /// pane if it has one (§7.1.1's 「预览 pane 中心=在该 pane 打开」), and a
    /// fresh preview split out at the fixed right seat if it has not (§7.1.1's
    /// 「预览固定右席」). A folder goes through
    /// [`Runtime::seat_a_files_column`], which is §7.1.1's own space verb for a
    /// folder — 「裂出新 files pane 根在该目录」 — arriving where and as wide as
    /// `Ctrl+Shift+B` puts one: the **leading** side of the root rim at
    /// `FILES_W`, never the trailing side the preview's fixed seat takes.
    ///
    /// **The two halves are asymmetric, and §7.1.1 is where the asymmetry comes
    /// from rather than this function.** A file has a reuse target and it is a
    /// ruling — P95's unlocked preview pane — so "open it in that tab" has
    /// somewhere to land before it splits anything. A folder has none: the
    /// drag's folder verbs are 「裂出新 files pane」 for a *space* and
    /// 「重新扎根」 only at **a files pane's own centre**, and a strip entry is
    /// not a pane centre. Re-rooting from here would also be the hard verb
    /// reached without the question about range that the ruling of 2026-08-25
    /// put in front of it (see `re_rooting_is_reached_only_after_the_question_
    /// about_range`), which is a door this gesture has no business opening.
    ///
    /// **The target is activated first, and this is where §7.1.6k's "the target
    /// is not activated" stops applying.** A pane adopted into another tab is a
    /// *pane*: it is visible in that tab's strip badge and waits there. A
    /// document opened into a tab you are not looking at is invisible, so the
    /// ruling that activates the new-tab half activates this half by the same
    /// sentence — you dragged a file out to look at it. It is also what makes
    /// the two doors above reachable at all: both are written against the tab on
    /// the stage, which is where every other reader of them means.
    pub(crate) fn commit_row_into_tab(
        &mut self,
        payload: &RowPayload,
        target: TabId,
    ) -> Result<bool> {
        let Some(index) = self.tab_slot_of(target) else {
            return Ok(false);
        };
        self.activate_tab(index, false)?;
        match payload.kind {
            RowPayloadKind::File => self.open_preview_file(payload.path.clone())?,
            RowPayloadKind::Folder => {
                // `None` is the solver refusing, which the survey already asked
                // about ([`Runtime::row_adopt_fits`]) — so it is reported the way
                // every refusal between a survey and a release is, by the gesture
                // having done nothing.
                self.seat_a_files_column(payload.path.display().to_string())?;
            }
        }
        Ok(true)
    }

    /// **A pane leaves one tab's tree and lands in another's, at a named aim** —
    /// the window's half of [`pane_into_tab`], and the one door both cross-tab
    /// gestures go through.
    ///
    /// The two gestures are §7.1.6k's *"松在 tab 上"* (the tab list, aiming at the
    /// end of the tree) and §7.1.6k′'s *"落在布局上"* (the stage, aiming at a
    /// zone). They differ in the [`seats::LayoutAim`] their callers hand in and
    /// in nothing else — which is the whole reason this is one function: the
    /// strip surgery below is fiddly, order-dependent and easy to get subtly
    /// wrong, and two copies of it would be two chances to.
    ///
    /// What is left here is what only a window can do: find the two entries, mint
    /// the id an eviction would spend, hand over the solver, and put back into
    /// the run whatever the move took out of it or pushed out of a tree.
    pub(crate) fn move_pane_across_tabs(
        &mut self,
        leaf: LeafId,
        target: TabId,
        aim: seats::LayoutAim,
    ) -> Result<bool> {
        let (Some(from), Some(into)) = (self.tab_slot_of(leaf.tab), self.tab_slot_of(target))
        else {
            return Ok(false);
        };
        if from == into {
            return Ok(false);
        }
        let metrics = self.seat_metrics();
        let arrival = PaneArrival {
            metrics: &metrics,
            viewport: self.window.seat_viewport,
            aim,
        };
        let watching = self.window.tabs[self.window.active_tab].id;
        let render_physical =
            presentation_physical_size(self.window.renderer.presentation_geometry());
        let renderer = &self.window.renderer;
        let policy = self.window.size_policy;
        let rail = self.rail_posture();
        let chrome = self.platform_chrome();
        let (source, host) = two_tabs_mut(&mut self.window.tabs, from, into);
        let moved = pane_into_tab(source, host, leaf.seat, &arrival, Some(watching), |seats| {
            let (layout, overflow, _, _) =
                solve_seats(seats, renderer, render_physical, policy, rail, chrome);
            (layout, overflow)
        });
        let Some(moved) = moved else {
            return Ok(false);
        };
        debug_assert!(
            self.window.tabs[into].seats.tree().contains(moved.landed),
            "§7.1.6k: the pane landed under an id its new tree does not have"
        );
        debug_assert!(
            moved
                .traded
                .is_none_or(|stood_in| self.window.tabs[from].seats.tree().contains(stood_in)),
            "B4: the pane traded for is not in the tree the traveller left"
        );
        // **Both journeys carry their pages** (§7.10 ④‴), and both are named
        // here because both are one re-key of a table that is the window's and
        // not a tab's. The trade's pair is read off the aim — a centre is the
        // only landing that displaces anybody, and the seat it displaces is the
        // one it aimed at — so there is no second search of a tree for it. One
        // call and not two, because a trade fills the traveller's old id with the
        // pane it traded with: the two re-keys have to be one transaction or the
        // second one lands on the first one's key.
        let mut moves = vec![(
            leaf,
            LeafId {
                tab: target,
                seat: moved.landed,
            },
        )];
        if let (seats::LayoutAim::SeatCentre(displaced), Some(stood_in)) = (aim, moved.traded) {
            moves.push((
                LeafId {
                    tab: target,
                    seat: displaced,
                },
                LeafId {
                    tab: leaf.tab,
                    seat: stood_in,
                },
            ));
        }
        self.carry_the_pages_of_moved_panes(&moves)?;
        self.carry_the_recordings_of_moved_panes(&moves);
        if moved.source_emptied {
            // The tab did not close — it was emptied by a move, and `close_tab`
            // would file a still-running shell into Recent and shut it down. This
            // is [`Runtime::absorb_tab`]'s door, for [`absorb_tab_into_strip`]'s
            // stated reason.
            //
            // **Nothing takes the vacated slot any more** (B4). The one thing
            // that ever did was a pane a centre had evicted, and a centre now
            // trades instead — which also means this branch and a centre are
            // mutually exclusive, because a trade never empties the tab it
            // traded out of.
            let follow = if watching == leaf.tab {
                target
            } else {
                watching
            };
            absorb_tab_into_strip(
                &mut self.window.tabs,
                &mut self.window.active_tab,
                from,
                None,
            );
            if let Some(index) = self.tab_slot_of(follow) {
                // Forced: the removal may already have left `active_tab` naming
                // this index, and what is owed is the whole way in — a re-solve, a
                // grid for the shells, a title — rather than the index alone.
                self.activate_tab(index, true)?;
            }
        }
        self.settle_seat_set_change()?;
        Ok(true)
    }

    /// The move itself, with no drag around it.
    ///
    /// Split out when `Move pane to new tab` joined the drag as a second door
    /// onto this verb (user ruling, 2026-08-16). One verb, two ways of asking —
    /// which is this window's rule for every gesture that also has a menu row,
    /// and it matters more here than usual: the whole point of the row is that
    /// the pane *moves*, sessions and scrollback intact, and a second
    /// implementation is exactly how a move quietly becomes a respawn.
    ///
    /// **The id comes back** (multiwindow slice F1c), because `Move pane to new
    /// window` is this verb and then the application's transfer, and the tab the
    /// second half moves is the one the first half made. `None` is the layout
    /// refusing to let the pane out — a lone pane, whose tree `close_seat` will
    /// not empty — and it is a fact the caller needs rather than a failure: a
    /// lone pane is already a whole tab, so the journey to a window of its own
    /// simply starts one step further along.
    pub(crate) fn extract_pane_into_new_tab(
        &mut self,
        leaf: LeafId,
        slot: usize,
    ) -> Result<Option<TabId>> {
        let metrics = self.seat_metrics();
        // Spent whether or not the layout lets the pane out — [`Runtime::absorb_tab`]'s
        // reason, and the same allocator.
        let id = self.app.tab_ids.mint();
        let render_physical =
            presentation_physical_size(self.window.renderer.presentation_geometry());
        let renderer = &self.window.renderer;
        // **The pane's own tab and not the one on screen** (§7.1.6k): the spring
        // can have moved the view since the hand closed, and a tear-out reaches
        // into the tree the pane is actually standing in.
        let Some(source) = self.tab_slot_of(leaf.tab) else {
            return Ok(None);
        };
        let seat = leaf.seat;
        let motion = self.app.motion;
        let policy = self.window.size_policy;
        let rail = self.rail_posture();
        let chrome = self.platform_chrome();
        let torn = tear_pane_into_tab(
            &mut self.window.tabs[source],
            &metrics,
            seat,
            id,
            Instant::now(),
            motion,
            |seats| {
                let (layout, overflow, _, _) =
                    solve_seats(seats, renderer, render_physical, policy, rail, chrome);
                (layout, overflow)
            },
        );
        let Some(torn) = torn else {
            return Ok(None);
        };
        // **The name the pane now answers to**, read off the tab that was just
        // built rather than re-derived: `seats::Seats::lone_seat` mints it, and a
        // tab with one pane has one seat, which is the seat its keyboard is on.
        let landed = LeafId {
            tab: id,
            seat: torn.focused_leaf,
        };
        // The survey already clamped this against the same strip (N158); the
        // `min` is the ordinary bound on an insertion index, not a second opinion
        // about the partition.
        let slot = slot.min(self.window.tabs.len());
        self.window.tabs.insert(slot, torn);
        if slot <= self.window.active_tab {
            self.window.active_tab += 1;
        }
        // **And the half that does not live on a `TabState`** (§7.10 ④‴): a page
        // is the window's, so `pane_into_new_tab` cannot have carried it and this
        // is where it follows the pane it is drawn in. A recording is the
        // window's for the same reason and follows by the same door (§7.44 ⑮).
        self.carry_the_pages_of_moved_panes(&[(leaf, landed)])?;
        self.carry_the_recordings_of_moved_panes(&[(leaf, landed)]);
        debug_assert!(
            self.sessions_match_terminals(),
            "item 6: a tear-out leaves the tab it left matching its own tree"
        );
        self.settle_seat_set_change()?;
        Ok(Some(id))
    }

    /// Where a tab is now, by identity.
    fn tab_index(&self, tab: TabId) -> usize {
        self.window
            .tabs
            .iter()
            .position(|candidate| candidate.id == tab)
            .unwrap_or(self.window.active_tab)
    }

    /// Open the tab-name editor (J99-J101, mock-up 5854-5870).
    fn open_rename(&mut self, tab: TabId) -> Result<()> {
        let Some(index) = self
            .window
            .tabs
            .iter()
            .position(|candidate| candidate.id == tab)
        else {
            return Ok(());
        };
        // Mock-up 5858-5859: a tab with no session to name does not open an
        // editor. This was J104's stub — "the guard exists and is asked, and
        // T5's files-only tab will be the first thing it turns away" — and T5
        // arrived, so it is a live gate. A folder tab and a file tab are
        // identified by a path on disk, which is not a field anybody can type
        // into; a tab whose identity leaf this build cannot read seeds nothing
        // at all, and a name written onto nothing could not be saved either.
        if !self.window.tabs[index]
            .seed()
            .is_some_and(|seed| seed.can_be_named())
        {
            return Ok(());
        }
        self.window.rename = Some(TabRename::open(
            tab,
            self.window.tabs[index].manual_name.as_deref(),
        ));
        // A caret that arrives mid-blink arrives invisible half the time.
        self.window
            .rename_blink
            .reset(Instant::now(), self.app.motion);
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// Close the editor, writing the draft through or throwing it away.
    ///
    /// "Escape restores and leaves; Enter and blur commit. Two paths, not
    /// three" (mock-up 5847-5849) — two outcomes, and [`RenameExit`]'s three
    /// exits, because the address field brought a door that can say no. Doing
    /// nothing when no editor is open is the point rather than an oversight:
    /// this is called from every blur-shaped event in the window, and most of
    /// the time there is nothing to blur.
    pub(crate) fn finish_rename(&mut self, exit: RenameExit) -> Result<()> {
        let Some(editor) = self.window.rename.take() else {
            return Ok(());
        };
        let commit = exit.commits();
        // **The file the preview head names** (user ruling 2026-08-19). It
        // leaves through the same two paths a tab's name does — Enter, Escape
        // and blur — because it is the same editor; what differs is only what
        // committing writes, which here is a file moving on disk.
        // **B5 puts a second row on the same subject** (user ruling 2026-08-25):
        // the breadcrumb's last segment is that very file, so it commits through
        // this arm rather than growing one of its own. Two arms would be two
        // chances for a name to be moved on disk by different rules.
        if let RenameSubject::PreviewName { surface, source }
        | RenameSubject::PreviewCrumb { surface, source } = &editor.subject
        {
            if commit {
                self.rename_preview_file(*surface, source, editor.text())?;
            }
            self.refresh_chrome();
            return self.present_chrome_change();
        }
        // **A row of a files column** (B5). Not the arm above, because a row is a
        // *place on the disk* and not a document: there is no buffer to re-key,
        // no preview to follow, and the thing being renamed may be a folder.
        if let RenameSubject::FilesRow { leaf, key } = &editor.subject {
            if commit {
                let (leaf, key) = (*leaf, key.clone());
                self.rename_files_row(leaf, &key, editor.text())?;
            }
            self.refresh_chrome();
            return self.present_chrome_change();
        }
        // **A file or a folder that does not exist yet** (0.3). Not the arm
        // above, because there is nothing to move: what commits is a `create`,
        // and what a refusal means is different in kind. A rename refused leaves
        // the name it had; this one has no name to leave, so **Enter on a draft
        // the folder will not take keeps the field open** — the address bar's
        // sentence, for the address bar's reason, and it is Enter's sentence and
        // not blur's for that ruling's reason too: a blur has already left, and
        // re-opening the field it was leaving is how a box becomes unclosable.
        if let RenameSubject::FilesNew {
            leaf,
            parent,
            folder,
            ..
        } = &editor.subject
        {
            if commit {
                let (leaf, parent, folder) = (*leaf, parent.clone(), *folder);
                // **And the refusal is written onto the editor before the
                // editor goes back** (D8(a)): the box's own advisory is a
                // prediction of this answer, and the one thing a reader must
                // never be shown is a field that looks valid with Enter doing
                // nothing.
                if let Some(refusal) =
                    self.create_files_row(leaf, &parent, folder, editor.text())?
                    && exit.may_stay_open()
                {
                    let mut editor = editor;
                    editor.refuse(refusal);
                    self.window.rename = Some(editor);
                    self.refresh_chrome();
                    return self.present_chrome_change();
                }
            }
            self.refresh_chrome();
            return self.present_chrome_change();
        }
        // **The address a page's head names** (§7.7 ②). The same two paths, and
        // a third thing that is not a path: an address the door will not take
        // **leaves the editor open**. Enter on a refused address does nothing —
        // 「回车什么都不做、原来的页面原地不动」 — and the field goes on saying so
        // where it is being typed, which is the search capsule's own answer to a
        // regex that will not parse.
        //
        // **And that is Enter's sentence, not blur's** ([`RenameExit`], user
        // report 2026-08-24). A blur has already left; re-opening the field it
        // was leaving is how a `file:` page — refused by the address door on
        // every attempt, so every `.html` this product opens — became a name
        // cell nothing could close.
        if let RenameSubject::WebAddress { leaf } = editor.subject {
            // **An empty box is not an address the door refused — it is a draft
            // nobody finished** (§7.7 ⑨). The refusal above keeps the field open
            // so that what was typed can be corrected where it was typed, and
            // that is only an answer when something *was* typed: an empty field
            // reopened by its own blur is a field that cannot be left. It is the
            // box `Ctrl+Shift+L` opens on a page that has never been anywhere, so
            // the rule had to be stated before that door could exist at all —
            // and `would_go_to` had already stated it from the other side, where
            // an empty field is the one wrong-looking thing that does not light
            // up red.
            if commit && !editor.text().trim().is_empty() {
                let engine = self.app.settings_store.loaded().search_engine;
                let compositor_outcomes = {
                    let window = &mut *self.window;
                    window
                        .web
                        .get_mut(&leaf)
                        .map(|web| web.go_to(editor.text(), engine, &window.compositor))
                };
                if let Some((taken, outcomes)) = compositor_outcomes {
                    if taken {
                        // The seat has been asked to go somewhere, so it is a
                        // page whatever comes back — a failure has a card of its
                        // own to stand on it. Spent before the outcomes are
                        // applied, because one of them can be the commit that
                        // would ask again.
                        self.forget_a_blank_page(leaf);
                        self.apply_web_outcomes(leaf, outcomes)?;
                    } else if exit.may_stay_open() {
                        // Enter, refused: the field stays exactly where the
                        // typing is, and the page does not move. Nothing else on
                        // this path runs — a blank page kept for a field that is
                        // still open is a blank page still being used.
                        self.window.rename = Some(editor);
                        self.refresh_chrome();
                        return self.present_chrome_change();
                    }
                    // A blur the door refused falls through: the address was not
                    // taken, so the page does not move — and the field goes,
                    // because leaving is what a blur already is.
                }
            }
            // Escape, a click away, or an empty box: the field is gone, and a
            // blank page that only ever existed to hold it goes with it.
            self.withdraw_a_blank_page(leaf)?;
            self.refresh_chrome();
            return self.present_chrome_change();
        }
        if commit
            && let Some(index) = self
                .window
                .tabs
                .iter()
                .position(|tab| Some(tab.id) == editor.tab())
        {
            let name = editor.committed_name();
            if self.window.tabs[index].manual_name != name {
                self.window.tabs[index].manual_name = name;
                // The seed reads `manual_name` (`TabState::term_leaf`), so the
                // vault, the session file and the restore prompt all pick the
                // new name up from here without a second write — and the OS
                // window title is the active tab's own.
                if index == self.window.active_tab {
                    self.window.window.set_title(&self.display_title());
                }
                self.mark_session_dirty(Instant::now());
            }
        }
        // Unconditional, exactly as the mock-up's `finish` is (5885-5889): the
        // commonest exit is opening the editor, changing your mind and clicking
        // away, where the state is byte-identical to before and only the drawing
        // is wrong.
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// Route an event-loop tick to the press promise and the rename caret.
    pub(crate) fn advance_tab_press_if_due(&mut self, now: Instant) -> Result<()> {
        let matured = self
            .window
            .tab_press
            .as_mut()
            .is_some_and(|press| press.matured(now));
        if !matured {
            return Ok(());
        }
        let tab = self
            .window
            .tab_press
            .expect("a press that matured is a press")
            .tab;
        self.activate_tab(self.tab_index(tab), false)
    }

    pub(crate) fn advance_rename_blink_if_due(&mut self, now: Instant) -> Result<()> {
        if self.window.rename.is_none() || !self.window.rename_blink.advance(now) {
            return Ok(());
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// **Where a tab is now**, given the id it was named by.
    ///
    /// `None` when it has gone, which is an answer and not a failure: a palette
    /// row naming a tab whose shell exited while the reader was typing should
    /// do nothing, rather than do something to whichever tab moved up into its
    /// place.
    pub(crate) fn tab_index_of(&self, tab: TabId) -> Option<usize> {
        self.window.tabs.iter().position(|found| found.id == tab)
    }

    /// A wheel notch over the tab strip, turned into horizontal motion (A7/A8).
    pub(crate) fn scroll_tab_strip(&mut self, delta: MouseScrollDelta) -> Result<()> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let width = self
            .window
            .renderer
            .presentation_geometry()
            .swapchain_size
            .0 as f32;
        let geometry = seats::tab_strip_geometry(
            width,
            scale,
            self.platform_chrome(),
            &self.tab_trailers(Instant::now()),
            self.window.active_tab,
            self.window.tab_scroll,
        );
        let travel = match delta {
            MouseScrollDelta::LineDelta(x, y) => {
                // A strip has no lines of its own to count, so a notch moves one
                // wheel-amount of *this product's* line — the same distance the
                // terminal under it would have moved. A notch that changed length
                // depending on what it was over is a distance the hand has to
                // relearn at every surface.
                let line = self.line_height_subpixels().get() as f32
                    / bt_viewport::SUBPIXELS_PER_PX as f32;
                let amount =
                    match recoverable_wheel_scroll_amount(bt_platform::wheel_scroll_amount()) {
                        bt_platform::WheelScrollAmount::Lines(lines) => lines as f32 * line,
                        // A page of a horizontal scroller is a screenful of it.
                        bt_platform::WheelScrollAmount::Page => geometry.viewport[1],
                    };
                // A horizontal wheel says what it means. A vertical one over a
                // scroller that only has a horizontal axis is the case that has
                // to be translated, and translating it is why a one-axis mouse
                // can reach the far end of the strip at all.
                if x != 0.0 { x * amount } else { y * amount }
            }
            MouseScrollDelta::PixelDelta(position) => {
                // A trackpad gesture already speaks pixels, and it has both axes:
                // honour whichever one the fingers actually moved along.
                let (x, y) = (position.x as f32, position.y as f32);
                if x.abs() >= y.abs() { x } else { y }
            }
        };
        // Wheel-up reveals what lies to the left, which is a smaller offset.
        let scrolled = (self.window.tab_scroll - travel).clamp(0.0, geometry.max_scroll);
        if scrolled == self.window.tab_scroll {
            return Ok(());
        }
        self.window.tab_scroll = scrolled;
        // The strip moved under a stationary pointer, so what it is over changed
        // without the pointer having done anything.
        if let Some(position) = self.window.pointer_position {
            self.window.seat_pointer.hover = self.chrome_target_at(position);
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Retire shells that have exited, and the panes and tabs they emptied.
    ///
    /// Two levels now, because a shell exiting is a fact about a *pane*. A tab
    /// whose right-hand pane's shell exits loses that pane and keeps running;
    /// only when a tab has no live shell left has the tab itself ended. That is
    /// the same rule §7.1.4 already gives closing — the last pane closing is the
    /// tab closing — read from the other direction.
    pub(crate) fn reap_exited_tabs(&mut self) -> Result<()> {
        // Which panes of the *active* tab died: those are the ones that can be
        // closed as panes, because `close_pane` re-solves the tab the user is
        // looking at.
        let active = self.window.active_tab;
        let mut exited_panes = Vec::new();
        for (seat, leaf) in self.window.tabs[active].leaves_mut() {
            let Some(pty) = leaf.pty.as_mut() else {
                continue;
            };
            if pty.try_wait()?.is_some() {
                exited_panes.push(*seat);
            }
        }
        // Never close the last one here: an empty tab is not a state, and
        // `close_pane` routes that case to `close_tab` on its own.
        if self.window.tabs[active].sessions.len() > exited_panes.len() {
            for seat in exited_panes {
                self.close_pane(seat)?;
            }
        }

        let mut exited = Vec::new();
        for (index, tab) in self.window.tabs.iter_mut().enumerate() {
            // A tab has ended when every shell it holds has ended. A tab in
            // probe mode holds no PTY at all and never ends this way.
            let mut any_live = false;
            let mut any_pty = false;
            for (_, leaf) in tab.leaves_mut() {
                let Some(pty) = leaf.pty.as_mut() else {
                    continue;
                };
                any_pty = true;
                if pty.try_wait()?.is_none() {
                    any_live = true;
                }
            }
            if any_pty && !any_live {
                exited.push(index);
            }
        }
        for index in exited.into_iter().rev() {
            self.close_tab(index)?;
            if self.window.tabs.len() == 1 && index == 0 {
                break;
            }
        }
        Ok(())
    }

    /// The second arm of [`Self::open_address_here`]: **a tab with no page gets
    /// one, empty, and the caret lands in its address.**
    ///
    /// The page is the host's own blank ([`webnav::Mint::Blank`]) and it goes
    /// out through [`Self::open_minted_page`], which is the door every navigation
    /// this window starts passes — a URL this build wrote itself gets no more
    /// trust than one a person typed (`plan.md` §3). Where it lands is the
    /// preview landing rule's answer and not a new one, because a page arriving
    /// in a tab is a page arriving in a tab however it was asked for.
    ///
    /// **The field opens empty and nothing arranges that.** `about:blank` is
    /// this host's scaffolding, and `WebSeat`'s `SourceChanged` already refuses
    /// to put it in the head — so the address the field is seeded from is the
    /// empty string, which is what a person who has not typed an address has.
    pub(crate) fn mint_a_blank_page_and_open_its_address(&mut self) -> Result<()> {
        // Read before the page lands, because landing is what destroys both
        // facts: the pane it takes over stops showing what it was showing, and a
        // pane that did not exist a moment ago is indistinguishable afterwards
        // from one that did.
        let landing = self.seats.landing_preview();
        let was_showing = landing.and_then(|seat| {
            self.preview_pane(self.preview_here(seat))
                .and_then(|pane| pane.buffer.clone())
        });
        let back = blank_page_return(landing.is_some(), was_showing);
        self.open_minted_page(webnav::Mint::Blank)?;
        // The seat the landing rule chose, asked again rather than remembered
        // from above: `open_minted_page` is allowed to decline (no pane could be
        // opened), and a receipt for a page that was never made would be a door
        // with nothing behind it. Asked *again* and not reused from `landing`,
        // because `landing` is `None` in exactly the case a pane was minted.
        let Some(seat) = self.page_on_the_landing_pane() else {
            return Ok(());
        };
        let leaf = self.leaf_here(seat);
        self.window.blank_page = Some(BlankPage { leaf, back });
        self.open_web_address_on(leaf)
    }

    /// **The blank page was never given an address, so it goes** (§7.7 ⑨,
    /// recommended by Claude and open to being overruled).
    ///
    /// Opening a door is a gesture, and a gesture that cannot be abandoned is a
    /// gesture that costs something to try. Escape and a click away close the
    /// field; this is what they leave behind, which is the tab exactly as it was
    /// before the chord.
    ///
    /// **What "never given an address" means is not this function's opinion.**
    /// [`webhost::WebSeat::identity`] is `WebMachine::recoverable_url`, the one
    /// field a *successful* navigation writes, and the two lines that write it
    /// name `about:blank` in order to refuse it. So a page that has been
    /// somewhere — including somewhere that failed and left a card standing on
    /// the seat — is a page, and this leaves it alone.
    fn withdraw_a_blank_page(&mut self, leaf: LeafId) -> Result<()> {
        if self.window.blank_page.as_ref().map(|door| door.leaf) != Some(leaf) {
            return Ok(());
        }
        let went_somewhere = self
            .window
            .web
            .get(&leaf)
            .is_none_or(|web| web.identity().is_some());
        // **Only on this page's own tab.** The two verbs below name a pane
        // through the tab in front — a seat number is unique inside a tab and
        // nowhere else — and stepping to another tab with the field still open
        // is stepping away from the gesture. The receipt is spent either way:
        // the field it belonged to is gone, and a blank page nobody withdrew is
        // a page like any other, with an address bar of its own to type into.
        if went_somewhere || leaf.tab != self.id {
            self.window.blank_page = None;
            return Ok(());
        }
        let back = self
            .window
            .blank_page
            .take()
            .expect("the door was there one line ago")
            .back;
        // The engine goes first and by the call the orphan sweep already makes
        // ([`Self::advance_web_page`]): whatever the pane becomes below, it
        // stops being a page here, in this turn, rather than a tick later.
        let window = &mut *self.window;
        let outcomes = window
            .web
            .get_mut(&leaf)
            .map(|web| web.close(&window.compositor))
            .unwrap_or_default();
        self.apply_web_outcomes(leaf, outcomes)?;
        let surface = PreviewSurface::Seat(leaf);
        match back {
            // `close_pane` and not a bare `close_seat`: it is the one verb for a
            // leaf leaving a tree, and the pane this door minted leaves exactly
            // as the `×` on its head would make it leave.
            BlankPageReturn::TakeThePane => self.close_pane(leaf.seat),
            // The document lane's own door, so the caret and the scroll the pane
            // filed on its way out are the ones it comes back to.
            BlankPageReturn::PutTheDocumentBack(source) => {
                let Some(name) = self
                    .preview_pool
                    .get(&source)
                    .map(|buffer| buffer.name.clone())
                else {
                    return Ok(());
                };
                self.land_preview_source_on(surface, source, name)
            }
            // An empty preview pane is what was there, so an empty preview pane
            // is what is left.
            BlankPageReturn::LeaveThePaneEmpty => Ok(()),
        }
    }

    /// **The door was walked through, so there is nothing to take back.**
    ///
    /// Its own name rather than a `= None` at three call sites, because the three
    /// are one sentence — this seat has been asked to go somewhere — and a
    /// receipt left lying about is a page that would be withdrawn by the *next*
    /// field to close over the same seat number.
    pub(crate) fn forget_a_blank_page(&mut self, leaf: LeafId) {
        if self.window.blank_page.as_ref().map(|door| door.leaf) == Some(leaf) {
            self.window.blank_page = None;
        }
    }
}
