//! `peek` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{
    DragLatch, FilePeek, FilePeekPress, FilePeekSubject, FloatDrag, FloatDragKind, FloatGrasp,
    LeafId, MathWorkerRequest, PeekBodyKind, PeekCacheEntry, PeekCandidate, PeekClock, PeekFacts,
    PeekFootPress, PeekPageOutcome, PeekPageRaster, PeekPageSlot, PeekSubject, PeekThumbnail,
    PeekThumbnailTarget, PreviewPane, PreviewSurface, ReferenceCard, RowHost, Runtime,
    ScaleWorkerRequest, ScrollThumbState, ShellAddress, facts_of_a_file_the_user_chose, file_peek,
    file_peek_promotion, files, float, git, git_document_question, git_panel, hang_watch, i18n,
    input, mark_opacity, marks, peek_body_kind, peek_cache_key_for_decode, peek_foot_press,
    peek_page_texture_key, peek_scale_task, peek_strip, preview, preview_body_bar, profiles,
    ring_arc, risen_frame, scroll_bar_layer, seats, session_is_breathing, tooltip, wait_pulse,
};
use anyhow::Context;
use anyhow::Result;
use bt_doc::Bias;
use bt_layout::SeatId;
use bt_render::{FrameSource, FrameTrigger, PeekImageOverlay};
use bt_term::normalized_local_image_path_key;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::dpi::PhysicalPosition;
use winit::event::MouseButton;

impl Runtime<'_> {
    /// Whether this tab has a layout worth showing, and whether now is a moment
    /// to show it (L131).
    ///
    /// One predicate, read by both the arming path and the retiring one, because
    /// the two asking different questions is exactly how a popup survives the
    /// death of its own subject.
    pub(in crate::runtime) fn layout_peek_eligible(&self, tab: usize) -> bool {
        let Some(state) = self.window.tabs.get(tab) else {
            return false;
        };
        peek_strip::eligible(
            state.seats.pane_count(),
            tab == self.window.active_tab,
            // **The card column is the card rail `eligible`'s own doc says this
            // window does not have** (user ruling, 2026-08-21). Read off the
            // posture rather than off `window.focus_mode`, from the same
            // [`Self::rail_posture`] the solver and every geometry are handed,
            // so "is a card column on screen" has one author here as well.
            //
            // Fed in rather than tested at the call sites: `layout_peek_eligible`
            // is the one predicate both the arming path and the retiring one
            // read (that is this function's whole reason), and a second `if
            // focus` in `layout_peek_target_at` would be exactly the pair of
            // paths asking different questions that this function exists to
            // prevent — a peek settled a frame before the mode came on would
            // then have nobody to retire it.
            self.rail_posture().draws_focus_rail(),
            // A drag owns the pointer outright — the rule the tip, the hyperlink
            // underline and the terminal's own selection already live by.
            self.window.drag.is_some(),
            // The editor IS the answer, exactly as it is for the tip: a
            // schematic laid over the box you are typing a name into covers the
            // box you are typing it into.
            self.window
                .rename
                .as_ref()
                .is_some_and(|editor| editor.tab() == Some(state.id)),
        )
    }

    /// The tab a peek would belong to, if the pointer is on one that qualifies.
    ///
    /// A tab's own controls count as the tab: `pointerenter`/`pointerleave` do
    /// not fire for a child, so in the mock-up the pointer crossing onto the pin
    /// never leaves the tab, and the schematic stays up.
    ///
    /// `&mut self` because [`Self::chrome_target_at`] is the float-aware router
    /// now and a window's claim is measured, not remembered — which is also what
    /// makes "a tab a float is covering is not under the pointer" true here
    /// without a word about floats.
    pub(in crate::runtime) fn layout_peek_target_at(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Option<usize> {
        let tab = match self.chrome_target_at(position)? {
            seats::ChromeTarget::Tab(index)
            | seats::ChromeTarget::TabPin(index)
            | seats::ChromeTarget::TabClose(index) => index,
            _ => return None,
        };
        self.layout_peek_eligible(tab).then_some(tab)
    }

    /// Track the tab under the pointer (L131, L135).
    pub(in crate::runtime) fn note_layout_peek(&mut self, tab: Option<usize>) -> Result<()> {
        if self.window.layout_peek.observe(tab, Instant::now()) && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Show a settled peek — and, on the same beat, silence the tip.
    ///
    /// §6's whole mechanism is this one line. The peek is due at 350ms and the
    /// tip at 380ms, so promotion always happens first, and promotion is where
    /// the tip stands down. It is *disarmed* rather than merely left undrawn:
    /// a candidate held past its own deadline would report that deadline
    /// forever, and a `WaitUntil` on an instant already in the past is a loop
    /// that never sleeps.
    pub(in crate::runtime) fn advance_layout_peek_if_due(&mut self, now: Instant) -> Result<()> {
        if !self.window.layout_peek.activate_if_due(now) {
            return Ok(());
        }
        self.window.tooltip.hide();
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Take the peek down — any press, a lost window, a drag starting (L135).
    pub(in crate::runtime) fn hide_layout_peek(&mut self) -> Result<()> {
        if self.window.layout_peek.hide() && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Whether a showing peek has already answered for this anchor.
    ///
    /// The other half of §6, and the half that handles the pointer *moving*
    /// inside a tab whose peek is up: promotion silenced the tip once, and this
    /// is what stops the next mouse-move from arming it again.
    pub(in crate::runtime) fn layout_peek_suppresses(
        &self,
        anchor: tooltip::TooltipAnchorId,
    ) -> bool {
        peek_strip::suppresses(self.window.layout_peek.active(), anchor)
    }

    /// The peek's own layer, or nothing when none is showing.
    ///
    /// Everything is read out of *this* frame — the tree, the names, the focus,
    /// the breath — for the tip's reason: a schematic that remembered the frame
    /// it appeared on would keep showing a pane that has since closed.
    pub(in crate::runtime) fn layout_peek_layer(&mut self) -> Vec<marks::OverlayLayer> {
        let Some(index) = self.window.layout_peek.active() else {
            return Vec::new();
        };
        let now = Instant::now();
        let motion = self.app.motion;
        let scale = self.window.renderer.scale_factor() as f32;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        // The peek hangs off the row the pointer is on, and *which* geometry
        // holds that row is the same question `chrome_target_at` already asks
        // before it can say the pointer is on a row at all. Asking it here too
        // is what stops the card being placed by the horizontal strip's
        // arithmetic while the pointer is in the rail: `tab_strip_geometry`
        // still answers in vertical layout — it is a pure function of a width
        // and a trailer list, and knows nothing about a rail being on screen —
        // so it handed back slot 0's box up in the title bar, and the card was
        // drawn in the window's top-left corner across the rail it belonged to.
        let (host, side) = match self.window.rail.layout {
            seats::TabLayoutMode::Vertical => {
                let Some(rail) = self.rail_geometry_now(now) else {
                    return Vec::new();
                };
                let Some(row) = rail.tabs.get(index).map(|row| row.body) else {
                    return Vec::new();
                };
                (row, peek_strip::PeekSide::Beside)
            }
            seats::TabLayoutMode::Horizontal => {
                let geometry = seats::tab_strip_geometry(
                    width as f32,
                    scale,
                    self.platform_chrome(),
                    &self.tab_trailers(now),
                    self.window.active_tab,
                    self.window.tab_scroll,
                );
                let Some(host) = geometry.tabs.get(index).map(|slot| slot.body) else {
                    return Vec::new();
                };
                (host, peek_strip::PeekSide::Below)
            }
        };
        let Some(tab) = self.window.tabs.get(index) else {
            return Vec::new();
        };
        // The breath belongs to the strip's clock, and the *elapsed* half of it
        // is sampled once here so the mark in the schematic and the mark on the
        // tab are at the same point of the same 1.7s — two clocks would beat
        // against each other. Only "is this one working?" is asked per pane,
        // which is what makes a schematic of several shells honest: the tab's
        // mark breathes if any of them is running (`fleet_working`), and each
        // row of the schematic breathes if *it* is.
        let elapsed = tab.animation_elapsed(now);
        let focus = tab.seats.focus();
        // Each preview *seat*'s own caption, because a schematic draws the tab's
        // tree — a float is not in it, and two preview leaves in one tree are two
        // rows naming two files. Both doors, as the head reads them (P36): the
        // picture's title when the seat is showing one, the buffer's name
        // otherwise.
        let preview_title = |id: SeatId| {
            let pane = tab.preview_panes.get(tab.preview_here(id))?;
            match pane.image.as_ref() {
                Some(image) => Some(image.title()),
                None => tab
                    .preview_pool
                    .get(pane.buffer.as_ref()?)
                    .map(|buffer| buffer.name.clone()),
            }
        };
        // The peek reads the caption through the same door the head does, and
        // now out of the same per-seat map, so a schematic can never name a pane
        // something the pane itself does not — nor name two panes alike, which is
        // what one cwd for the whole tab did.
        let terminal_names = tab.terminal_names();
        let files_names = tab.files_names();
        let palette = bt_render::chrome_palette();
        // False for every tab a peek can be showing — `peek_strip::eligible`
        // refuses the active one outright — but asked rather than assumed, so
        // the claim a leaf makes here is the same fact the strip computes from
        // the same ledger, arrived at the same way.
        let tab_is_active = index == self.window.active_tab;
        let leaves: Vec<peek_strip::PeekLeaf> = tab
            .seats
            .tree()
            .seats_in_order()
            .iter()
            .map(|seat| {
                // Only a terminal has a session that can be working, reporting
                // or unread, and only its own session can say so. This is the
                // whole of what the peek adds over the tab: the tab wears the
                // loudest claim of its fleet, and the schematic says which pane
                // is making it.
                let session = tab.sessions.get(&seat.id);
                let status = session.map(|leaf| leaf.session.status());
                // No tween: a tab's arc eases toward a new reading because it
                // has somewhere to ease *from*, and that memory is per tab. The
                // peek has no per-leaf memory to ease from and, being a snapshot
                // that appears and disappears whole, nothing to ease during — so
                // it shows the reading itself rather than an approach to it.
                // Asked once and spent twice: the dot this leaf wears and
                // whether that dot breathes are two questions about one claim,
                // and folding the fleet a second time is how the two start
                // disagreeing (§7.1.6b′: 不新写第二套聚合).
                let claim = session
                    .map(|leaf| leaf.session_facts(tab_is_active).claim())
                    .unwrap_or_default();
                let ring = status.and_then(|status| status.progress).map(|state| {
                    let arc = ring_arc(state, None, elapsed, motion, &palette);
                    seats::TabRing {
                        arc: arc.color,
                        start_milliturns: arc.start_milliturns,
                        sweep_milliturns: arc.sweep_milliturns,
                    }
                });
                peek_strip::PeekLeaf {
                    kind: seat.kind,
                    // This leaf's own shell, off the session that is running in
                    // it — the same map every other per-seat fact in this frame
                    // comes from.
                    profile_mark: session
                        .map(|leaf| profiles::mark(profiles::index_of_id(&leaf.profile))),
                    // The short name, and C28's own two lengths are why. A pane
                    // head has a whole bar and answers "where is this" with the
                    // place entire; this popup is a 210px thumbnail whose names
                    // answer "which one is this", which is the question the last
                    // segment answers. Written with `seat_caption` it printed
                    // whole paths into a box built for words, and a name wider
                    // than the box ran straight out over the terminal — the
                    // fourth reader of `seat_short_caption`, beside the drag
                    // ghost, the drop preview and a collapsed bar, and a label
                    // for the same reason all three are.
                    title: {
                        let preview_title = preview_title(seat.id);
                        seats::seat_short_caption(
                            seat.kind,
                            preview_title.as_deref(),
                            terminal_names.get(&seat.id).map(String::as_str),
                            files_names.get(&seat.id).map(String::as_str),
                        )
                        .to_owned()
                    },
                    focused: seat.id == focus,
                    mark_opacity: mark_opacity(
                        // Through the same predicate the tab's own breath is
                        // taken over, so a schematic row cannot breathe beside a
                        // tab mark that has stood down — the schematic exists to
                        // say *which* pane is making the tab's claim, and a row
                        // making a claim the tab does not is the one thing it
                        // must never do.
                        status.is_some_and(session_is_breathing),
                        // Was hard-coded `false` while a peek leaf had no ring
                        // to be replaced by. Now that it can have one, the
                        // breath has to stand down for it exactly as the tab's
                        // does, or a ring would be drawn pulsing.
                        ring.is_some(),
                        elapsed,
                        motion,
                    ),
                    dot: claim.dot(&palette),
                    // **The same claim's other answer**, asked here rather than
                    // guessed from the dot's colour: `Bell` and `Awaiting` wear
                    // one warn and only the second is a program standing still
                    // (§7.1.5b). One sampler for the whole window — the tab's
                    // card, its strip chip, its rail row and this schematic all
                    // spend `wait_pulse`'s reading of the same elapsed time, so
                    // no two of them can be caught at different phases of one
                    // breath.
                    pulse: claim.pulses().then(|| wait_pulse(elapsed, motion)),
                    ring,
                }
            })
            .collect();
        let tree = tab.seats.tree().clone();

        // Only the font knows how wide a name is, so the measuring happens here,
        // beside the renderer, exactly as the tip's does.
        let font_px = peek_strip::LIST_FONT_LOGICAL_PX * scale;
        let widths: Vec<f32> = leaves
            .iter()
            .map(|leaf| {
                self.window
                    .renderer
                    .measure_chrome_text(&mut self.app.gpu, &leaf.title, font_px)
            })
            .collect();
        let Some(layout) = peek_strip::layout(
            &tree,
            &leaves,
            &widths,
            host,
            side,
            (width as f32, height as f32),
            scale,
        ) else {
            return Vec::new();
        };
        peek_strip::build(&layout, &leaves, &palette, scale)
    }

    /// **Ask the decoration worker for one file's native pixels**, down whichever of the two
    /// decoders can read it (user ruling 2026-08-27; §7.23).
    ///
    /// One door for both surfaces that show pixels — the preview pane and the glance card — and
    /// therefore the one place the fork between the picture decoder and the video decoder is
    /// written. Both fill the same cache under the same key, so the callers below this line do not
    /// know which of them answered and must not: what they asked for is "this file's pixels", and
    /// a second reading of the name at the *delivery* end is how a `.png` and a `.mp4` would come
    /// to disagree about who owns an entry.
    ///
    /// The name is read with [`preview::path_names_a_video`], which is the very predicate
    /// [`preview_open_lane`] forks on, so a file that opened as a video is decoded as one.
    ///
    /// Answers whether the question went out. `false` is a worker that has stopped or a channel
    /// that has closed, which each caller turns into its own kind of silence.
    pub(in crate::runtime) fn request_peek_pixels(&self, path: &Path) -> bool {
        // **The door itself asks whether this is a file to read** (routes A and E of the
        // untrusted-path audit, 2026-09-08). Three surfaces reach this line — the preview pane,
        // the glance card and a markdown page's own pictures — and each of them used to decide
        // locality for itself or not at all, so a share reached the decoder through whichever of
        // the three had not asked. One door, one question: what goes out of here is a request to
        // open a file, and a request nobody may make is one this window does not send.
        if !preview::is_readable_unasked(path) {
            return false;
        }
        if !self.app.math_worker_running {
            return false;
        }
        let leaf = self.focused_shell_address();
        let path = path.to_owned();
        let request = if preview::path_names_a_video(&path) {
            MathWorkerRequest::PeekVideoFrame { leaf, path }
        } else {
            MathWorkerRequest::PeekImage { leaf, path }
        };
        self.app.math_worker.tasks.send(request).is_ok()
    }

    /// The pointer is resting on this row — arm the 350ms, or leave a running
    /// intent alone (P146).
    ///
    /// Three refusals, and each is one of the mock-up's own lines:
    ///
    /// * **A directory row does not glance** (`row.dataset.dir === "1"` →
    ///   `hideFilePeek()`): there is nothing to show of a folder that the row
    ///   itself is not already showing.
    /// * **The same row does not restart the intent** (`row === fpeekRow`):
    ///   crossing from a row's triangle to its name to its trailing space is
    ///   three hover events over one row, and an intent that restarted on each
    ///   would never mature.
    /// * **A drag arms nothing**, which is the first line of `showFilePeek`'s
    ///   guard and the same rule every hover in this window follows.
    ///
    /// Answers whether a card that was **on screen** came down, which the caller
    /// owes a frame for. Arming an intent owes nothing — nothing is drawn for
    /// 350ms — and that asymmetry is the whole of what this returns: the paint
    /// debt is for the card, not for the timer.
    pub(in crate::runtime) fn observe_file_peek(
        &mut self,
        host: Option<(RowHost, usize)>,
        now: Instant,
    ) -> bool {
        let at = self
            .window
            .pointer_position
            .map(|point| [point.x as f32, point.y as f32]);
        // **A card on screen answers before the rows do** (user ruling,
        // 2026-08-14), and it answers for the whole of its corridor rather than
        // for its own rectangle.
        //
        // The rectangle alone would not do, and the reason is the gesture this
        // ruling exists to permit. The card is hung eight pixels above the row
        // and stands off its right edge, so a hand reaching for the *middle* of
        // a 264px card travels right and **down** — across a dozen rows of the
        // very tree it came from. If each of those rows took the card as it
        // passed, the card would be pulled down and re-armed under the hand that
        // was reaching for it, and the reach would never land: exactly the
        // failure the corridor was drawn to make impossible, wearing a different
        // hat. So while the card is alive and the pointer is inside its
        // corridor, **another row neither takes the card nor restarts the
        // intent** — the ruling's own words, and the only reading of them that
        // leaves the reach possible.
        //
        // **What it does not cost any more** (user ruling, 2026-08-14). It used
        // to cost those rows outright: a file listed just under the one being
        // glanced could not be glanced until the card came down, and the only
        // way out was to leave the corridor. The rule above now outranks a hand
        // *crossing* the rows and not a hand that *stops* on one — see
        // `dwell_file_peek`, which is the whole of the `Kept` arm's second line
        // — so travelling through still costs nothing and resting on a row
        // still buys that row's own card.
        match self.file_peek_life(at) {
            // Inside the card itself. The rows under it are the rows the card is
            // *drawn over*, and a hand standing in a card is not resting on the
            // tree behind it — so the dwell below is deliberately not asked.
            Some(file_peek::Life::Held) => return self.keep_file_peek(),
            // In the corridor: on the card's row, on the gap, or on one of the
            // rows the reach crosses. The card stays whatever happens next, and
            // *which* of those it is decides only whether a dwell is counting.
            Some(file_peek::Life::Kept) => {
                self.dwell_file_peek(host, now);
                return self.keep_file_peek();
            }
            // A hand that has started carrying something takes the card down
            // now, with no grace: a grace is a courtesy to a hand still reaching
            // for the card, and this one is busy.
            Some(file_peek::Life::Gone) => return self.hide_file_peek(),
            // Outside the corridor. Whatever is under the pointer down there is
            // free to arm its own card, and if nothing is, the grace starts.
            Some(file_peek::Life::Released) | None => {}
        }
        let Some((host, index)) = host.filter(|_| self.window.drag.is_none()) else {
            return self.release_file_peek(now);
        };
        let Some((key, _, _)) = self.peek_row(host, index) else {
            return self.release_file_peek(now);
        };
        if self
            .window
            .file_peek
            .as_ref()
            .is_some_and(|peek| peek.host == host && peek.key == key)
        {
            // The card's own row, reached from outside its corridor — which is
            // only possible while the intent is still counting down, since a
            // card that exists contains its row. Nothing to do but let the
            // clock run: an intent that restarted here would never mature.
            return self.keep_file_peek();
        }
        let taken = self.hide_file_peek();
        self.window.file_peek = self.armed_file_peek(host, index, now);
        taken
    }

    /// **The hand crossed the foot's address**, in either direction: a frame
    /// is owed so the strip lights or goes dark under it (owner report
    /// 2026-09-23). Read against the painter's own receipt, so a move that
    /// stays on one side of the edge owes nothing — the card is otherwise drawn
    /// identically wherever the hand rests in it.
    ///
    /// Asked by both pointer callers right after [`Self::observe_file_peek`],
    /// on whatever card that left standing.
    pub(in crate::runtime) fn relight_file_peek_foot(&self) -> bool {
        let lit = self.file_peek_foot_grasp();
        self.window
            .file_peek
            .as_ref()
            .is_some_and(|peek| peek.foot_lit != lit)
    }

    /// **The card this row would put up**, armed and not yet matured.
    ///
    /// One construction site for the two ways a glance begins: the pointer
    /// coming to rest on a row with nothing showing ([`Self::observe_file_peek`])
    /// and the pointer coming to rest on *another* row with a card already up
    /// ([`Self::dwell_file_peek`]). The second is the first re-walked — "缓冲、
    /// 文档、滚动全新", the ruling's own words — and re-walking it is only
    /// literally true while there is one place that walks it.
    ///
    /// `None` for everything that is not a file row of a rooted tree: a
    /// directory, a notice, a column pointed at nothing, a tree that has not been
    /// laid out yet.
    fn armed_file_peek(&mut self, host: RowHost, index: usize, now: Instant) -> Option<FilePeek> {
        let (key, name, source) = self.peek_row(host, index)?;
        let rect = self.peek_row_rect(host, index)?;
        Some(FilePeek {
            host,
            source,
            key,
            name,
            rect,
            clock: PeekClock::Settling(now + Duration::from_millis(file_peek::PEEK_INTENT_MS)),
            frame: None,
            body: None,
            head: None,
            foot: None,
            foot_lit: false,
            column: None,
            closing_at: None,
            thumb_grab: None,
            dwell: None,
        })
    }

    /// **What a glance over this row would be about** — its identity, the name
    /// on the card's head, and the document behind it.
    ///
    /// One function for all four hosts, because "which row is this and what is
    /// it about" is one question however the row is drawn. `None` for every row
    /// that offers no glance: a directory, a notice, a branch, a commit, a
    /// heading, a column pointed at nothing, a cell of a terminal with no
    /// verified file named on it.
    fn peek_row(
        &mut self,
        host: RowHost,
        index: usize,
    ) -> Option<(String, String, preview::PreviewSource)> {
        match host {
            // **A reference in the output is a row naming a file** (user ruling
            // 2026-08-27, §7.29), and the three things this function owes are
            // read off the one resolution the anchor is read off — so the card
            // that comes up and the run it stands beside cannot be about two
            // different references.
            //
            // A folder reaches here as `None` on purpose and not by omission:
            // its card is a *window* rather than this card, opened on the
            // float's own clock ([`Runtime::float_trigger_at`]), which is the
            // same division the files column makes when it refuses a directory
            // row a glance because the tree beneath it is already the answer.
            RowHost::Terminal(seat) => {
                let reference = self.terminal_reference_at(seat, u32::try_from(index).ok()?)?;
                let ReferenceCard::File(path) = reference.card else {
                    return None;
                };
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                );
                Some((reference.key, name, preview::PreviewSource::file(path)))
            }
            RowHost::Column(_) | RowHost::Float(_) => {
                let (root, rows) = self.host_rows(host)?;
                if root.is_empty() {
                    return None;
                }
                let row = rows
                    .get(index)
                    .filter(|row| matches!(row.kind, files::RowKind::File))?;
                let (key, name) = (row.key.clone(), row.name.clone());
                let path = files::full_path(&root, &key);
                Some((key, name, preview::PreviewSource::file(path)))
            }
            RowHost::Git(seat) => self.git_peek_row(seat, index),
        }
    }

    /// Where the row a glance would stand beside is.
    ///
    /// The tree's geometry for a tree and the page's for the page — each read
    /// from the same derivation its own painter used, which is what keeps the
    /// card beside the row instead of beside where the row used to be.
    fn peek_row_rect(&mut self, host: RowHost, index: usize) -> Option<[f32; 4]> {
        match host {
            RowHost::Column(_) | RowHost::Float(_) => {
                Some(self.row_geometry(host)?.row_rect(index))
            }
            RowHost::Git(seat) => {
                let scale = self.window.renderer.scale_factor() as f32;
                let rect = seats::files_pane_rect(&self.seat_layout, seat)?;
                let body = seats::files_pane_geometry(rect, scale, true).body;
                let page = self.window.git_pages_shown.get(&seat)?;
                Some(git_panel::git_panel_geometry(body, page, scale).row_rect(index))
            }
            // The grid's own, through the one resolution [`Self::peek_row`] also
            // goes through: the cells the underline is lit on, on the row the
            // hand is on.
            RowHost::Terminal(seat) => Some(
                self.terminal_reference_at(seat, u32::try_from(index).ok()?)?
                    .rect,
            ),
        }
    }

    /// **The pointer resting on another row while a card is up** (user ruling,
    /// 2026-08-14: *穿行不换,停留即换*).
    ///
    /// The corridor's rule is that a row under the reach may neither take the
    /// card nor restart its intent ([`file_peek::corridor`]), and it had to be
    /// that way round or the reach could never land. What it cost was the rows
    /// themselves: a file listed just below the one being glanced could not be
    /// glanced at all until the card came down. This pays that back without
    /// giving the corridor up, by telling the two gestures apart — a hand
    /// *crossing* those rows on its way to the card, and a hand that has
    /// *stopped* on one of them.
    ///
    /// The clock is [`file_peek::PEEK_INTENT_MS`], the same 350ms the first card
    /// was armed on, because it is the same question asked of the same hand. It
    /// runs against the row rather than against the pointer, so crossing three
    /// rows starts three clocks and finishes none — see [`file_peek::dwell`].
    ///
    /// The card is untouched here whatever the answer: a dwell that matures is
    /// fired by the tick ([`Self::switch_file_peek`]), which is the only place
    /// that can, because a hand at rest sends no events for a pointer path to
    /// notice. Nothing is drawn for a dwell either, so this owes no frame — the
    /// same asymmetry [`Self::observe_file_peek`] reports for arming an intent,
    /// and the reason it answers nothing at all.
    fn dwell_file_peek(&mut self, host: Option<(RowHost, usize)>, now: Instant) {
        // A row of the card's own is not another row, and neither is a folder,
        // a notice, or the empty space past the end of the list: all of them
        // answer `Idle` and drop whatever was counting, so the card is neither
        // moved nor taken down *on this account*.
        //
        // Whether it survives at all is the corridor's answer, made one line up
        // and untouched here. A folder is worth a word, because the tree's own
        // order settles it: directories sort above files, and a card hangs
        // *below* the row it is about — so a folder row is essentially never
        // inside a living card's envelope, and moving onto one leaves the
        // corridor. What happens then is the standing behaviour and not this
        // ruling's: the grace starts, and the card comes down when it runs out
        // (real-machine capture, 2026-08-14).
        let over = host
            .filter(|_| self.window.drag.is_none())
            .and_then(|(host, index)| {
                let (key, _, _) = self.peek_row(host, index)?;
                let showing = self
                    .window
                    .file_peek
                    .as_ref()
                    .is_some_and(|peek| peek.host == host && peek.key == key);
                (!showing).then_some((host, index, key))
            });
        let row = over.as_ref().map(|(host, _, key)| (*host, key.clone()));
        let waiting = self
            .window
            .file_peek
            .as_ref()
            .and_then(|peek| peek.dwell.as_ref())
            .map(|dwell| (dwell.host, dwell.key.clone()));
        match file_peek::dwell(row.as_ref(), waiting.as_ref()) {
            // Already counting on this very row: leave the clock alone, exactly
            // as a running intent is left alone. **This is 穿行不换** — a hand
            // crossing rows never reaches this arm twice on one row.
            file_peek::Dwell::Keep => {}
            file_peek::Dwell::Idle => {
                if let Some(peek) = self.window.file_peek.as_mut() {
                    peek.dwell = None;
                }
            }
            file_peek::Dwell::Start => {
                let (host, index, _) = over.expect("a start is a start on some row");
                let next = self.armed_file_peek(host, index, now).map(Box::new);
                if let Some(peek) = self.window.file_peek.as_mut() {
                    peek.dwell = next;
                }
            }
        }
    }

    /// **The dwell has run out: the card moves to the row the hand stopped on.**
    ///
    /// The switch is the arm re-walked and nothing else — the pending card was
    /// built by [`Self::armed_file_peek`] when the dwell started, so promoting it
    /// is a move rather than a construction, and everything the old card was
    /// holding (its buffer, its parsed document, its scroll, its picture) is let
    /// go by [`Self::hide_file_peek`] on the way past. "缓冲、文档、滚动全新" is
    /// therefore true by construction rather than by a list of fields to clear.
    ///
    /// Fired from the tick because a hand that has come to rest sends no pointer
    /// events at all: the very gesture this exists for is the one that would
    /// never be noticed by a pointer path.
    fn switch_file_peek(&mut self, now: Instant) -> bool {
        let due = self
            .window
            .file_peek
            .as_ref()
            .and_then(|peek| peek.dwell.as_ref())
            .and_then(|dwell| dwell.clock.due());
        if due.is_none_or(|due| due > now) {
            return false;
        }
        let Some(next) = self
            .window
            .file_peek
            .as_mut()
            .and_then(|peek| peek.dwell.take())
        else {
            return false;
        };
        self.hide_file_peek();
        self.window.file_peek = Some(*next);
        // Straight onto the glass rather than into another 350ms: the hand has
        // already spent that on the row, and a card that made it spend it twice
        // would be charging for the same wait.
        self.mature_file_peek(now)
    }

    /// Whether a card is **on screen and holding the pointer** — the card's own
    /// hit test, and the first question every pointer path asks about it.
    ///
    /// An intent that has not matured holds nothing: there is no card yet, and
    /// the rectangle a previous one left behind is not a place to stand.
    pub(in crate::runtime) fn file_peek_holds(&self, at: [f32; 2]) -> bool {
        self.window.file_peek.as_ref().is_some_and(|peek| {
            peek.clock.is_shown()
                && peek
                    .frame
                    .is_some_and(|frame| file_peek::contains(frame, at))
        })
    }

    /// The card is being touched or stood in: cancel any grace and keep it.
    ///
    /// Returns whether a frame is owed, which is only true when a grace was
    /// actually running — the card is drawn identically either way, so a card
    /// that was never dying costs nothing to save.
    fn keep_file_peek(&mut self) -> bool {
        self.window
            .file_peek
            .as_mut()
            .is_some_and(|peek| peek.closing_at.take().is_some())
    }

    /// [`file_peek::life`] for the card that is **on screen**, or `None` when
    /// there is not one.
    ///
    /// The decision itself is over there, as a value, so that the corridor's rule
    /// can be read and tested without a window; this side owns only the question.
    ///
    /// An intent that has not matured answers `None` rather than any of the four:
    /// there is nothing drawn to be gentle with, the rectangle a previous card
    /// left behind is not a place to stand, and a grace held over an unborn card
    /// would fire it over a row the pointer left long ago.
    fn file_peek_life(&self, at: Option<[f32; 2]>) -> Option<file_peek::Life> {
        let peek = self.window.file_peek.as_ref()?;
        let frame = peek.frame.filter(|_| peek.clock.is_shown())?;
        Some(file_peek::life(
            peek.rect,
            frame,
            at,
            self.window.drag.is_some(),
        ))
    }

    /// The pointer is outside a living card's corridor and there is nothing under
    /// it to arm a new one: start the grace, or drop an intent that never
    /// matured.
    ///
    /// Returns whether a frame is owed. Starting a grace owes none — the card is
    /// drawn identically while it runs — and that is the same asymmetry
    /// [`Self::observe_file_peek`] reports for arming an intent.
    fn release_file_peek(&mut self, now: Instant) -> bool {
        let Some(peek) = self.window.file_peek.as_ref() else {
            return false;
        };
        if !peek.clock.is_shown() || peek.frame.is_none() {
            return self.hide_file_peek();
        }
        let peek = self.window.file_peek.as_mut().expect("just borrowed");
        if peek.closing_at.is_none() {
            peek.closing_at = Some(now + float::FLOAT_CLOSE_GRACE);
        }
        false
    }

    /// The grace has run out — take the card down.
    ///
    /// Returns whether anything changed, so the tick that calls it knows whether
    /// it owes a frame.
    fn expire_file_peek(&mut self, now: Instant) -> bool {
        if self
            .window
            .file_peek
            .as_ref()
            .and_then(|peek| peek.closing_at)
            .is_none_or(|due| due > now)
        {
            return false;
        }
        self.hide_file_peek()
    }

    /// The intent has matured: read what the card will show (P147).
    ///
    /// **The pool answers first**, and that is P145's "shows the tab's POOL
    /// buffer when the file is already open, so the glance never lies about
    /// unsaved edits". Only when the pool has never heard of the file does the
    /// glance take a body of its own — one slot, off the pool entirely, so that
    /// running the pointer down a list of names cannot evict the eight buffers
    /// the user actually opened (P119).
    ///
    /// Returns whether anything changed.
    fn mature_file_peek(&mut self, now: Instant) -> bool {
        let Some(peek) = self.window.file_peek.as_ref() else {
            return false;
        };
        if peek.clock.due().is_none_or(|due| due > now) {
            return false;
        }
        let (source, name) = (peek.source.clone(), peek.name.clone());
        let git_seat = match peek.host {
            RowHost::Git(seat) => Some(seat),
            RowHost::Column(_) | RowHost::Float(_) | RowHost::Terminal(_) => None,
        };
        // **The fade's epoch is set here and nowhere else** (owner's ruling
        // 2026-09-13): this is the one line in the window that puts a card on
        // screen, so it is the one line that can say when the card appeared.
        // The 350ms wait is unchanged and the fade starts where it ends.
        if let Some(peek) = self.window.file_peek.as_mut() {
            peek.clock = PeekClock::Shown(now);
        }
        // The card's surface is pointed at the file here, once, rather than on
        // every frame that draws it: [`PreviewSurface::Peek`]'s pane is what
        // holds the parsed document, and re-aiming it per frame would be a
        // document key that changed per frame and a markdown file re-parsed at
        // sixty hertz.
        self.window.peek_pane = PreviewPane {
            buffer: Some(source.clone()),
            ..PreviewPane::default()
        };
        if let Some(pooled) = self.preview_pool.get_mut(&source) {
            // A restored pool remembers names, never text (P151) — so "in the
            // pool" is not "has something to show". A buffer that still wants
            // its head read gets the same request a stranger would; the answer
            // lands in the pool and the card lights up through the same
            // repaint path every head read already takes. (User-reported
            // 2026-08-14: hovering a restored, never-opened pool entry showed
            // an empty card forever.)
            let wants_read = pooled.claim_head_read();
            self.window.peek_buffer = None;
            if let Some(want) = wants_read {
                let tab = self.id;
                if !self.app.preview_worker.request(preview::PreviewRequest {
                    window: self.window_id(),
                    tab,
                    source,
                    want,
                }) {
                    self.disable_preview_worker();
                }
            }
            return true;
        }
        // **The glance's buffer, not a pane's** — see
        // [`preview::PreviewBuffer::glancing`]. The one thing it does differently
        // is read a page whose bytes are text (`.html`) as text, which is the
        // 2026-08-25 ruling and is why the card can show source that no seat in
        // this window ever shows.
        let mut buffer = preview::PreviewBuffer::glancing(source.clone(), name);
        let wants_read = buffer.claim_head_read();
        self.window.peek_buffer = Some(buffer);
        // **A composed document waits for git and never for a disk.** A glance
        // over a commit's file is a `GitShow`, and `wants_head_read` answers
        // `false` for it by construction — so without this the card would sit
        // empty for as long as the hand did. The question is the one the *pane*
        // would ask (`git_document_question`), on the column the glance is over,
        // and its answer comes home through `apply_git_results`.
        if let Some(seat) = git_seat
            && let Some(question) = git_document_question(&source, None)
        {
            let tab = self.id;
            if !self.app.git_worker.request(git::GitRequest {
                window: self.window_id(),
                host: git::GitHost::Column(LeafId { tab, seat }),
                question,
            }) {
                self.disable_git_worker();
            }
            return true;
        }
        if let Some(want) = wants_read {
            let tab = self.id;
            if !self.app.preview_worker.request(preview::PreviewRequest {
                window: self.window_id(),
                tab,
                source,
                want,
            }) {
                self.disable_preview_worker();
            }
        }
        true
    }

    /// **Every way the glance ends** (P149): leave, press, scroll, drag, and the
    /// window losing focus.
    ///
    /// One door, because "gone on leave/press/scroll/drag" is one sentence about
    /// one card, and four call sites each clearing two of the three fields is
    /// how a card outlives the row it was about.
    ///
    /// Answers whether a card that was **showing** came down — a matured peek,
    /// not a timer — because that is the only case that owes a frame.
    pub(crate) fn hide_file_peek(&mut self) -> bool {
        let showing = self
            .window
            .file_peek
            .as_ref()
            .is_some_and(|peek| peek.clock.is_shown());
        // **The card's recording stops with the card** (route B slice ②; §7.44
        // ③) — the pointer left, the card is collapsing, and an engine left
        // behind would be a decoder, a work queue and a Direct3D device
        // belonging to a surface that is no longer on the glass.
        //
        // **Except while it is being carried into a float**, which is the one
        // moment this door runs with the seat still wanted:
        // [`Self::promote_file_peek`] tears the card down *before* it opens the
        // window, so the flag is what tells this door the engine has somewhere
        // to go. Without it a tear-off would shut the decoder down and the float
        // would start another — which is the re-open the ruling refused.
        if !self.window.video_carried_off_the_card {
            self.window.video.close(PreviewSurface::Peek);
        }
        self.window.file_peek = None;
        // And the hand that was on its head. A press whose card has gone has
        // nothing left to promote, and a promotion that fired afterwards would
        // open a window over a file nobody is pointing at any more.
        self.window.file_peek_press = None;
        self.window.peek_buffer = None;
        // And the parsed document with it: a card that is down is holding a
        // markdown layout for a file nobody is looking at.
        self.window.peek_pane = PreviewPane::default();
        self.window.peek_picture = None;
        // And the facts with it: they are about the file the card was over, and
        // the next card asks for its own.
        self.window.peek_facts = None;
        // **The page's pixels stay and its question does not.** A rastered page
        // is worth keeping across a pointer that leaves a row and comes back;
        // what it is not worth is trusting, so the next card over the same file
        // asks the disk once more whether it is still that file. See
        // [`PeekPageSlot`].
        if let Some(slot) = self.window.peek_page.as_mut() {
            slot.asked.clear();
        }
        // And any hand that was on a block inside it. The card's surface is the
        // one the sweep deliberately never retires ([`Self::sweep_preview_panes`]),
        // because its life is this field's — so this is where that life ends for
        // the gestures too.
        if self
            .preview_block_drag
            .is_some_and(|drag| drag.surface == PreviewSurface::Peek)
        {
            self.preview_block_drag = None;
        }
        if self
            .preview_block_hover
            .is_some_and(|(surface, _)| surface == PreviewSurface::Peek)
        {
            self.preview_block_hover = None;
        }
        showing
    }

    /// **What the card is about**, or `None` while the intent is still running —
    /// everything that can be read off the buffer without laying anything out.
    ///
    /// It is derived on the frame that draws it, rather than stored when the peek
    /// was armed: a head read that lands two frames later has to reach a card that
    /// is already on screen, and a buffer being edited in a pane behind the card
    /// has to reach it too. Both are the same fact — the glance is a *view* of a
    /// buffer, never a copy of one.
    pub(in crate::runtime) fn file_peek_subject(&self) -> Option<FilePeekSubject> {
        let peek = self.window.file_peek.as_ref()?;
        if !peek.clock.is_shown() {
            return None;
        }
        // The pool's copy wins whenever there is one — the glance shows the file
        // as this tab has it, edits and all. Asked through the card's *surface*
        // rather than off the peek's path, because that is the same door every
        // other reader of a preview buffer uses and it is the door that knows
        // about the off-pool slot; `mature_file_peek` points the surface at the
        // row in the same breath it clears `due`, so the two cannot disagree.
        let buffer = self.preview_buffer_on(PreviewSurface::Peek)?;
        Some(FilePeekSubject {
            path: peek.source.file_path().map(Path::to_path_buf),
            // The one host whose path came out of a child process — see the field.
            printed_in: match peek.host {
                RowHost::Terminal(seat) => Some(seat),
                RowHost::Column(_) | RowHost::Float(_) | RowHost::Git(_) => None,
            },
            name: peek.name.clone(),
            // **The chip says what the name claims, and when the name claims
            // nothing it says what the bytes said** (user ruling 2026-08-27;
            // §7.32). The first half is the 2026-08-25 ruling untouched: an
            // `.html` glance says `web` in the corner and shows the markup
            // underneath, because the name really does mean page and the body is
            // a judgement about which reader the card uses. The second half is
            // this ruling's, and it is not a second rule but the same one asked
            // of a name with nothing in it: `script.ps1` is in no table, so
            // there is no claim for the chip to print, and printing `unknown`
            // over a card full of legible source is the card contradicting
            // itself.
            ftype: match preview::preview_ftype(&peek.name) {
                preview::PreviewFtype::Unknown => buffer.ftype,
                named => named,
            },
            // A network path or a type this window will not read says so in the
            // card's own one line, exactly as the preview pane's unknown card
            // does — the refusal is the preview's judgement, borrowed.
            refused: buffer.refusal().is_some(),
            dirty: buffer.dirty,
            anchor: self.peek_anchor(peek),
        })
    }

    /// **What the glance stands beside** — its row, or the floating card that
    /// row is drawn inside (user ruling 2026-09-07, `docs/DESIGN.md` §7.58).
    ///
    /// Three of the four hosts are surfaces pinned to an edge of this window, and
    /// their rows are their own anchors: a files column's row runs to the column's
    /// edge, a Git page's row to the page's, and a terminal reference is a run of
    /// cells on the glass. The fourth is a **window** — the folder card, and every
    /// other floating tree — and a row inside a window is not a thing anything can
    /// stand beside: the ten pixels would be measured from a line drawn inside the
    /// card, and the glance would come to rest on that card's own border.
    ///
    /// So the float's frame is the anchor and the row is the height, which is
    /// [`file_peek::PeekAnchor::row_in_a_card`]'s whole argument. It is asked of
    /// the frame **as it is drawn now** — through the rise the window is making if
    /// it is still arriving — for the reason [`Self::row_geometry`] reads the same
    /// number the same way: a card placed against where a window used to be is a
    /// card standing in a gap.
    ///
    /// A float that has gone falls back to the row, which is unreachable in
    /// practice (the card goes with the window it was raised in,
    /// [`Self::forget_dead_float_gestures`]) and is the honest answer if it ever
    /// is not.
    fn peek_anchor(&self, peek: &FilePeek) -> file_peek::PeekAnchor {
        let card = match peek.host {
            RowHost::Float(id) => {
                let scale = self.window.renderer.scale_factor() as f32;
                let now = Instant::now();
                self.window
                    .float
                    .live(id)
                    .map(|win| risen_frame(win.frame, self.float_fade_of(win, now, scale)))
            }
            RowHost::Column(_) | RowHost::Git(_) | RowHost::Terminal(_) => None,
        };
        card.map_or_else(
            || file_peek::PeekAnchor::row(peek.rect),
            |card| file_peek::PeekAnchor::row_in_a_card(peek.rect, card),
        )
    }

    /// **The card's whole body over a video**: one frame of it, and the two lines that say what it
    /// is (user ruling 2026-08-27; §7.23).
    ///
    /// [`Self::file_peek_facts`]'s opposite number and [`Self::file_peek_picture`]'s twin — which
    /// is the point of it being neither of them written twice. The picture comes down the picture
    /// lane, byte for byte the same way a `.png`'s does, because by the time the card sees it a
    /// frame **is** a picture; the two lines come from the facts the same decode filed
    /// ([`WindowRuntime::video_facts`]), which is why they are still there for a container this
    /// machine cannot decode at all.
    ///
    /// It is fitted into the **page** box rather than the picture box, and the difference is a
    /// judgement about what is being looked at: a picture's box leaves the card short because a
    /// photograph is the whole answer, and a video's card carries two lines under the frame, so it
    /// takes the taller ground a PDF's page column already established at the same width.
    fn file_peek_frame(&mut self, path: &Path, scale: f32) -> file_peek::PeekBody {
        let (width, height) = self
            .file_peek_fitted_pixels(
                path,
                scale,
                (
                    file_peek::PEEK_PAGE_W_LOGICAL_PX,
                    file_peek::PEEK_PAGE_H_LOGICAL_PX,
                ),
            )
            .unwrap_or((0.0, 0.0));
        file_peek::PeekBody::Frame { width, height }
    }

    /// **How large the picture in this file really is** — the decode's own
    /// dimensions, or `None` while nothing has decoded it (user ruling
    /// 2026-08-29; §7.29 ⑬).
    ///
    /// The card draws a *resample* fitted into 280×120, and the number a reader
    /// wants beside it is the picture's own — the same fact the preview pane's
    /// meta strip states off `PreviewImageState::native`, read here from the one
    /// place a card has it: the path-keyed native decode
    /// ([`WindowRuntime::peek_cache`]) that [`Self::file_peek_fitted_pixels`] is
    /// already fitting. It asks for nothing: a card with a picture on it has
    /// this entry by construction, and a card whose decode is still out says its
    /// size alone until the pixels land, which is the same silence every other
    /// part of this body keeps.
    fn file_peek_native_pixels(&self, path: &Path) -> Option<(u32, u32)> {
        match self
            .window
            .peek_cache
            .get(&normalized_local_image_path_key(path))?
        {
            PeekCacheEntry::Ready {
                width_px,
                height_px,
                ..
            } => Some((*width_px, *height_px)),
            PeekCacheEntry::Pending | PeekCacheEntry::Failed(_) => None,
        }
    }

    /// **This file's pixels, resampled into `fit`** — or `None` while there are none on the card.
    ///
    /// The shared half of the two bodies above, and every sentence on
    /// [`Self::file_peek_picture`] is about this: the path-keyed native decode in
    /// [`WindowRuntime::peek_cache`] and the one resample lane behind it, asked by the frame,
    /// guarded so that a question already in flight is never asked twice.
    ///
    /// `None` is "draw the ground and no picture" and it is deliberately one word for four
    /// situations — the decode is out, the decode failed, the resample is out, the box has no
    /// extent. There is no "loading" among them for the same reason there is none in the document:
    /// a word that appears for two frames and is replaced is worse than the space it occupied.
    pub(in crate::runtime) fn file_peek_fitted_pixels(
        &mut self,
        path: &Path,
        scale: f32,
        fit: (f32, f32),
    ) -> Option<(f32, f32)> {
        let key = normalized_local_image_path_key(path);
        let (content_key, rgba, native_width, native_height) =
            match self.window.peek_cache.get(&key) {
                Some(PeekCacheEntry::Ready {
                    key,
                    rgba,
                    width_px,
                    height_px,
                    ..
                }) => (key.clone(), Arc::clone(rgba), *width_px, *height_px),
                Some(PeekCacheEntry::Pending | PeekCacheEntry::Failed(_)) => return None,
                None => {
                    if self.request_peek_pixels(path) {
                        self.window.peek_cache.insert(key, PeekCacheEntry::Pending);
                    }
                    return None;
                }
            };
        // The frame the mock-up gives the picture, in whole physical pixels.
        let fit_width = ((fit.0 * scale).round() as u32).max(1);
        let fit_height = ((fit.1 * scale).round() as u32).max(1);
        let (display_width, display_height) =
            bt_render::preview_image_extent(fit_width, fit_height, native_width, native_height)?;
        let fitted = (display_width as f32, display_height as f32);
        let target: PeekThumbnailTarget = (content_key, display_width, display_height);
        if self
            .window
            .peek_picture
            .as_ref()
            .is_some_and(|picture| picture.matches(&target))
        {
            return Some(fitted);
        }
        if self.window.peek_card_pending.as_ref() == Some(&target) || !self.app.math_worker_running
        {
            return None;
        }
        if self
            .app
            .math_worker
            .scale_tasks
            .send(ScaleWorkerRequest::Peek {
                leaf: self.focused_shell_address(),
                task: peek_scale_task(&target, rgba, native_width, native_height),
            })
            .is_ok()
        {
            self.window.peek_card_pending = Some(target);
        }
        None
    }

    /// **The card's whole body over a page**: its pages, and how many of them
    /// there are (user rulings 2026-08-25; the size left this question on
    /// 2026-08-29 for the card's own stat — §7.29 ⑬).
    ///
    /// Asked from the frame for [`Self::file_peek_picture`]'s reason and with the
    /// same guard: the card is rebuilt whenever the chrome is, so the question is
    /// filed against the path the moment it is sent and a frame that finds it
    /// already filed asks nothing. What comes back is one optional number, and
    /// the body is the same shape whether or not it has arrived — see
    /// [`file_peek::PeekBody::Facts`].
    ///
    /// **Neither question is answered on the frame's own thread, and they are not
    /// answered on the same one.** A page count is read off the file's structure
    /// ([`pdf::page_count`]) — a walk over as many bytes as the file has — and
    /// goes down the preview worker's lane, which is a disk. A raster
    /// ([`pdf::page_raster`]) is a parse and a rasterisation and goes to the
    /// decoration worker, beside the formula engine; putting it in front of the
    /// disk's queue would make a preview pane's head read wait on a picture.
    /// Either one on the thread that draws is a hover over a large document
    /// freezing the window.
    fn file_peek_facts(&mut self, path: &Path, scale: f32) -> file_peek::PeekBody {
        let pages = match self
            .window
            .peek_facts
            .as_ref()
            .filter(|facts| facts.path == path)
        {
            Some(facts) => facts.answer,
            None => {
                self.window.peek_facts = Some(PeekFacts {
                    path: path.to_owned(),
                    answer: None,
                });
                let (tab, window) = (self.id, self.window_id());
                if !self.app.preview_worker.request(preview::PreviewRequest {
                    window,
                    tab,
                    source: preview::PreviewSource::file(path.to_owned()),
                    want: preview::PreviewWant::PageCount,
                }) {
                    self.disable_preview_worker();
                }
                None
            }
        };
        // **Clamped here, against the number this very line just read.** The
        // reach grows the frame a page count lands and shrinks the frame a file
        // turns out to be shorter than the one before it; both are answered by
        // the count, so this is the one place that has the old offset and the new
        // extent at once — which is `file_peek_layer`'s own rule about a
        // document, applied to the body that has no document.
        let column = file_peek::peek_page_column_max_scroll(pages.unwrap_or(1), scale);
        self.window.peek_pane.scroll[1] = self.window.peek_pane.scroll[1].clamp(0.0, column);
        // **The pages are asked for after the count and with it.** The count is what says how long
        // the column is, and until it lands the column is one slot — so a request run filed before
        // it would be a run that asked for exactly the cover and then never widened, because the
        // set of pages already asked about does not shrink.
        self.file_peek_pages(path, scale, pages.unwrap_or(1));
        file_peek::PeekBody::Facts {
            scroll: self.window.peek_pane.scroll[1],
            pages,
        }
    }

    /// **The pages this card is showing, put on their way** (user rulings 2026-08-25 and
    /// 2026-08-26).
    ///
    /// Asked from the frame for [`Self::file_peek_picture`]'s reason and guarded the same way —
    /// the card is rebuilt whenever the chrome is, so each question is filed against its page the
    /// moment it goes out and a frame that finds it already filed asks nothing. What is different
    /// is what happens when the pointer comes back: the pixels are still here, they are drawn on
    /// the first frame, and the disk is asked once more in the background whether they are still
    /// the file's. See [`PeekPageSlot`].
    ///
    /// **Only what is in view is asked for** (`file_peek::peek_pages_in_view`). A hundred-page
    /// report is a hundred rasters and tens of seconds of a worker's life, spent on pages nobody
    /// has scrolled to; the column reserves their slots from the *count*, which is cheap, and buys
    /// their pixels one wheel notch at a time. The same range decides what is drawn, so a page
    /// this window is waiting for and a page it is showing a blank sheet for are the same page.
    fn file_peek_pages(&mut self, path: &Path, scale: f32, count: u32) {
        let fit = |logical: f32| ((logical * scale).round() as u32).max(1);
        let fit = (
            fit(file_peek::PEEK_PAGE_W_LOGICAL_PX),
            fit(file_peek::PEEK_PAGE_H_LOGICAL_PX),
        );
        // A slot about another file — or about this one at another size — is not this card's, and
        // is replaced whole rather than edited: its pixels were drawn for a question nobody is
        // asking any more.
        let mine = self
            .window
            .peek_page
            .as_ref()
            .is_some_and(|slot| slot.path == path && slot.fit == fit);
        if !mine {
            self.window.peek_page = Some(PeekPageSlot {
                path: path.to_owned(),
                fit,
                mtime: None,
                asked: BTreeSet::new(),
                pages: Vec::new(),
            });
        }
        if !self.app.math_worker_running {
            return;
        }
        let scroll = self.window.peek_pane.scroll[1];
        let leaf = self.focused_shell_address();
        for index in file_peek::peek_pages_in_view(count, scroll, scale) {
            let slot = self
                .window
                .peek_page
                .as_mut()
                .expect("the slot was just filled");
            // **A page in view is a page wanted**, whether or not it has to be drawn: this is the
            // one place the cache's eviction order is written, and recording it here rather than
            // at the paint is what keeps the run of kept pages centred on the hand instead of on
            // whichever frame ran last.
            let held = slot.wanted(index);
            if !slot.asked.insert(index) {
                continue;
            }
            // `known` only for a page whose pixels are actually here. For any other, "unchanged"
            // would be an answer about a picture this window does not have, and the page would
            // stay blank for as long as the file went untouched.
            let known = held.then_some(slot.mtime).flatten();
            if self
                .app
                .math_worker
                .tasks
                .send(MathWorkerRequest::PeekPage {
                    leaf,
                    path: path.to_owned(),
                    page: index,
                    fit,
                    known,
                })
                .is_err()
                && let Some(slot) = self.window.peek_page.as_mut()
            {
                // Nobody is going to answer, so nothing is out: leave the slot able to ask again
                // rather than waiting for ever on a lane that has gone.
                slot.asked.remove(&index);
            }
        }
    }

    /// **The opacity the glance card should be painted at this instant**, or
    /// `None` when there is no card on screen (owner's ruling 2026-09-13).
    ///
    /// [`Self::tooltip_opacity`]'s twin, and pointedly the *same rule* rather
    /// than the same shape: both read [`tooltip::hover_fade_opacity`], because
    /// the card and the tip are two surfaces summoned by holding still and the
    /// owner's ruling is that everything summoned that way behaves alike. The
    /// card is summoned by *stopping*, so it must never look launched — which is
    /// why the ruling took a bare fade over the mock's other two candidates (a
    /// 4px drop, a scale), both of which say "opened".
    ///
    /// The epoch is the card's own, filed by [`Self::mature_file_peek`] the
    /// instant the 350ms wait ends. That wait is untouched: this is what happens
    /// in the 90ms *after* it.
    fn file_peek_opacity(&self, now: Instant) -> Option<f32> {
        let shown = self.window.file_peek.as_ref()?.clock.shown_at()?;
        Some(tooltip::hover_fade_opacity(
            now.duration_since(shown),
            self.app.motion,
        ))
    }

    /// Whether the card on screen differs from the card last painted — the
    /// strip's own frame-debt question ([`Self::tooltip_owes_frame`]), asked
    /// about this fade.
    ///
    /// This and not "is it still fading" is what schedules the **landing**
    /// frame: the moment the fade ends there is one more frame owed, carrying
    /// the opacity from wherever the last wake left it up to a solid 1.
    fn file_peek_owes_frame(&self, now: Instant) -> bool {
        self.window.file_peek_drawn_opacity != self.file_peek_opacity(now)
    }

    /// When this window next has glance-card work: the settle deadline while one
    /// is armed, or the next frame of a fade that has not landed.
    ///
    /// [`Self::tooltip_deadline`]'s twin, down to the frame interval — a fade
    /// that owes frames owes them at the window's animation rate, and a card
    /// whose fade has landed owes nothing at all. **Hiding owes nothing either**:
    /// the card leaves in one frame, so there is no exit to schedule.
    pub(in crate::runtime) fn file_peek_deadline(&self, now: Instant) -> Option<Instant> {
        if self.file_peek_owes_frame(now) {
            return self.next_animation_deadline();
        }
        let clock = self.window.file_peek.as_ref()?.clock;
        if let Some(due) = clock.due() {
            return Some(self.clamp_animation_deadline(due));
        }
        let shown = clock.shown_at()?;
        self.animating_deadline(
            tooltip::hover_fade_owes_frames(now.duration_since(shown), self.app.motion),
            now,
        )
    }

    /// The 350ms is up — put the card on screen (P145).
    pub(in crate::runtime) fn advance_file_peek(&mut self, now: Instant) -> Result<()> {
        // The card's three clocks, in the order they can fire: the dwell that
        // moves it to another row, the 350ms that puts it up and the grace that
        // takes it down. All through one tick, because a card that matured and
        // expired between two wakes owes exactly one frame and not two.
        //
        // The switch is first because it *is* an arm-and-mature: it puts a fresh
        // card up on the same beat, and asking the maturity clock afterwards is
        // what lets the two share the one frame this owes.
        //
        // **And the fade's own frames**, beside the three clocks and for the
        // tip's reason ([`Self::advance_tooltip_if_due`]): nothing else in this
        // window would wake the loop to finish a 90ms a still hand started, and
        // the frame the fade *lands* on is owed by this question and by no other.
        //
        // **The three clocks are state and are never paced; the fade is the one
        // paced thing here** (closure review O4, 2026-09-18). Behind the gate, a
        // card whose dwell matured while a neighbouring pane was printing did
        // not appear, and one whose grace ran out did not leave — see
        // [`Self::animation_frame_is_due`] for the three things an advancer does.
        let moved = self.switch_file_peek(now)
            | hang_watch::during(hang_watch::Station::ClockFileDwell, || {
                self.mature_file_peek(now)
            })
            | hang_watch::during(hang_watch::Station::ClockFileClose, || {
                self.expire_file_peek(now)
            });
        if !moved && !self.animation_frame_is_due() {
            return Ok(());
        }
        if !moved && !self.file_peek_owes_frame(now) {
            return Ok(());
        }
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The card's layer, or nothing when no glance is up.
    ///
    /// # The order, and why it is this one
    ///
    /// The card shrink-wraps its body and the body is a document, so *how tall
    /// the card is* depends on *how tall the document came out* — and how tall a
    /// document comes out depends on how wide it was laid out, which is the one
    /// thing about the card that is fixed. So: lay the document out at the card's
    /// width in a box as tall as the card could ever be, ask what height it
    /// wanted, size the card, place it, and only then build the body into the box
    /// the placement produced. The second build re-uses the first's parse — the
    /// document's key is its content and its *width*, and the width never changed
    /// — so the two passes cost one.
    /// **The glance card, and the control bar of a recording playing on it**
    /// (route B slice ②, 2026-08-28; §7.44 ②).
    ///
    /// A wrapper rather than a line inside [`Self::file_peek_card_layers`],
    /// because that function returns from five places — one per body kind — and
    /// a bar appended at four of them is a bar missing from the fifth. The bar
    /// goes **after** the card's own layers for the reason a float's does: the
    /// video is drawn over the card's face and under everything the card draws
    /// on top of it, and the bar is the topmost of those.
    pub(in crate::runtime) fn file_peek_layer(
        &mut self,
        below: usize,
        now: Instant,
    ) -> Vec<marks::OverlayLayer> {
        // Rebuilt from nothing on every pass, exactly as the float group's two
        // ledgers are: it is a record of what *this* frame drew, and a stale
        // index in it is a picture drawn into somebody else's layer.
        self.window.file_peek_level = None;
        // And so is the fade's receipt, for the same reason and the tip's
        // ([`Self::tooltip_layer`]): a frame that drew no card painted no
        // opacity, and a number left behind is a frame debt owed for a card that
        // is not there.
        self.window.file_peek_drawn_opacity = None;
        let mut layers = self.file_peek_card_layers();
        if layers.is_empty() {
            return layers;
        }
        // **And a slot of its own for a recording, directly over the card's
        // face** (route B slice ②; §7.44 ③, found on the machine 2026-08-28 —
        // the float's own defect, on the third host).
        //
        // The renderer draws `VideoStage::Overlay(n)` between layer `n`'s ground
        // and layer `n`'s **fills**. Pointed at the card's face, that puts the
        // picture underneath the card's own picture well — which is a fill —
        // and the card shows an empty box with a control bar counting away
        // beneath it. The float carried exactly this defect and was mended
        // exactly this way ([`WindowRuntime::float_video_level`]): a stage index
        // has to name a layer the z-order loop actually reaches, and what this
        // one is for is being reached. It costs no buffer, no draw call and no
        // quad — see `marks::OverlayLayer::default`.
        //
        // Written **before** the push, because that is the slot the push is
        // about to take, and `below` is handed in for
        // [`OverlayStack::below_the_file_peek`]'s reason: the index is a fact
        // about the stack, and only the chrome pass has a view of the stack.
        self.window.file_peek_level = Some(below + layers.len());
        layers.push(marks::OverlayLayer::default());
        // **The ▶ on the card, and the ruling that put it there** (user
        // 2026-08-28: *「能动的就动」*). §7.23's card drew a first frame and said
        // out loud that winding it was not what that build did. It is what this
        // build does, on the card as much as anywhere else — so the card wears
        // the same disc a pane does, and pressing it starts the engine where the
        // card stands.
        //
        // Above the slot, with the bar, for the float's reason: a layer paints
        // its quads before the picture it carries, and the picture here is a
        // video drawn over the slot's own (empty) ground.
        layers.extend(self.video_play_mark_layer(PreviewSurface::Peek));
        layers.extend(self.video_bar_layer(PreviewSurface::Peek));
        // **The card fades in as one thing** (owner's ruling 2026-09-13, option
        // B of the four-way motion mock): `opacity 0 -> 1` over
        // [`tooltip::TOOLTIP_FADE`] on the tip's own `ease`, no travel and no
        // scale, and instant on the way out.
        //
        // Folded here, over **every** layer the card put down, rather than
        // handed to `file_peek::build` for the card's face alone: the face, the
        // scroll bar beside its document, the ▶ on a recording and that
        // recording's control bar are one surface arriving, and a fade that
        // reached the face would have left a solid bar hanging in the air over a
        // card that was not there yet. It is the argument
        // `bt_render::OverlayLayer::opacity` is a layer's and not a fill's, made
        // once more one level up — and the reason this is the wrapper's line and
        // not `file_peek_card_layers`', which returns from five places.
        //
        // Multiplied into whatever each layer already carries, never assigned
        // over it: a layer that is faded for a reason of its own is faded for
        // that reason *and* for this one.
        let opacity = self.file_peek_opacity(now).expect(
            "a card with layers is a card on screen, and PeekClock::Shown carries its epoch",
        );
        self.window.file_peek_drawn_opacity = Some(opacity);
        for layer in &mut layers {
            layer.opacity *= opacity;
        }
        layers
    }

    fn file_peek_card_layers(&mut self) -> Vec<marks::OverlayLayer> {
        let Some(subject) = self.file_peek_subject() else {
            return Vec::new();
        };
        // The foot's address, taken before the subject's fields are handed to the
        // card's content below — the file's folder, a string operation (user
        // ruling 2026-09-20; `file_peek::peek_foot_address`).
        let address = subject.foot_address_on(
            bt_platform::host_platform(),
            profiles::home_directory(&bt_pty::SystemShellEnvironment).as_deref(),
        );
        let scale = self.window.renderer.scale_factor() as f32;
        // How tall the document really is, kept whole: the card's body is the
        // *capped* height, and the scroll bar's arithmetic needs the uncapped one
        // to know what share of it is showing.
        let mut document = 0.0_f32;
        // **Has the file gone since the reference was validated, and how large
        // is it?** (§7.37, and §7.29 ⑬ for the second half.) One stat, and only
        // for a real local file — a network path is refused before it ever
        // reaches here, and stat-ing one every frame would block the loop on a
        // share. A card is up only while the pointer rests on it, so this is one
        // existence check per frame of one hover, which is the same budget §7.29
        // spends on `is_dir()` per pointer move.
        //
        // **And the size is read off that same call** (user ruling 2026-08-29).
        // `Path::exists` *is* a `metadata` whose answer is thrown away, so the
        // card's own line about the file costs nothing it was not already
        // spending — and the size is there on the very first frame the card is
        // drawn, rather than a worker hop later under a resting pointer. It is
        // one author for one number: the worker's page read stopped carrying a
        // byte count on the same day ([`preview::PreviewWant::PageCount`]).
        //
        // **And for a name a program printed, neither half is asked here at all** (audit 3 C-2).
        // Route D of the 2026-09-08 audit put a locality predicate in front of the stat below and
        // called both halves local calls; they are local for a link on a local disk, and they are
        // a network round trip for a mapped drive or for an intermediate junction into a share —
        // once per frame, on the thread that paints, over a name a child process chose. So a
        // terminal reference reads its four facts out of its own pane's ledger, where a worker
        // put them, and the card costs the window thread a map lookup. A files column row, a Git
        // row and a composed document keep the stat, in a function of its own: the user pointed
        // at those, which is `DESIGN.md:189`'s division, and their budget is unchanged.
        let (gone, bytes) = match (subject.path.as_deref(), subject.printed_in) {
            (None, _) => (false, None),
            (Some(path), Some(seat)) => match self.seat_path_verdict(seat, path) {
                Some(verdict) => (!verdict.exists, verdict.bytes),
                // A reference with no verdict is not a link and raises no card, so this is
                // unreachable in practice; `false` is the answer that claims nothing.
                None => (false, None),
            },
            (Some(path), None) => facts_of_a_file_the_user_chose(path),
        };
        let body_kind = match peek_body_kind(
            subject.ftype,
            subject.path.as_deref(),
            subject.refused,
            gone,
        ) {
            // A picture is a picture of a *file*; nothing composed is one.
            PeekBodyKind::Picture => {
                let path = subject
                    .path
                    .clone()
                    .expect("a picture body is only chosen for a file");
                self.file_peek_picture(&path, scale)
            }
            PeekBodyKind::Refused => file_peek::PeekBody::Refused,
            // The file has gone: one line saying so, and no body read is
            // asked for — there is nothing on disk to read (§7.37).
            PeekBodyKind::Gone => file_peek::PeekBody::Gone,
            PeekBodyKind::Page => file_peek::PeekBody::Page,
            // The facts of a page this card cannot render. Like a picture's,
            // they are a *file's* and are asked for by the frame that needs
            // them — the read is one worker hop and the box it lands in is
            // reserved from the first frame either way.
            PeekBodyKind::Facts => {
                let path = subject
                    .path
                    .clone()
                    .expect("a facts body is only chosen for a file");
                self.file_peek_facts(&path, scale)
            }
            // One frame of a video and the two lines under it. Like a
            // picture's, they are a *file's* — see [`Self::file_peek_frame`].
            PeekBodyKind::Frame => {
                let path = subject
                    .path
                    .clone()
                    .expect("a frame body is only chosen for a file");
                self.file_peek_frame(&path, scale)
            }
            PeekBodyKind::Document => {
                // **Laid out against the room the card will actually give it**:
                // a document card states its size on the strip like every other
                // card (§7.29 ⑬), so the strip's line comes off the cap here as
                // well — measured with the same predicate the content below is
                // built with, because a document measured in a taller box than
                // the card has is a document cut by a card that thought it fit.
                let probe = [
                    0.0,
                    0.0,
                    file_peek::body_width(scale),
                    file_peek::body_max_height(scale, bytes.is_some()),
                ];
                self.rebuild_preview_document(PreviewSurface::Peek, probe, scale);
                document = self.preview_surface_document_height(PreviewSurface::Peek, probe, scale);
                file_peek::PeekBody::Document(document.min(probe[3]))
            }
        };
        // **What this kind of file has to say about itself**, which is the half
        // of the card's one line that is not the size (user ruling 2026-08-29;
        // §7.29 ⑬). Each arm is the spelling that kind already had — nothing is
        // invented here and nothing new is read from the disk for it:
        //
        // * a **picture** says its own pixels, off the native decode the card is
        //   already drawing a resample of;
        // * a **page** says how many pages the worker counted;
        // * a **recording** says how long it runs and how large its picture is,
        //   through `preview::video_fact_lines` — the first of its two lines,
        //   which is the half that is about the recording, while the half about
        //   the file is the size every card states;
        // * everything else says nothing of its own and prints its size alone.
        //   A **document** would say how many lines it holds if this window knew
        //   — it does not: a buffer holds at most the first 64 KB of a file
        //   (`PREVIEW_HEAD_BYTES`), so a count off it would be a number about
        //   the head rather than about the file, and reading the whole file to
        //   learn it is a disk read this ruling explicitly did not ask for.
        let stated = match &body_kind {
            file_peek::PeekBody::Image { .. } => subject
                .path
                .as_deref()
                .and_then(|path| self.file_peek_native_pixels(path))
                .map(|(width, height)| preview::format_pixel_size(width, height)),
            file_peek::PeekBody::Facts { pages, .. } => {
                pages.map(|pages| i18n::peek_page_count(pages as usize))
            }
            file_peek::PeekBody::Frame { .. } => {
                let path = subject
                    .path
                    .clone()
                    .expect("a frame body is only chosen for a file");
                let extension = path.extension().and_then(std::ffi::OsStr::to_str);
                let [recording, _] =
                    preview::video_fact_lines(extension, self.video_facts_of(&path));
                recording
            }
            file_peek::PeekBody::Document(_)
            | file_peek::PeekBody::Refused
            | file_peek::PeekBody::Page
            | file_peek::PeekBody::Gone => None,
        };
        let content = file_peek::PeekContent {
            // **The chip says what the *file* is, and never which lane draws
            // it** (user ruling 2026-08-29; §7.29 ⑬). `PreviewFtype::Web` is a
            // lane three spellings share, so a `.pdf` card wore `web` in its
            // corner until `preview::type_label` asked the name inside the
            // class.
            //
            // A gone file has no type to name, and the card draws no chip for an
            // empty one (§7.37) — every other body keeps its own word.
            ftype: if matches!(body_kind, file_peek::PeekBody::Gone) {
                String::new()
            } else {
                preview::type_label(&subject.name, subject.ftype).to_owned()
            },
            name: subject.name,
            dirty: subject.dirty,
            // A card over a file that has gone has no size and states nothing —
            // the strip is not reserved for it at all ([`file_peek::layout`]).
            meta: file_peek::meta_line(stated, bytes),
            body: body_kind,
        };
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        // Only the font knows how wide a line is, so the measuring happens here,
        // beside the renderer, exactly as the tip's and the ghost's do — and in
        // the file-name head's shared face (§7.37), the same weight and tracking
        // the name is drawn in, so the box the name is clamped into and the dot
        // hung after it are measured against the letters that actually land.
        let name_width = self.window.renderer.measure_chrome_label(
            &mut self.app.gpu,
            &content.name,
            file_peek::PEEK_HEAD_FONT_LOGICAL_PX * scale,
            bt_render::HEAD_TITLE_WEIGHT,
            bt_render::HEAD_TITLE_TRACKING_EM,
            false,
        );
        let ftype_width = self.window.renderer.measure_chrome_text(
            &mut self.app.gpu,
            &content.ftype,
            file_peek::PEEK_TYPE_FONT_LOGICAL_PX * scale,
        );
        let layout = file_peek::layout(
            &content,
            subject.anchor,
            (width as f32, height as f32),
            name_width,
            ftype_width,
            scale,
        );
        // The file's own image off the resample lane, for the one body that wears one. A page
        // card draws a *column* instead and takes it separately — they were one argument while
        // both bodies drew exactly one picture, and the column ended that.
        //
        // **And no still at all while the picture in this box is moving** —
        // a recording playing on the card, or an animation running on it
        // (§7.44 ⑤, and §7.44 ① for the recording half). The frames are drawn
        // over this layer's ground by the video lane; a still handed in here
        // would land in the card's *icon* channel, which runs after that lane,
        // and would stand over the picture it is one frame of. The box is
        // unaffected — `layout` was solved a moment ago from the file's own size
        // — so the card is the same shape either way.
        //
        // **Both halves, and they are one question.** This clause asked only
        // about an animation until 2026-08-28, and the machine showed what the
        // other half costs: the card's own ▶ started the engine, the bar counted
        // 0:01 → 0:06 off it, and the picture underneath never moved — a still of
        // the frame at one tenth, painted over a recording that was playing
        // perfectly well beneath it. Everywhere else in this window the pair is
        // asked together (`refit_preview_picture`), because "is the thing in this
        // box moving" is one question with two ways of being true.
        let moving = self.surface_is_playing_a_video(PreviewSurface::Peek)
            || self.animation_running_on(PreviewSurface::Peek);
        let picture = match layout.body_kind {
            file_peek::PeekBody::Facts { .. } => None,
            _ if moving => None,
            _ => self
                .window
                .peek_picture
                .as_ref()
                .map(|picture| file_peek::PeekPicture {
                    key: &picture.key,
                    rgba: &picture.rgba,
                    width_px: picture.width_px,
                    height_px: picture.height_px,
                }),
        };
        // **The pages in view that have come home** — asked of the same range the request lane
        // asks (`file_peek::peek_pages_in_view`), so that "drawn" and "asked for" cannot drift
        // apart into a slot that stays blank because nobody ever wanted it. Borrowed, never
        // cloned: each page is hundreds of kilobytes and this runs on every frame the card is up.
        let pages: Vec<file_peek::PeekPage<'_>> = match layout.body_kind {
            file_peek::PeekBody::Facts { pages, scroll, .. } => self
                .window
                .peek_page
                .as_ref()
                .map(|slot| {
                    file_peek::peek_pages_in_view(pages.unwrap_or(1), scroll, scale)
                        .filter_map(|index| {
                            slot.page(index).map(|raster| file_peek::PeekPage {
                                index,
                                picture: file_peek::PeekPicture {
                                    key: &raster.key,
                                    rgba: &raster.rgba,
                                    width_px: raster.width_px,
                                    height_px: raster.height_px,
                                },
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        // The foot's two halves, measured here for the reason the name and the
        // chip were: only something holding a font can say how wide a line is.
        // The card is never saved into, and a press on its foot takes it down,
        // so it has no flash — its left hand is the file's folder, always (user
        // ruling 2026-09-20): the path's parent, spelled as the files column's
        // foot spells a folder, and never a question put to a disk.
        let notice = self
            .preview_standing_fact(PreviewSurface::Peek, Instant::now())
            .unwrap_or_default()
            .to_owned();
        let foot = {
            let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
            let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
            seats::dress_foot(
                seats::FootDress {
                    dissolved: 0.0,
                    run: file_peek::foot_run(&layout, scale),
                    lead: &address,
                    flash: None,
                    notice: &notice,
                    // An address is read from both ends — where it stands and
                    // which folder it names — so it gives way in the middle.
                    cut: seats::LeadCut::Middle,
                    font_px: file_peek::PEEK_FOOT_FONT_LOGICAL_PX * scale,
                    gap_px: seats::FILES_FOOT_NOTICE_GAP_LOGICAL_PX * scale,
                },
                &mut measure,
            )
        };
        let palette = bt_render::chrome_palette();
        let pointer = self
            .window
            .pointer_position
            .map(|at| [at.x as f32, at.y as f32]);
        let mut layer = file_peek::build(
            &layout, &content, &foot, pointer, picture, &pages, &palette, scale,
        );
        // **Where the card came to rest, filed before anything is drawn into
        // it** — every pointer question about the card reads these two, so a
        // frame that painted a card without recording it would be a card on
        // screen that nothing could touch. How tall the document turned out is
        // no longer filed with them: the hit test asks the card's own pane, the
        // way it asks a seat's.
        if let Some(peek) = self.window.file_peek.as_mut() {
            peek.frame = Some(layout.frame);
            peek.body = Some(layout.body);
            peek.head = Some(layout.head);
            peek.foot = file_peek::foot_address_box(&layout, &foot);
            // And whether this frame lit it — the receipt a pointer move reads
            // to know that the hand crossed the address's edge and a frame is
            // owed ([`Self::relight_file_peek_foot`]).
            peek.foot_lit = file_peek::over_foot(peek.foot, pointer);
            // And the column's reach, or the fact that this card has none — written
            // unconditionally, because a card that changed body without clearing it would
            // answer a wheel with the last document's number.
            peek.column = match content.body {
                file_peek::PeekBody::Facts { pages, .. } => Some(
                    file_peek::peek_page_column_max_scroll(pages.unwrap_or(1), scale),
                ),
                _ => None,
            };
        }
        // **A column of pages wears the same bar a document does**, over the window it is wound
        // past rather than over the whole body: the two fact lines underneath do not move, and a
        // rule beside them would be a bar claiming to measure them (user ruling 2026-08-26).
        if let file_peek::PeekBody::Facts { pages, scroll, .. } = content.body {
            let ground = file_peek::page_ground(layout.body, scale);
            let Some(bar) = preview_body_bar(
                ground,
                preview::ScrollAxis::Vertical,
                [0.0, scroll],
                file_peek::peek_page_column_height(pages.unwrap_or(1), scale),
                scale,
            ) else {
                return vec![layer];
            };
            let state = ScrollThumbState::of(
                self.window
                    .file_peek
                    .as_ref()
                    .is_some_and(|peek| peek.thumb_grab.is_some()),
                self.window
                    .pointer_position
                    .is_some_and(|at| file_peek::contains(bar.grab, [at.x as f32, at.y as f32])),
            );
            return vec![layer, scroll_bar_layer(&bar, state, &palette)];
        }
        if !matches!(content.body, file_peek::PeekBody::Document(_)) {
            return vec![layer];
        }
        // The scroll, clamped **on the way in** rather than on the way out (R2's
        // ruling, second reading): a document that shrank under a card already
        // scrolled — a head read landing, a theme changing the type — leaves an
        // offset past its own end, and clamping here is the one place that sees
        // both the old offset and the new extent. Nothing downstream has to
        // wonder whether what it was handed is reachable.
        let scroll = self.clamped_preview_scroll(
            PreviewSurface::Peek,
            layout.body,
            scale,
            self.window.peek_pane.scroll,
        );
        self.window.peek_pane.scroll = scroll;
        // The same channel a preview float's document rides, for the same
        // reason: the body has to be drawn *above* the card's own face, and
        // the seats' document lane is a whole pass below the overlays.
        layer.body = self.build_preview_body_in(PreviewSurface::Peek, layout.body);
        // **The card wears one bar and it is the vertical one.** A glance is a
        // three-hundred-pixel mirror of a pane, and a second rule along its
        // bottom would be furniture arguing with the thing it is a glance at.
        let Some(bar) = preview_body_bar(
            layout.body,
            preview::ScrollAxis::Vertical,
            scroll,
            document,
            scale,
        ) else {
            return vec![layer];
        };
        let state = ScrollThumbState::of(
            self.window
                .file_peek
                .as_ref()
                .is_some_and(|peek| peek.thumb_grab.is_some()),
            self.window
                .pointer_position
                .is_some_and(|at| file_peek::contains(bar.grab, [at.x as f32, at.y as f32])),
        );
        vec![layer, scroll_bar_layer(&bar, state, &palette)]
    }

    /// **A press inside the card**, and what it means (user ruling, 2026-08-14).
    ///
    /// None of its answers edits the document, which is what keeps "read-only"
    /// true of a card the pointer can now reach:
    ///
    /// * on the **scroll thumb**, take hold of it — asked first for the reason
    ///   [`Self::press_preview_block_thumb`] is asked before the body it rides
    ///   over: a bar drawn inside a region is still a bar, and a press there
    ///   means the bar, exactly as it does in every text editor on the desk;
    /// * on the **foot's folder address**, find the file where it lives — in the
    ///   files column, or under the hand-over modifier in Explorer or Finder
    ///   ([`Self::press_file_peek_foot`], user ruling 2026-09-20);
    /// * **anywhere else in the card, open the real preview pane** — the same
    ///   door Enter and the double-click take — and take the card down, because
    ///   what it was standing in for is now on screen.
    ///
    /// There is deliberately no caret and no selection: a press in a document is
    /// a place to type in a *pane*, and the card has nothing to type into. That
    /// is now the whole of what "read-only" names.
    ///
    /// Only the left button; a right press falls through to be whatever it
    /// already was, and the generic dismissal below takes the card down with it.
    pub(in crate::runtime) fn press_file_peek(&mut self, button: MouseButton) -> Result<bool> {
        if button != MouseButton::Left {
            return Ok(false);
        }
        let Some(position) = self.window.pointer_position else {
            return Ok(false);
        };
        let at = [position.x as f32, position.y as f32];
        if !self.file_peek_holds(at) {
            return Ok(false);
        }
        let frame = self
            .window
            .file_peek
            .as_ref()
            .and_then(|peek| peek.frame)
            .unwrap_or_default();
        let head = self
            .window
            .file_peek
            .as_ref()
            .and_then(|peek| peek.head)
            .unwrap_or_default();
        let foot = self.window.file_peek.as_ref().and_then(|peek| peek.foot);
        match file_peek::press_at(frame, head, foot, self.file_peek_bar().as_ref(), at) {
            file_peek::Press::Elsewhere => Ok(false),
            file_peek::Press::Foot => self.press_file_peek_foot(),
            // **The card's head is a handle, and this press is not yet anything**
            // (user ruling 2026-08-27, §7.29). Six pixels of travel make it a
            // carry ([`Self::promote_file_peek_press`]); a release without them
            // makes it the door, which is what the rest of the face is and what
            // the head was until today. Consuming the press is what buys the
            // choice: an answer given on the way down cannot be taken back on
            // the way up.
            file_peek::Press::Head if self.file_peek_promotes() => {
                self.window.file_peek_press = Some(FilePeekPress {
                    latch: DragLatch::new(position),
                });
                Ok(true)
            }
            // **A card with no window to become keeps the head it always had**
            // — see [`Self::file_peek_promotes`]. Nothing is armed, nothing is
            // held back, and the press means on the way *down* exactly what a
            // press on the rest of the face means, which is what it meant here
            // yesterday.
            file_peek::Press::Head => self.press_file_peek_door(),
            file_peek::Press::Thumb(grab) => {
                if let Some(peek) = self.window.file_peek.as_mut() {
                    peek.thumb_grab = Some(grab);
                }
                // The thumb's ink changes the moment it is taken, so this owes a
                // frame even though nothing has moved yet.
                if self.refresh_overlay() {
                    self.present_chrome_change()?;
                }
                Ok(true)
            }
            file_peek::Press::Open => {
                // **A player standing on the picture answers before the door
                // underneath it** (route B slice ②; §7.44 ①, found by
                // photographing the machine 2026-08-28 — the second host to
                // have this same defect, and for the same reason).
                //
                // `press_video_at` states the whole order — a play mark on a
                // picture, then every control on a bar that is up, then a double
                // click on the picture — and it is reached from
                // `chrome_mouse_input`. A press inside the card never gets that
                // far: `press_file_peek` is asked *above* the chrome router,
                // which is what makes a card opaque to the layout beneath it,
                // and this arm went straight to the door. So the card drew a
                // play disc that lit under the pointer and, when it was pressed,
                // opened the preview pane instead of playing — the ruling's
                // *「能动的就动」* reaching a glance card and being defeated by a
                // route, one host over from the float in §7.44 ①.
                //
                // Here rather than at the top of the function, and that is the
                // same placement `press_float` gives it: the card's *furniture*
                // — the head that is a handle, the scroll thumb — is still the
                // card's, and only on the face does the player out-rank what is
                // under it.
                if self.press_video_at(position)? {
                    return Ok(true);
                }
                // **A wide block in the card is a scrolling region, and the bar
                // under it is a bar** (user ruling, 2026-08-14). Asked between
                // the card's own thumb and the door, which is the order the
                // three are drawn in: the card's bar rides over everything, a
                // block's bar rides over the block, and the door is the whole
                // face underneath both. It is the same order a docked pane takes
                // its presses in — see the `press_preview_body_thumb` /
                // `press_preview_block_thumb` pair.
                if self.press_preview_block_thumb(position)? {
                    return Ok(true);
                }
                self.press_file_peek_door()
            }
        }
    }

    /// **A press on the card's foot: the file, found where it lives** (user
    /// ruling 2026-09-20).
    ///
    /// The foot names the folder that holds the file, and a press on it answers
    /// that address with one of the two verbs this window already has for a
    /// place: a plain click **locates** — the files column stands on the folder,
    /// keeping its root when the folder is inside it and re-rooting when it is
    /// not, and selects the file's row
    /// ([`Self::locate_folder_in_files_column`]) — and the hand-over modifier
    /// **reveals** the file, selected, in Explorer or Finder, through the door
    /// the card's own surface uses for its `Ctrl`+click ([`peek_foot_press`]).
    ///
    /// The card comes down first, as it does for the pane's door: the column is
    /// about to move under it, and a card left standing would be placed against
    /// a row that has just gone. **The column stays where the press took it** —
    /// nothing here remembers the root it replaced, and taking the card down
    /// restores nothing (owner, 2026-09-23: "it is navigation, not a peek").
    ///
    /// No disk is asked on the way: the folder is the path's parent, and a
    /// printed reference is handed over off its pane's ledger.
    fn press_file_peek_foot(&mut self) -> Result<bool> {
        let Some((host, path)) = self.window.file_peek.as_ref().and_then(|peek| {
            peek.source
                .file_path()
                .map(|path| (peek.host, path.to_path_buf()))
        }) else {
            return Ok(false);
        };
        let hand_over = input::pointer_chord_held(self.window.modifiers_held);
        let Some(press) = peek_foot_press(host, &path, hand_over) else {
            return Ok(false);
        };
        self.hide_file_peek();
        match press {
            PeekFootPress::Locate { folder, file } => {
                self.locate_folder_in_files_column(&folder, Some(&file))?;
            }
            PeekFootPress::Reveal(path) => {
                self.reveal_in_explorer(&path);
            }
            PeekFootPress::RevealVerified(seat, path) => {
                let facts = self.verified_target(seat, &path);
                self.reveal_verified(&path, facts);
            }
        }
        self.apply_pointer_cursor();
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// **Whether the pointer is on the card's folder address** — the hand the
    /// terminal's links wear, because the address is one: it goes somewhere
    /// when pressed (user ruling 2026-09-20).
    pub(in crate::runtime) fn file_peek_foot_grasp(&self) -> bool {
        let Some(at) = self.window.pointer_position else {
            return false;
        };
        self.window
            .file_peek
            .as_ref()
            .filter(|peek| peek.clock.is_shown())
            .is_some_and(|peek| file_peek::over_foot(peek.foot, Some([at.x as f32, at.y as f32])))
    }

    /// **The card is the door to the pane** — the answer the whole face gave
    /// before the head became a handle, and still gives everywhere else.
    ///
    /// Its own function because two arms reach it now: the face, and a head that
    /// has nothing to promote to ([`Self::file_peek_promotes`]). A second copy
    /// would be a second opinion about where a card leads.
    fn press_file_peek_door(&mut self) -> Result<bool> {
        let Some((host, source, name)) = self
            .window
            .file_peek
            .as_ref()
            .map(|peek| (peek.host, peek.source.clone(), peek.name.clone()))
        else {
            return Ok(false);
        };
        // Down before the pane opens rather than after: opening re-solves the
        // layout, and a card left standing would be placed against a row that
        // has just moved under it.
        self.hide_file_peek();
        // **The door leads where the row leads.** A glance over a file opens the
        // file; a glance over a commit's file opens that commit's reading of it,
        // through the very door the row itself uses — so pressing the card and
        // pressing the row behind it arrive at one document and not at two.
        match source.file_path().map(Path::to_path_buf) {
            Some(path) => self.open_preview(path)?,
            None => {
                if let RowHost::Git(seat) = host {
                    self.open_git_document(seat, source, name, None)?;
                }
            }
        }
        Ok(true)
    }

    /// **Whether this card has a window to become** — the one predicate the
    /// head's cursor and the head's press both read (user ruling 2026-08-27,
    /// §7.29; page arm opened 2026-08-28, §7.39).
    ///
    /// One refusal, and it is a fact about what a *preview float* can hold
    /// rather than a hedge: **a composed document is on no disk.** A glance over
    /// a commit's reading of a file has no path, and the door a promotion goes
    /// through ([`Self::open_preview_onto`]) takes one. The repository's own
    /// answer for a window over a composed document is `open_float_git_document`,
    /// a different verb reached from a different place.
    ///
    /// **A page is no longer refused.** It used to be — `.html` and `.pdf` go
    /// down the engine's lane, and the note here read *"a page needs a seat, and
    /// a float is not one"*. That was the whole of it, and §7.29 ⑥′ said what
    /// would end it: *"when a float can be given an engine of its own, this arm
    /// goes"*. It can now — [`Self::open_minted_page_on_float`] mints the
    /// float a detached leaf and opens the engine on it, which is the very
    /// engine `pop_out_preview` hands a float when it carries a page out of a
    /// pane (§7.14b). So a `.pdf` card torn out is a window with a page on it,
    /// not an empty one, and the arm is gone.
    ///
    /// A file path is all that is asked, then: both a document and a page have
    /// one, and a composed glance has none. The class is not re-spelled here by
    /// extension — the page lane's own fork ([`source_opens_as_a_page`]) is
    /// where `.html` and `.pdf` are written down (§7.16 ④), and it is asked on
    /// the way through, not duplicated on the way in.
    fn file_peek_promotes(&self) -> bool {
        self.window
            .file_peek
            .as_ref()
            .and_then(|peek| peek.source.file_path())
            .is_some()
    }

    /// The pointer travelling with the card's thumb in hand.
    ///
    /// The bar is re-read every move rather than remembered, which is
    /// [`Self::drag_preview_block_thumb`]'s rule and every scrollbar's: a drag
    /// that trusted a stored rectangle would be dragging where the thumb *was*.
    /// Answers whether the pointer was the thumb's, so the caller stops.
    pub(in crate::runtime) fn drag_file_peek_thumb(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let Some(grab) = self
            .window
            .file_peek
            .as_ref()
            .and_then(|peek| peek.thumb_grab)
        else {
            return Ok(false);
        };
        let Some(bar) = self.file_peek_bar() else {
            return Ok(true);
        };
        // `bar.along` rather than `position.y`: the axis is the bar's own fact,
        // and a caller that reached for a component itself would be the second
        // place that has to be right about which way this one runs.
        let along = bar.along([position.x as f32, position.y as f32]);
        let wanted = preview::scroll_dragged_to(&bar, along, grab);
        if (wanted - self.window.peek_pane.scroll[1]).abs() < f32::EPSILON {
            return Ok(true);
        }
        self.window.peek_pane.scroll[1] = wanted;
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// **Six pixels, and the glance you were given is a window you keep** (user
    /// ruling 2026-08-27, §7.29).
    ///
    /// The folder card has answered a head carried six pixels since 2026-08-12
    /// (`FloatHost::promote`), and the ruling is that every card answers it: a
    /// glance is a thing you were shown, and reaching for it with the whole hand
    /// rather than the eye is the one gesture that says *keep this*. What the
    /// two cards promote **to** differs because what they are differs — a folder
    /// card is already a window in the peek slot and simply moves to the pinned
    /// list, while a glance card is not a window at all and one has to be opened
    /// for it — but the gesture, the threshold and the sentence are one.
    ///
    /// Answers whether the pointer has been spent, so the move that promoted
    /// does not go on to be a hover as well.
    pub(in crate::runtime) fn promote_file_peek_press(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let scale = self.window.renderer.scale_factor();
        let Some(press) = self.window.file_peek_press.as_mut() else {
            return Ok(false);
        };
        if !press.latch.travelled(position, scale) {
            // Still only a press: the hand has not said which gesture this is,
            // and the card is being held rather than carried. It still owns the
            // pointer, because a press that let go of it here would let the
            // hover underneath re-arm a card that is about to be consumed.
            return Ok(true);
        }
        self.window.file_peek_press = None;
        self.promote_file_peek(position)?;
        Ok(true)
    }

    /// The card, become a preview window — the promotion itself.
    ///
    /// **It is `pop_out_preview` with no pane to leave behind**, and every
    /// sentence that function's own note makes holds here with one substitution:
    /// there is no seat to close, no stand-in shell to spawn and no tree to
    /// refuse to empty, because a card was never in the layout. What is left is
    /// the half that matters — a pinned window on the preview chassis, wearing
    /// the address rail its tenant asks for, appended to the pinned list where
    /// windows on one root have been allowed to sit side by side since the
    /// 同根去重 repeal of 2026-08-12.
    ///
    /// **The file is opened rather than carried.** A pop-out moves a *view*
    /// because the view it moves was built at the pane's own width; a card's was
    /// built at three hundred pixels, and a scroll offset measured in a column
    /// that narrow means nothing in a window four times wider — a reflowed
    /// document would land somewhere else and a page column somewhere else
    /// again. So the window opens the file through
    /// [`Self::open_preview_onto`], which is the same door a file dropped on a
    /// preview goes through, and every lane a card can show comes with it: a
    /// video's face, a picture, a page on the engine, a document in the pool.
    /// **A video does not begin to play** — nothing in that door starts one, and
    /// this ruling did not ask for one.
    ///
    /// **It opens where the card stood.** Not cascaded, deliberately: the
    /// cascade exists so that a window landing unattended on top of another is
    /// visibly a second window, and this one is landing in a hand — the hand
    /// that is about to carry it somewhere. The grab is measured here rather
    /// than at the press for [`FilePeekPress`]'s stated reason.
    fn promote_file_peek(&mut self, position: PhysicalPosition<f64>) -> Result<()> {
        let Some(peek) = self.window.file_peek.as_ref() else {
            return Ok(());
        };
        let (Some(frame), Some(path)) = (
            peek.frame.filter(|_| peek.clock.is_shown()),
            peek.source.file_path().map(Path::to_path_buf),
        ) else {
            return Ok(());
        };
        let body_height = peek.body.map_or(0.0, |body| body[3] - body[1]);
        let scale = self.window.renderer.scale_factor() as f32;
        let viewport = self.float_viewport();
        // The window's own opening size, from the same door a pop-out asks:
        // min(64vh, 520) around whatever body it is given. The card's body is
        // what it is given, so a two-line file promotes to a small window and a
        // long one to a tall one — the card's own shrink-wrap, kept.
        let size = float::float_opening_size(
            float::float_height_for_body(body_height, scale),
            viewport,
            scale,
            float::FloatSizing::preview(),
        );
        let (placed, grab) = file_peek_promotion(
            frame,
            size,
            [position.x as f32, position.y as f32],
            viewport,
            scale,
        );
        let tab = self.id;
        // **Whatever is playing on the card is coming with it** (user ruling
        // 2026-08-28: *「拖头转浮窗时把引擎带走(不重开,位置不丢)」*; §7.44 ③).
        //
        // Read before `hide_file_peek`, which shuts down the card's seat along
        // with everything else the card owns, and re-homed after the float
        // exists — so the sequence is: remember, tear the card down, open the
        // window, hand the engine over. What is *not* here is a second `open`:
        // the decoder is not restarted, the playhead does not go back to zero
        // and the texture keeps its name, so not one frame is decoded or
        // uploaded twice. That is the whole difference between carrying a video
        // and re-opening one, and it is the difference a reader sees.
        let carried = self.window.video.get(PreviewSurface::Peek).is_some();
        if carried {
            self.window.video_carried_off_the_card = true;
        }
        // The card goes **before** the window opens, and not after: an opening
        // re-solves and repaints, and a card left standing through that would be
        // drawn once more against a row it no longer belongs to.
        self.hide_file_peek();
        let id = self.window.float.open(
            float::FloatMode::Pinned,
            // Torn off rather than summoned: there is no trigger to re-click and
            // nothing to re-place it against — `pop_out_preview`'s own two
            // `None`s, for its own reason.
            None,
            float::FloatTenant::Preview(float::FloatPreview { tab, page: None }),
            placed,
            None,
            Instant::now(),
        );
        if carried {
            self.window.video_carried_off_the_card = false;
            self.window
                .video
                .rehome(PreviewSurface::Peek, PreviewSurface::Float(id));
        }
        self.open_preview_onto(PreviewSurface::Float(id), path)?;
        self.window.float_drag = Some(FloatDrag {
            win: id,
            kind: FloatDragKind::Move { grab },
        });
        self.forget_dead_float_gestures();
        self.apply_pointer_cursor();
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// Letting go of the card's head without having carried it — the other half
    /// of what that press could have meant (user ruling 2026-08-27, §7.29).
    ///
    /// The door, which is what the whole face of the card was before the head
    /// became a handle and what the rest of it still is: pressing a glance opens
    /// the file it is glancing at. Spending it on the release rather than on the
    /// press is the price of the head being two gestures, and it is the price
    /// every drag-or-click control pays.
    ///
    /// Answers whether the release was the head's, so the caller knows whether
    /// anything else may still have it.
    pub(in crate::runtime) fn release_file_peek_press(&mut self) -> Result<bool> {
        if self.window.file_peek_press.take().is_none() {
            return Ok(false);
        }
        // The same door, and literally the same one: a head that decided it was
        // a press and a face that never had a choice must arrive at one
        // document, or the card would lead two places depending on where in it
        // you happened to put the pointer.
        self.press_file_peek_door()?;
        Ok(true)
    }

    /// Letting go of the thumb.
    ///
    /// It **pays for a frame and consumes nothing**, which is the pair of
    /// answers a release owes here. The frame is owed because the thumb wears
    /// the divider's held ink while it is held and the resting one after — the
    /// same debt [`Self::press_file_peek`] settles on the way down, and left
    /// unpaid it leaves a blue thumb under a hand that has already let go, until
    /// some unrelated event happens to redraw (real-machine capture,
    /// 2026-08-14). Consuming nothing is because a release is never *only* the
    /// thumb's: the same button coming up still has to reach whatever else was
    /// waiting for it.
    pub(in crate::runtime) fn release_file_peek_thumb(&mut self) -> Result<()> {
        let released = self
            .window
            .file_peek
            .as_mut()
            .is_some_and(|peek| peek.thumb_grab.take().is_some());
        if released && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The card's scroll bar as it stands **right now**, for a hand rather than
    /// for the painter.
    ///
    /// Re-derived from the card's own filed geometry instead of stored, which is
    /// the block bar's own hard-won rule: a rectangle remembered from the frame
    /// it was drawn on is a rectangle the pointer is tested against after the
    /// thing moved.
    fn file_peek_bar(&self) -> Option<preview::ScrollBar> {
        let peek = self.window.file_peek.as_ref()?;
        let body = peek.body.filter(|_| peek.clock.is_shown())?;
        // The same door the seat's bar and the float's go through, asked with
        // the one thing the card has to hand in: the box its own layout
        // produced. The document's height is read off the card's pane exactly as
        // a pane's is read off its own, so the hand and the painter cannot end
        // up with two answers.
        self.preview_surface_bar(
            PreviewSurface::Peek,
            preview::ScrollAxis::Vertical,
            body,
            self.window.renderer.scale_factor() as f32,
        )
    }

    /// **Whether the pointer is inside the glance one of the peek's own rows
    /// raised** (user ruling 2026-09-07, `docs/DESIGN.md` §7.58).
    ///
    /// The third entry on "reaching for the thing that summoned it is not leaving
    /// it", read the other way round: the trigger and the root menu are things the
    /// peek came *from*, and this is a thing the peek gave *rise to*. Both are the
    /// same sentence — the region the peek is alive in is larger than its own
    /// rectangle — and the ruling states the consequence outright: the folder card
    /// stays open while its child preview is showing. Without this, walking off the
    /// folder card into the card it just opened starts the folder card's 220ms and
    /// takes the preview's anchor down from under it.
    ///
    /// **Only the glance this peek raised**, never any other: a card standing over
    /// a files column, over a Git page or over a terminal reference has nothing to
    /// do with this window, and holding a window open under one would be a peek
    /// that never closes while an unrelated card happens to be up.
    pub(in crate::runtime) fn pointer_is_in_the_peeks_own_glance(
        &self,
        position: PhysicalPosition<f64>,
    ) -> bool {
        let Some(id) = self.window.float.peek_id() else {
            return false;
        };
        self.window
            .file_peek
            .as_ref()
            .is_some_and(|peek| peek.host == RowHost::Float(id))
            && self.file_peek_holds([position.x as f32, position.y as f32])
    }

    /// Whether the pointer is on the run of cells that summoned the peek which
    /// is up — the reference's half of "reaching for the thing that summoned it
    /// is not leaving it".
    ///
    /// The *live* rectangle rather than the anchor the window was placed
    /// against, for [`Self::trigger_rect`]'s reason: the run is on a grid that
    /// scrolls, and a hand held still over a reference that has moved under it
    /// is not on it any more.
    pub(in crate::runtime) fn pointer_is_on_the_peeks_reference(
        &self,
        position: PhysicalPosition<f64>,
    ) -> bool {
        self.window
            .float
            .peek()
            .and_then(|win| win.origin)
            .filter(|origin| matches!(origin, float::FloatTrigger::Reference { .. }))
            .and_then(|trigger| self.trigger_rect(trigger))
            .is_some_and(|rect| file_peek::contains(rect, [position.x as f32, position.y as f32]))
    }

    /// Whether the glance card's own ninety milliseconds is still climbing — the
    /// last arm of [`Self::file_peek_deadline`], asked on its own.
    ///
    /// The card's *epoch* and nothing else (review round 3, 2026-09-18). This
    /// used to begin with [`Self::file_peek_owes_frame`], which is the debt the
    /// landing frame is scheduled by rather than a statement that anything is
    /// moving: a card whose fade had finished at ninety milliseconds went on
    /// reporting itself in flight until some turn rebuilt the overlay and
    /// settled the receipt, which under a flood is one carry past the end and
    /// over a surface that could not be laid out is never. The debt is still in
    /// the deadline and is still paid by the advance.
    pub(crate) fn file_peek_is_fading(&self, now: Instant) -> bool {
        self.window
            .file_peek
            .as_ref()
            .and_then(|peek| peek.clock.shown_at())
            .is_some_and(|shown| {
                tooltip::hover_fade_owes_frames(now.duration_since(shown), self.app.motion)
            })
    }

    /// Which tab's folder summoned the peek that is on screen, if any.
    ///
    /// The peek slot alone — `FloatHost::peek` — because this is the only float
    /// whose life is spent by [`Self::advance_float`]'s "the header it hangs
    /// from has gone" rule, and a pinned window is exempt from that rule by
    /// ruling (§7.1.2). A float torn off a docked column carries no trigger at
    /// all, and one summoned from a pane head names a leaf rather than a tab;
    /// both answer `None` here, and both are right to — see
    /// [`tab_trailing_targets`].
    pub(in crate::runtime) fn peeking_tab(&self) -> Option<usize> {
        match self.window.float.peek().and_then(|win| win.origin)? {
            float::FloatTrigger::Tab(id) => self.window.tabs.iter().position(|tab| tab.id == id),
            float::FloatTrigger::Pane(_) | float::FloatTrigger::Reference { .. } => None,
        }
    }

    /// The image a hover at `hit` may preview, from any shape the screen can offer it in.
    ///
    /// The frame's own scan comes first, and it already carries every shape whose file can be named
    /// from what is drawn: a printed path, an OSC 7 relative form, a bare `file://` URI, and the
    /// target of an OSC 8 link whose visible text names no file at all. It is the same list the
    /// underline is painted from, which is what makes "you can peek exactly what is marked" a fact
    /// about one list rather than an agreement between two. Verification is not required here: a
    /// hover is how a picture nobody has opened yet gets opened.
    ///
    /// Last comes the one shape that names no file: an OSC 1337 payload, hovered over the
    /// `[image]` placeholder the adapter wrote for it. It is asked last because it is the only
    /// source that cannot be re-read — where a path and a placeholder somehow shared a cell, the
    /// text the pointer was actually put on is still what wins.
    ///
    /// The complement of inline admission is checked once, here, before any source is consulted —
    /// a link that happens to lie across a banded content point does not smuggle a second
    /// presentation of it.
    /// Every question below is asked of the pane the pointer is in — its frame, its scan, its
    /// shell's admission rule. Asking the focused pane instead is how a hover over the pane you
    /// are not typing in came to be answered by a cell the pointer is nowhere near.
    pub(in crate::runtime) fn peek_target(
        &self,
        hit: bt_render::GridHit,
    ) -> Option<(PeekSubject, SeatId)> {
        let (seat, leaf, _) = self.hovered_leaf()?;
        // **The glance card goes first, and this flyout keeps what it does not
        // take** (user ruling 2026-08-27, §7.29: *一个文件,不论从哪指向它,都是
        // 同一张卡*).
        //
        // Before today a `.png` named in the output was the one file in this
        // window with two different answers to a resting hand — a bare picture
        // here, the card everywhere else — and the ruling is that there is one
        // card. So a reference this window has verified now raises that card,
        // and what is left for this surface is exactly the shape that has no
        // file to raise one over: an OSC 1337 payload, whose bytes arrived in
        // the stream and were never on a disk, and a printed path no worker has
        // opened, which carries no link and therefore no card. Both keep the
        // picture, and both keep it for the same reason — there is nothing else
        // this window could put there.
        //
        // Asked of the cell rather than of the flyout's own state, so the two
        // surfaces cannot both be up over one reference for the length of one
        // clock: the card's own 350ms and this one's 300ms would otherwise race
        // every time, and the picture would win.
        if self
            .reference_cell_index(seat, hit)
            .and_then(|cell| self.pointer_reference_at(seat, cell))
            .is_some()
        {
            return None;
        }
        let anchor = leaf
            .last_presented_frame
            .as_ref()?
            .anchor_at(hit.row, hit.column, Bias::Before)
            .ok()??;
        if !leaf.session.peek_admits_at(&anchor) {
            return None;
        }
        leaf.frame_image_references
            .at(hit)
            .map(|reference| PeekSubject::from_path(reference.path.clone()))
            .or_else(|| {
                leaf.session
                    .inline_image_payload_peek_at(&anchor)
                    .map(PeekSubject::from_content_key)
            })
            .map(|subject| (subject, seat))
    }

    /// Present a pure peek-overlay change. The overlay lives beside the frame, not inside it, so
    /// `redraw` would find nothing queued and skip: when nothing newer is pending, the frame that
    /// is already on screen re-enters the slot; a queued newer frame carries the overlay along on
    /// its own redraw.
    pub(in crate::runtime) fn present_peek_overlay(
        &mut self,
        overlay: Option<PeekImageOverlay>,
    ) -> Result<()> {
        if !self.window.renderer.set_peek_overlay(overlay) {
            return Ok(());
        }
        // Not while a resize present is outstanding: that gate admits only the newly projected
        // grid, and the frame on screen is the previous one. A repaint is already owed to the
        // resize, and it carries the renderer-side overlay state with it.
        if !self.resize_present_owed
            && self.window.pending_frames.pending_frame().is_none()
            && let Some(frame) = self.window.last_presented_frame.clone()
        {
            self.window
                .pending_frames
                .publish(
                    frame,
                    FrameTrigger {
                        occurred_at: Instant::now(),
                        source: FrameSource::Expose,
                    },
                )
                .context("re-present the on-screen frame for a peek overlay change")?;
        }
        hang_watch::during(hang_watch::Station::WindowRedraw, || {
            self.window.window.request_redraw()
        });
        Ok(())
    }

    /// Drop peek hover state and hide the flyout. Idempotent; used by every dismiss gesture
    /// (pointer off the span, pointer left, wheel, click, any key).
    pub(in crate::runtime) fn dismiss_peek(&mut self) -> Result<()> {
        self.window.peek_hover.clear();
        self.present_peek_overlay(None)
    }

    /// The rectangle a peek belonging to `seat` is sized against: that pane's *body*, which is the
    /// same rectangle its grid was drawn into and therefore the same one the 40% cap has always
    /// meant. `None` once the pane is gone.
    fn peek_seat_viewport(&self, seat: SeatId) -> Option<bt_render::SeatViewport> {
        seats::pane_body_viewport(
            &self.seats,
            &self.seat_layout,
            seat,
            self.window.renderer.scale_factor() as f32,
        )
    }

    pub(in crate::runtime) fn activate_peek_if_due(&mut self, now: Instant) -> Result<()> {
        if let Some(candidate) = self.window.peek_hover.activate_if_due(now) {
            self.show_or_request_peek(&candidate)?;
        }
        Ok(())
    }

    /// Resolve the flyout for a settled hover: decode the subject if it is new, resample the decode
    /// into the box this viewport will draw it in if that raster is not the one already held, and
    /// present when display-sized pixels are in hand. Each miss is one worker round trip and the
    /// completion re-enters here, so the event thread neither decodes nor resamples.
    pub(in crate::runtime) fn show_or_request_peek(
        &mut self,
        candidate: &PeekCandidate,
    ) -> Result<()> {
        let cache_key = candidate.subject.key.clone();
        let (content_key, native_rgba, native_width_px, native_height_px) =
            match self.window.peek_cache.get(&cache_key) {
                Some(PeekCacheEntry::Ready {
                    key,
                    rgba,
                    width_px,
                    height_px,
                    ..
                }) => (key.clone(), Arc::clone(rgba), *width_px, *height_px),
                // A failed decode stays silent: the terminal text is the honest surface, and the
                // negative entry keeps hovers from re-hitting the disk.
                Some(PeekCacheEntry::Pending) | Some(PeekCacheEntry::Failed(_)) => return Ok(()),
                None => {
                    // Nothing to read: a stream payload is cached when its decode lands or never,
                    // and the session only names one whose decode already succeeded, so a miss
                    // here is a hover that arrived first. The next one finds it.
                    let Some(path) = candidate.subject.path.clone() else {
                        return Ok(());
                    };
                    if !self.app.math_worker_running {
                        return Ok(());
                    }
                    if self
                        .app
                        .math_worker
                        .tasks
                        .send(MathWorkerRequest::PeekImage {
                            leaf: self.focused_shell_address(),
                            path,
                        })
                        .is_ok()
                    {
                        self.window
                            .peek_cache
                            .insert(cache_key, PeekCacheEntry::Pending);
                    }
                    return Ok(());
                }
            };
        // The box is sized against the pane that owns the picture — the pane the pointer was in,
        // never the pane holding the keyboard. A pane that has gone, or is too small to host the
        // flyout, shows none, and nothing is resampled for it.
        let Some(seat) = self.peek_seat_viewport(candidate.seat) else {
            return Ok(());
        };
        let Some((display_width_px, display_height_px)) = self
            .window
            .renderer
            .peek_thumbnail_extent(seat, native_width_px, native_height_px)
        else {
            return Ok(());
        };
        let target: PeekThumbnailTarget = (content_key, display_width_px, display_height_px);
        if let Some(thumbnail) = self.window.peek_thumbnail.as_ref()
            && thumbnail.matches(&target)
        {
            let overlay = thumbnail.overlay(seat, candidate.pointer);
            return self.present_peek_overlay(Some(overlay));
        }
        if self.window.peek_thumbnail_pending.as_ref() == Some(&target)
            || !self.app.math_worker_running
        {
            return Ok(());
        }
        if self
            .app
            .math_worker
            .scale_tasks
            .send(ScaleWorkerRequest::Peek {
                leaf: ShellAddress {
                    window: self.window.window.id(),
                    leaf: LeafId {
                        tab: self.id,
                        seat: candidate.seat,
                    },
                },
                task: peek_scale_task(&target, native_rgba, native_width_px, native_height_px),
            })
            .is_ok()
        {
            self.window.peek_thumbnail_pending = Some(target);
        }
        Ok(())
    }

    /// Remember a decoration-worker decode in the peek cache, under the identity the peek asks by:
    /// the decoder's content key for an OSC 1337 payload, the normalized path for a named file.
    ///
    /// Two reasons, one seam. A stream payload is the one image the peek cannot go and fetch — the
    /// bytes were in the stream, the session decoded them once, and nothing else will ever ask for
    /// them again — so catching it here is the only way the `[image]` placeholder can peek at all.
    /// A named file *can* be re-read, but under the 2026-08-04 verification ruling the session has
    /// just had a worker open, size-check, format-check and decode it in order to earn the resting
    /// underline; letting that decode reach the flyout's cache is what makes the promised peek
    /// appear at once instead of after a second read of the same file. The ruling names that as the
    /// beneficial side effect to preserve, and this is where it is preserved.
    ///
    /// One file still has one entry: the key is `normalized_local_image_path_key`, the very key
    /// `PeekSubject::from_path` computes, so this fills the entry the peek would have created
    /// rather than adding a second one that could disagree.
    pub(in crate::runtime) fn remember_decode_for_peek(
        &mut self,
        task: &bt_term::InlineImageTask,
        decoded: Option<&bt_term::DecodedInlineImage>,
    ) {
        let Some(decoded) = decoded else {
            return;
        };
        let cache_key = peek_cache_key_for_decode(&task.source, decoded);
        self.window.peek_cache.insert(
            cache_key,
            PeekCacheEntry::Ready {
                key: decoded.key.clone(),
                rgba: Arc::clone(&decoded.rgba),
                width_px: decoded.width_px,
                height_px: decoded.height_px,
                native_size: decoded.native_size,
            },
        );
    }

    /// **Take delivery of a glance card's first page** (user ruling 2026-08-25).
    ///
    /// The answer is filed only against the slot that asked for it — same file, same box — so a
    /// page arriving after the pointer has moved to another row is dropped where it lands, which
    /// is the cancellation every other hover in this window performs the same way.
    ///
    /// A [`PeekPageOutcome::Unchanged`] writes nothing and owes no frame: the pixels it is about
    /// are already on the card. Anything else replaces both the pixels and the modification time
    /// they were drawn at, including the `None` that means this file will not raster — a card that
    /// kept the *previous* file's page because this one failed would be a card showing the wrong
    /// document.
    pub(in crate::runtime) fn complete_peek_page(
        &mut self,
        path: &Path,
        page: u32,
        fit: (u32, u32),
        outcome: PeekPageOutcome,
    ) -> Result<()> {
        let Some(slot) = self
            .window
            .peek_page
            .as_mut()
            .filter(|slot| slot.path == path && slot.fit == fit)
        else {
            return Ok(());
        };
        let PeekPageOutcome::Drawn { mtime, raster } = outcome else {
            return Ok(());
        };
        // **A file that has been written is a different document, and the pages already drawn are
        // of the old one** (user ruling 2026-08-26). The single-slot version could not meet this:
        // it held one page, so replacing it *was* the whole cache. A run of pages can be half a
        // report from before a save and half from after — a reader scrolling through would watch
        // the document change under the hand, page by page, as each one happened to be re-asked.
        // So the stamp is what the cache belongs to, and a new one empties it.
        if slot.mtime != mtime && !slot.pages.is_empty() {
            slot.pages.clear();
            // And every page has to be asked again, including the ones a question is already out
            // about — those answers are about the file this is no longer.
            slot.asked.clear();
            slot.asked.insert(page);
        }
        slot.mtime = mtime;
        if let Some(raster) = raster {
            slot.keep(
                page,
                PeekPageRaster {
                    key: peek_page_texture_key(path, mtime, page, raster.width, raster.height),
                    rgba: Arc::from(raster.rgba.into_boxed_slice()),
                    width_px: raster.width,
                    height_px: raster.height,
                },
            );
        }
        // The card is chrome, and a page that lands while it is up owes the frame that shows it —
        // nothing else is going to move the pointer.
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Take delivery of the flyout's display-sized raster. Only the question still outstanding is
    /// answered here: an earlier size arriving after the viewport moved on leaves the newer request
    /// in flight rather than asking for it twice.
    pub(in crate::runtime) fn complete_peek_scale(
        &mut self,
        scaled: bt_term::ScaledInlineImage,
    ) -> Result<()> {
        let delivered: PeekThumbnailTarget = (
            scaled.content_key.clone(),
            scaled.width_px,
            scaled.height_px,
        );
        // **Two hovers ask down this one lane.** The terminal's inline-image
        // flyout is one; the file tree's glance card is the other, and it asks
        // here rather than through a fourth worker verb because the question is
        // identical — one content key at one display size — and the answer is the
        // same pixels. Which of them asked is the target tuple, which each of
        // them recorded before sending. The card is checked first and returns:
        // on the day both want the same picture at the same size the flyout will
        // simply find its slot cold and ask again, which costs one resample of an
        // image already decoded.
        if self.window.peek_card_pending.as_ref() == Some(&delivered) {
            self.window.peek_card_pending = None;
            self.window.peek_picture = Some(PeekThumbnail::from_scaled(scaled));
            // The card is chrome, and a picture that lands while it is up owes
            // the frame that shows it.
            if self.refresh_overlay() {
                self.present_chrome_change()?;
            }
            return Ok(());
        }
        if self.window.peek_thumbnail_pending.as_ref() == Some(&delivered) {
            self.window.peek_thumbnail_pending = None;
        }
        self.window.peek_thumbnail = Some(PeekThumbnail::from_scaled(scaled));
        if let Some(active) = self.window.peek_hover.active.clone() {
            self.show_or_request_peek(&active)?;
        }
        Ok(())
    }

    /// The shape the glance card's head offers, if the pointer is on it.
    ///
    /// A card whose head is pressed but not yet carried keeps the open hand
    /// rather than swapping to the closed one, and that is [`float_grasp`]'s own
    /// K113: a press that has not become a drag has not made anything happen,
    /// and a cursor that changed there would say it had. The closed hand arrives
    /// with the carry, through the float the promotion opens.
    pub(in crate::runtime) fn file_peek_head_grasp(&self) -> Option<FloatGrasp> {
        if !self.file_peek_promotes() {
            return None;
        }
        let at = self.window.pointer_position?;
        let peek = self.window.file_peek.as_ref()?;
        let head = peek.head.filter(|_| peek.clock.is_shown())?;
        file_peek::contains(head, [at.x as f32, at.y as f32]).then_some(FloatGrasp::Head)
    }
}
