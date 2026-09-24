//! `git` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{
    Ask, Fading, FilesFocusArrival, GitMenuDraw, GitMenuState, GitOrigin, GitPromptDraw,
    GitPromptState, GitRowKey, GraphFilterMenuState, GraphView, LeafId, MarkdownCaretPaint,
    MarkdownCaretSeat, MenuPaint, Popup, PreviewDocument, PreviewSurface, ProseParagraph,
    RenameExit, Runtime, TransferRefusal, answers_for, cli, float, float_git_hover,
    float_git_page_shown, float_graph_hover, git, git_answer_notice, git_document_answer,
    git_document_question, git_full_path, git_graph, git_panel, git_surfaces_wanting_reread,
    graph_key_of, hang_watch, i18n, input, markdown_gap_paragraph, marks, native_window, preview,
    profiles, recoverable_clipboard_write, restore, seats, settling, text_field, toast, web_thumb,
};
use anyhow::Context;
use anyhow::{Result, anyhow};
use bt_layout::SeatId;
use bt_render::{FrameSource, FrameTrigger};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use winit::dpi::PhysicalPosition;
use winit::event::{Ime, KeyEvent, MouseScrollDelta};
use winit::keyboard::{Key, NamedKey};

impl Runtime<'_> {
    /// One card per thing a second launch asked for and did not get — the same
    /// door and the same cap [`Self::honour_command_line`] uses, because they are
    /// the same event arriving through two front doors.
    pub(crate) fn report_launch_refusals(&mut self, refusals: Vec<cli::CliRefusal>) -> Result<()> {
        for refusal in refusals {
            self.toast(
                toast::ToastKind::Error,
                toast::ToastAnchor::Window,
                None,
                refusal.notice(),
            )?;
        }
        Ok(())
    }

    /// The engine answered a find (§7.7 ②).
    ///
    /// **And the keyboard comes back here, not merely at the call.** Starting a
    /// find moves the Win32 focus into the page, and it does so *asynchronously*
    /// — measured on the machine, 2026-08-22: a capsule over a page took exactly
    /// one character, the one that started the find, and every keystroke after
    /// it went to the document. A `SetFocus` issued beside `Find::Start` loses
    /// that race; one issued when the engine has answered does not, because the
    /// answer is the engine having finished.
    ///
    /// Only while the caret is in the field. B81's second stance — search open,
    /// hands back on the surface — is a reader who has deliberately given the
    /// page the keyboard, and taking it away from them once per match count
    /// would be this window arguing with a click.
    pub(in crate::runtime) fn web_find_reported(&mut self, count: i32, active: i32) -> Result<()> {
        if self.window.search.is_focused()
            && let Ok(native) = native_window(&self.window.window)
        {
            let _ = bt_platform::take_keyboard_focus(native);
        }
        if !self.window.search.report_engine_matches(count, active) {
            return Ok(());
        }
        self.after_search_change()
    }

    /// **Say where the working tree landed, and offer the way back** (user
    /// ruling, 2026-08-19).
    ///
    /// The card carries a verb only when there is somewhere to go back *to* — a
    /// branch `HEAD` was on before this move. Starting from a detached `HEAD`, or
    /// from a repository with no branch at all, there is nothing a single press
    /// could restore, and a button that said `Back to` with nothing after it
    /// would be worse than no button.
    fn announce_checkout(&mut self, host: &git::GitHost, target: &str) -> Result<()> {
        let anchor = self.git_toast_anchor(host);
        let from = self.window.checkout_from.take();
        let said = match from.as_ref().is_some_and(|(_, _, detach)| *detach) {
            true => git_graph::checkout_detached_notice(target),
            false => crate::i18n::checkout_notice(target),
        };
        let back = from.and_then(|(root, branch, _)| Some((root, branch?)));
        let Some((root, branch)) = back.filter(|(_, branch)| branch != target) else {
            return self.toast(toast::ToastKind::Info, anchor, None, said);
        };
        let verb = format!("{}{branch}", git_graph::graph_leave_detached());
        let id = self.toast_with_verb(toast::ToastKind::Info, anchor, said, &verb)?;
        self.window.checkout_undo = Some((id, root, branch));
        Ok(())
    }

    /// The card's verb: stand back on the branch this move left.
    ///
    /// It goes through the same door every other checkout goes through, so the
    /// tree being dirty asks the same question here as it does on a menu row —
    /// undoing a move is still a move.
    pub(in crate::runtime) fn take_checkout_undo(&mut self, card: toast::ToastId) -> Result<()> {
        let Some((id, root, branch)) = self.window.checkout_undo.take() else {
            return Ok(());
        };
        if id != card {
            self.window.checkout_undo = Some((id, root, branch));
            return Ok(());
        }
        let Some(origin) = self.git_origin_for_root(&root) else {
            return Ok(());
        };
        let said = branch.clone();
        self.ask_to_checkout(&origin, root, branch, said, restore::GitCheckoutKind::Stand)
    }

    pub(crate) fn apply_git_panel(&mut self, enabled: bool) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.git_panel = enabled;
        if !self.app.settings_store.store(settings) {
            return Ok(false);
        }
        if !enabled {
            for tab in &mut self.window.tabs {
                tab.git_trees.clear();
            }
            // **And the floating pages**, on the same sentence (§7.1.6g ②): a
            // switch that only stopped asking would still be holding what the
            // user has just said they do not want it holding, and a window is
            // not exempt from that for standing outside the tabs.
            for win in self.window.float.live_windows_mut() {
                if let Some(files) = win.files_mut() {
                    files.git = git::GitCache::default();
                }
            }
        }
        // Chrome, for [`Self::set_files_view`]'s reason — and here the strip
        // itself is arriving or leaving, so the column's body changes height
        // with it.
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// **The gap's own empty line, as the one paragraph a composition on it is**
    /// (§7.1.3q) — `None` whenever the caret is not in a gap or nothing is being
    /// composed there.
    ///
    /// The very rectangle the painter drew it in
    /// ([`build_preview_markdown_body`]'s gap arm), because a candidate list
    /// placed from a second derivation is a list standing beside the caret it
    /// claims to follow.
    pub(in crate::runtime) fn markdown_gap_paragraphs(
        &self,
        surface: PreviewSurface,
        scale: f32,
        caret: Option<&MarkdownCaretPaint>,
    ) -> Option<Vec<ProseParagraph>> {
        let caret = caret.filter(|caret| caret.lit)?;
        let preedit = caret.preedit.as_ref()?;
        let MarkdownCaretSeat::Gap { after, line_height } = caret.seat else {
            return None;
        };
        let offset = self.preview_pane(surface)?.caret.caret;
        let body = self.preview_surface_body_rect(surface, scale)?;
        let metrics = seats::preview_markdown_metrics(scale);
        let (left, right) = preview::markdown_measure_box(body, metrics);
        let pane = self.preview_pane(surface)?;
        let PreviewDocument::Markdown { layout, .. } = &pane.doc else {
            return None;
        };
        let top = body[1] + metrics.padding_y - pane.scroll[1]
            + after
                .and_then(|index| layout.get(index))
                .map_or(0.0, |placed| placed.top + placed.height);
        Some(vec![ProseParagraph {
            start: offset,
            splice: Some(preedit.splice(0)),
            paragraph: markdown_gap_paragraph(
                preedit,
                [left, top, right.max(left), top + line_height],
                metrics,
                line_height,
                &bt_render::chrome_palette(),
            ),
        }])
    }

    pub(crate) fn disable_git_worker(&mut self) -> bool {
        git::disable_git_worker_state(
            &mut self.app.git_worker_running,
            &mut self.app.git_worker_notice_pending,
        )
    }

    /// Take every repository answer the worker has finished.
    ///
    /// The third of the same shape, down to the silence: a tab or a column that
    /// went away while a `git status` was running has nowhere to put the answer,
    /// and a column that has since been re-rooted refuses it — both are the
    /// cancellation, arriving as a dropped result. Only an answer that actually
    /// changed something the active tab is drawing asks for a new frame.
    pub(crate) fn apply_git_results(
        &mut self,
        batch: &mut Vec<git::GitResponse>,
        lane_gone: bool,
    ) -> Result<()> {
        let mut changed = lane_gone;
        // A document landing is not a chrome change (G-3): what it turns from
        // "Loading …" into lines is the renderer's own preview surface, exactly
        // as a head read does — see the tail of [`Self::apply_preview_results`]
        // for what asking `refresh_chrome` alone costs.
        let mut body = false;
        for response in answers_for(batch, |response| self.owns(response.owner())) {
            let host = response.host;
            // **The asker has to still be there.** A column that closed,
            // a graph that was shut and a window that was dismissed are
            // one cancellation arriving as a dropped answer — and each
            // says so in its own register, because each is addressed in
            // its own way.
            //
            // **The float is asked first, and without a tab.** That is
            // the whole of why its host carries an epoch (user ruling,
            // 2026-08-19): a pinned window floats *across* tabs by
            // ruling, so "is that tab still open" is not the question
            // here, and dropping a page's answer because the tab the
            // window was born in has since closed would strand it
            // mid-read with no second asking owed.
            let floating = match &host {
                git::GitHost::Float { id, .. } => {
                    if self
                        .window
                        .float
                        .live(*id)
                        .and_then(float::FloatWin::files)
                        .is_none()
                    {
                        continue;
                    }
                    Some(*id)
                }
                git::GitHost::Column(_) | git::GitHost::Graph { .. } => None,
            };
            let tab_index = self.window.tabs.iter().position(|tab| tab.id == host.tab());
            if floating.is_none() {
                let Some(index) = tab_index else {
                    continue;
                };
                let tab = &self.window.tabs[index];
                match &host {
                    git::GitHost::Column(leaf) if !tab.files.contains_key(&leaf.seat) => {
                        continue;
                    }
                    git::GitHost::Graph { root, .. } if !tab.git_graphs.contains_key(root) => {
                        continue;
                    }
                    _ => {}
                }
            }
            // **The two answers that are documents go to the pool**
            // (G-3), which is a tab's and not a column's: a diff opened
            // from one column is the same buffer whichever pane shows
            // it, and a column re-rooted since does not make the
            // document it opened wrong. A float's diff lands in the pool
            // of the tab that was in front when the row was pressed,
            // which is the tab its host recorded for exactly this.
            let answer = match git_document_answer(response.answer) {
                Ok((source, outcome)) => {
                    let Some(index) = tab_index else {
                        continue;
                    };
                    let tab = &mut self.window.tabs[index];
                    // Evicted, or never opened — the cancellation,
                    // arriving as a dropped result.
                    let Some(buffer) = tab.preview_pool.get_mut(&source) else {
                        // **The glance's own slot**, exactly as a head
                        // read's answer finds it (`apply_preview_results`):
                        // a hover over a commit's file asks git and never
                        // enters the pool, so its answer lands here or
                        // nowhere. Matched by source, so a pointer that
                        // has moved on is the cancellation arriving as a
                        // dropped result.
                        if let Some(peek) = self
                            .window
                            .peek_buffer
                            .as_mut()
                            .filter(|peek| peek.source == source)
                        {
                            match outcome {
                                Ok(text) => peek.accept(preview::HeadOutcome::Read {
                                    text,
                                    truncated: false,
                                    mtime: None,
                                    // A composed document's text came out of a
                                    // program as a `String`; there are no bytes
                                    // here to be in doubt about (§7.32).
                                    content_says_text: true,
                                    encoding: preview::HeadEncoding::Utf8,
                                    lossy: false,
                                }),
                                Err(fault) => {
                                    peek.decline(git_panel::fault_sentence(&fault));
                                }
                            }
                            body |= index == self.window.active_tab;
                        }
                        continue;
                    };
                    match outcome {
                        Ok(text) => buffer.accept(preview::HeadOutcome::Read {
                            text,
                            // Nothing truncated it and no disk wrote it:
                            // both of those are facts about a file, and
                            // this document is not one.
                            truncated: false,
                            mtime: None,
                            // And nothing sniffed it, for the same reason:
                            // a program handed this window a `String`
                            // (§7.32).
                            content_says_text: true,
                            encoding: preview::HeadEncoding::Utf8,
                            lossy: false,
                        }),
                        Err(fault) => buffer.decline(git_panel::fault_sentence(&fault)),
                    }
                    body |= index == self.window.active_tab;
                    continue;
                }
                Err(answer) => answer,
            };
            // **A checkout that went through is about the whole
            // repository, not about the surface that asked for it.**
            // After it, every branch head, every status and every
            // history of that repository is about somewhere else — and
            // a panel still drawing the old branch beside a graph
            // drawing the new one is exactly the disagreement this
            // subsystem was built to prevent. Noted before the answer is
            // filed, because filing moves it.
            let moved = match &answer {
                git::GitAnswer::Checkout {
                    root,
                    outcome: Ok(()),
                    ..
                } => Some(root.clone()),
                // **A ref write is the same claim** (v2 ④): a branch
                // created, renamed or deleted changes the pills on every
                // row of every history of this repository, and a tracking
                // checkout moves `HEAD` as well. A panel still drawing
                // the old list beside a graph drawing the new one is the
                // one disagreement this subsystem exists to prevent.
                git::GitAnswer::Write {
                    root,
                    verb,
                    outcome: Ok(()),
                    ..
                } if verb.moves_refs() => Some(root.clone()),
                _ => None,
            };
            // **A verb git refused is a notice** (user ruling,
            // 2026-08-16), raised here and nowhere else: this is the one
            // instant the refusal *happens*, so it is raised once per
            // answer rather than re-derived on every frame from a
            // remembered sentence — which is what the red banner was, and
            // why it outstayed the thing it was about.
            //
            // The two verbs and only those two: a repository, a status, a
            // branch list or a history that would not read is a persistent
            // fault, and those keep the page's own quiet sentence (see
            // `git_panel::build`). Nothing about a machine with no git is
            // going to change in six seconds.
            // **A checkout that went through says so, and offers the
            // way back** (user ruling, 2026-08-19). Moving where you are
            // standing is reversible and named, so it is not asked about
            // on a clean tree — but a page that changed under a reader
            // with nothing to attribute the change to is the same silence
            // in a different place, and the honest answer to a reversible
            // move is a notice that says where you landed and one press
            // that undoes it.
            //
            // **Both spellings, because both moved you.** A tracking
            // checkout comes back as a `Write` — one command that made
            // the branch and stood on it — and a reader who pressed
            // `Checkout tracking` is owed the same sentence and the same
            // one press back as a reader who pressed `Checkout`. It says
            // the *local* name, which is where they are now standing.
            let landed = match &answer {
                git::GitAnswer::Checkout {
                    target,
                    outcome: Ok(()),
                    ..
                } => Some(target.clone()),
                git::GitAnswer::Write {
                    verb: git::GitWriteVerb::CheckoutTracking { name },
                    outcome: Ok(()),
                    ..
                } => Some(git::tracking_local_name(name).to_owned()),
                _ => None,
            };
            if let Some(landed) = landed {
                self.announce_checkout(&host, &landed)?;
            }
            if let Some(words) = git_answer_notice(&answer) {
                let anchor = self.git_toast_anchor(&host);
                self.toast(
                    toast::ToastKind::Error,
                    anchor,
                    Some(git_panel::git_toast_title().to_owned()),
                    words,
                )?;
                // `self` was borrowed mutably above; the tab has to be
                // taken again for the filing below.
            }
            let filed = if let Some(id) = floating {
                // The window's own cache, found by the epoch that minted
                // its view: with several floats on screen that is also
                // what *addresses* the answer, so two windows on one
                // repository never fill in with each other's readings.
                self.window
                    .float
                    .live_mut(id)
                    .and_then(float::FloatWin::files_mut)
                    .is_some_and(|files| files.git.accept(answer))
            } else {
                let Some(index) = tab_index else {
                    continue;
                };
                let tab = &mut self.window.tabs[index];
                match &host {
                    git::GitHost::Column(leaf) => tab
                        .git_trees
                        .get_mut(&leaf.seat)
                        .is_some_and(|cache| cache.accept(answer)),
                    git::GitHost::Graph { root, .. } => {
                        match tab.git_graphs.get_mut(root) {
                            Some(state) => {
                                let filed = state.cache.accept(answer);
                                // The lanes are a reading of the log, so they
                                // are brought level with it the moment it
                                // moves — a page appended, or a checkout that
                                // replaced the history altogether.
                                if filed {
                                    state.sync();
                                }
                                filed
                            }
                            None => false,
                        }
                    }
                    // Answered above, by the branch that needed no tab.
                    git::GitHost::Float { .. } => false,
                }
            };
            if let Some(root) = moved.filter(|_| filed) {
                if let Some(index) = tab_index {
                    let tab = &mut self.window.tabs[index];
                    for cache in tab.git_trees.values_mut() {
                        if cache.root() == Some(root.as_path()) {
                            cache.refresh();
                        }
                    }
                    if let Some(state) = tab.git_graphs.get_mut(&root) {
                        state.cache.refresh();
                        // The lanes are a reading of the history that was.
                        state.invalidate();
                    }
                }
                // **And every floating page on that repository** (user
                // ruling, 2026-08-19). A window standing on the branch
                // that was just left is the same disagreement a column
                // would be, and it is not in any tab to have been swept
                // with one.
                for win in self.window.float.live_windows_mut() {
                    if let Some(files) = win.files_mut()
                        && files.git.root() == Some(root.as_path())
                    {
                        files.git.refresh();
                    }
                }
            }
            // A float is window-level chrome and is on screen whichever
            // tab is in front, so its answer is always owed a frame.
            changed |= filed && (floating.is_some() || tab_index == Some(self.window.active_tab));
        }
        if body {
            self.refresh_preview_for_layout();
            self.refresh_chrome();
            self.present_chrome_change()?;
        } else if changed && self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        // **A seek walks one page at a time, and each page arrives here** (D2).
        // After the rebuild above, because what the step needs is the list the
        // page it was waiting for is now part of.
        if changed && self.step_graph_seeks()? && self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Which surface a repository answer's notice belongs over.
    ///
    /// A column names its own seat. A graph names none — [`git::GitHost::Graph`]
    /// is keyed by tab and root, because the *document* is the repository's and
    /// not the pane's — so the pane showing it is looked up the same way
    /// [`Self::git_graphs`] finds it: the preview seat whose buffer is that
    /// graph. When neither can be found this frame (another tab is in front, the
    /// pane was closed while git worked) the notice falls to the window's corner,
    /// which is what [`toast::ToastAnchor::Window`] is for — and it will find its
    /// column again on the frame that column comes back, because the anchor is
    /// resolved afresh every time it is drawn.
    fn git_toast_anchor(&self, host: &git::GitHost) -> toast::ToastAnchor {
        match host {
            git::GitHost::Column(leaf) => toast::ToastAnchor::FilesColumn(leaf.seat),
            git::GitHost::Graph { root, .. } => self
                .seats
                .preview_seats()
                .into_iter()
                .find(|seat| {
                    matches!(
                        self.preview_buffer_on(self.preview_here(*seat))
                            .map(|buffer| &buffer.source),
                        Some(preview::PreviewSource::GitGraph { root: showing })
                            if showing == root
                    )
                })
                .map_or(toast::ToastAnchor::Window, |seat| {
                    toast::ToastAnchor::PreviewSeat(self.leaf_here(seat))
                }),
            // A window is not a seat and has no pane for a card to stand over,
            // which is exactly the case the window's own corner is for.
            git::GitHost::Float { .. } => toast::ToastAnchor::Window,
        }
    }

    /// Put everything the repository under this column has not been asked yet.
    ///
    /// **The seam G-2 will keep.** The plan is derived from the cache
    /// ([`git::GitCache::pending_questions`]) rather than remembered here, so
    /// this is idempotent and R31 holds however often it is called: a slot that
    /// is already in flight or already answered asks for nothing. What is
    /// temporary is only *who calls it* — today the debug channel, tomorrow the
    /// Git page being open.
    pub(in crate::runtime) fn ask_git_for_column(&mut self, seat: SeatId, root: &str) {
        if root.trim().is_empty() {
            return;
        }
        let active = self.window.active_tab;
        let tab_id = self.window.tabs[active].id;
        let cache = self.window.tabs[active].git_trees.entry(seat).or_default();
        cache.retarget(std::path::Path::new(root));
        let questions = cache.pending_questions();
        // Marked before any send, so a second call on the same frame sees
        // questions already asked rather than asking them twice.
        for question in &questions {
            cache.mark_pending(question);
        }
        for question in questions {
            if !self.app.git_worker.request(git::GitRequest {
                window: self.window_id(),
                host: git::GitHost::Column(LeafId { tab: tab_id, seat }),
                question,
            }) {
                self.disable_git_worker();
                return;
            }
        }
    }

    /// R28's chord: turn the focused column's page over.
    ///
    /// **Which column** is the question this has to answer and the pointer does
    /// not. It takes the tab's Files columns in tree order and turns the first —
    /// which is the only column in every layout anyone has built, and in the two
    /// that have two, the leading one is the one the chord's own verb
    /// (`Ctrl+Shift+B`) opens. A chord with no column to work does nothing, and
    /// so does one pressed while the Git panel is off: a chord for a surface that
    /// is not there is not an error.
    pub(in crate::runtime) fn toggle_git_page(&mut self) -> Result<()> {
        if !self.git_panel_on() {
            return Ok(());
        }
        let Some(seat) = self.seats.files().into_iter().next() else {
            return Ok(());
        };
        let active = self.window.active_tab;
        let view = self.window.tabs[active]
            .files
            .get(&seat)
            .map_or(seats::FilesView::Files, |state| state.view);
        self.set_files_view(seat, view.toggled())
    }

    /// **A Git row's body, pressed** (G-3) — the row itself, not its verbs.
    ///
    /// Three rows do something and the page says which
    /// ([`git_panel::row_document`]): a changed file opens its diff, a commit
    /// turns its file list over, and one of those files opens that commit's
    /// reading of it. Nothing is decided here — this resolves the repository the
    /// press is about and carries out the answer, exactly as
    /// [`Self::press_git_act`] carries out `press_outcome`'s.
    pub(in crate::runtime) fn press_git_row(&mut self, seat: SeatId, index: usize) -> Result<()> {
        // **A press does not reach the page's furniture** (user report,
        // 2026-08-25). A heading, the masthead and a notice are not controls,
        // and the hit test hands this function their index for one reason only:
        // a hand resting inside a heading is what reveals the `+` in its corner,
        // and that verb answers ahead of the row. Left unguarded, a click on the
        // word `BRANCHES` moved the keyboard onto it — and the selected ground
        // is the block the reader reported.
        if self
            .window
            .git_pages_shown
            .get(&seat)
            .and_then(|page| page.rows.get(index))
            .is_none_or(git_panel::GitRow::is_furniture)
        {
            return Ok(());
        }
        // **The selection follows the hand**, before anything this press could
        // also mean and whether or not the row has a verb — the graph's own
        // first line (`press_graph_row`), and it is what makes `↑` after a click
        // step from the row you clicked instead of from the top of the list.
        //
        // **Onto an item, and not onto a header** (user report, 2026-09-14) —
        // `git_panel::GitRow::seats_the_keyboard`, which is where that report is
        // answered and why. The guard above lets the sub-group header through,
        // because a press is the whole of what that row is for; unguarded here,
        // the press left the keyboard's own number on the header, and a header
        // has one picture for the keyboard and the pointer alike — so the hover
        // brightened it once and could never move it again.
        if self
            .window
            .git_pages_shown
            .get(&seat)
            .and_then(|page| page.rows.get(index))
            .is_some_and(git_panel::GitRow::seats_the_keyboard)
        {
            self.select_git_row(seat, index);
        }
        let active = self.window.active_tab;
        // The rows as they are **on screen**, and the root as the *cache* has
        // it: a document opened against a root the column has since left would
        // be a diff of a file in another repository, and the cache is the one
        // thing that knows which repository this column found.
        let Some(root) = self.window.tabs[active]
            .git_trees
            .get(&seat)
            .and_then(git::GitCache::root)
            .map(Path::to_path_buf)
        else {
            return Ok(());
        };
        // **The REMOTES sub-group's own press** (T9), asked before the documents
        // because it is not one: it opens and shuts what is under it, and the
        // answer lives on the column's durable state rather than in a preview.
        if self
            .window
            .git_pages_shown
            .get(&seat)
            .and_then(|page| page.rows.get(index))
            .is_some_and(git_panel::row_toggles_remotes)
        {
            let state = self.window.tabs[active].files.entry(seat).or_default();
            state.git_remotes_open = !state.git_remotes_open;
            self.mark_session_dirty(Instant::now());
            if self.refresh_chrome() {
                self.present_chrome_change()?;
            }
            return Ok(());
        }
        let Some(open) = self
            .window
            .git_pages_shown
            .get(&seat)
            .and_then(|page| page.rows.get(index))
            .and_then(|row| git_panel::row_document(row, &root))
        else {
            return Ok(());
        };
        match open {
            git_panel::GitRowOpen::Document {
                source,
                name,
                renamed_from,
            } => self.open_git_document(seat, source, name, renamed_from),
            git_panel::GitRowOpen::Expand { hash } => self.expand_commit(seat, &hash),
            // **A branch row is the ordinary verb** and goes straight through on
            // a clean tree (tier 3 of the 2026-08-19 ruling) — but not over work
            // it would carry with it, which is what `ask_to_checkout` decides.
            git_panel::GitRowOpen::Checkout { target, detach } => {
                let said = target.clone();
                let kind = if detach {
                    restore::GitCheckoutKind::Detach
                } else {
                    restore::GitCheckoutKind::Stand
                };
                self.ask_to_checkout(&GitOrigin::Column(seat), root, target, said, kind)
            }
        }
    }

    /// Put a repository's document on the preview seat, and ask for its body.
    ///
    /// **Two steps, and the mock-up's own shape** (G93): the seat is taken
    /// first and the content arrives afterwards, because the content is not on
    /// a disk to be read synchronously. What the mock-up did next was assign the
    /// text into the buffer it had just opened; what happens here is a question
    /// on the git lane, and [`Self::apply_git_results`] files the answer into
    /// the same buffer.
    ///
    /// **A document already read is not read again.** A diff is a *snapshot* —
    /// what the repository said at the moment it was asked — and staging the
    /// file it is about does not rewrite the page you are looking at. That is
    /// the honest reading of it: the pane's head names the document, the panel
    /// beside it has already redrawn, and a diff that silently became a diff of
    /// something else while you were reading it would be the one surface here
    /// that changes under you. Pressing the row again re-asks, because that is a
    /// person saying "now".
    pub(in crate::runtime) fn open_git_document(
        &mut self,
        seat: SeatId,
        source: preview::PreviewSource,
        name: String,
        renamed_from: Option<String>,
    ) -> Result<()> {
        let Some(surface) = self.preview_landing_surface() else {
            return Ok(());
        };
        self.open_preview_source_on(surface, source.clone(), name)?;
        let unread = self
            .preview_pool
            .get(&source)
            .is_some_and(|buffer| buffer.load == preview::PreviewLoad::Pending);
        if !unread {
            return Ok(());
        }
        let Some(question) = git_document_question(&source, renamed_from) else {
            return Ok(());
        };
        let active = self.window.active_tab;
        let tab_id = self.window.tabs[active].id;
        if !self.app.git_worker.request(git::GitRequest {
            window: self.window_id(),
            host: git::GitHost::Column(LeafId { tab: tab_id, seat }),
            question,
        }) {
            self.disable_git_worker();
        }
        Ok(())
    }

    /// Put this page's selection on one row (焦点跟随可见视图, 2026-08-19).
    ///
    /// The **column's** state and not the frame's, exactly as
    /// [`Self::select_graph_row`] writes the graph's: the number outlives the
    /// frame it was chosen on, and the frame clamps it to the list it has just
    /// built ([`git_panel::GitPanelContent::selected`]).
    ///
    /// A seat with no state is a seat with no Git page to have selected anything
    /// on, so nothing is vivified here — the map's keys are the tab's columns
    /// (`files_match_files_seats`) and this is not a door that should add one.
    fn select_git_row(&mut self, seat: SeatId, index: usize) {
        let active = self.window.active_tab;
        if let Some(state) = self.window.tabs[active].files.get_mut(&seat) {
            state.git_sel = Some(index);
        }
    }

    /// Scroll a Git page until one of its rows is whole on screen — the twin of
    /// [`Self::reveal_graph_row`], one surface along.
    ///
    /// Measured against the page that was **drawn** (`git_pages_shown`) rather
    /// than a rebuild, for `scroll_git_panel`'s stated reason: the row the
    /// keyboard just moved to is a row in the list the reader is looking at.
    fn reveal_git_row(&mut self, seat: SeatId, index: usize) {
        let scale = self.window.renderer.scale_factor() as f32;
        let Some(page) = self.window.git_pages_shown.get(&seat) else {
            return;
        };
        let Some(rect) = seats::files_pane_rect(&self.seat_layout, seat) else {
            return;
        };
        let body = seats::files_pane_geometry(rect, scale, true).body;
        let wanted = git_panel::git_panel_geometry(body, page, scale).reveal(index);
        let active = self.window.active_tab;
        self.window.tabs[active].git_scroll.insert(seat, wanted);
    }

    /// One key, with a column holding the keyboard and its Git page showing.
    ///
    /// [`Self::files_tree_key`]'s opposite number, and it answers on the same
    /// terms: **everything is consumed**, whether or not it moved anything,
    /// because with a column focused there is nothing to type into and a letter
    /// that fell through to the encoder would land in a shell the user is not
    /// looking at (D49).
    ///
    /// What each key *means* is [`git_panel::panel_key`]'s, which speaks the
    /// graph's vocabulary — so the two git surfaces answer the same six keys and
    /// the rule about where `↓` lands is written once.
    pub(in crate::runtime) fn git_page_key(
        &mut self,
        seat: SeatId,
        event: &KeyEvent,
    ) -> Result<bool> {
        let Some(key) = graph_key_of(&event.logical_key, self.window.modifiers) else {
            // Still the column's key: it owns the keyboard, so nothing here
            // reaches a shell. This page simply has nothing to do with this one.
            return Ok(true);
        };
        // Holding Enter down on a commit would turn it over once per repeat.
        // Travel repeats happily; opening is a verb you mean once — the tree's
        // own line, one page along.
        if event.repeat
            && matches!(
                key,
                git_graph::GraphKey::Enter | git_graph::GraphKey::Compare
            )
        {
            return Ok(true);
        }
        // The page as it is **on screen**: the row a key is about is a row the
        // reader is looking at, which is `git_pages_shown` and never a rebuild.
        let Some(page) = self.window.git_pages_shown.get(&seat).cloned() else {
            return Ok(true);
        };
        let selected = match git_panel::panel_key(&page, key) {
            git_graph::GraphKeyAction::None => return Ok(true),
            // **`Esc`'s outermost rung is the column's own** (D47 / §7.1.5):
            // with nothing folded open, giving the keyboard back is what this
            // key means here, and it must never reach a child.
            git_graph::GraphKeyAction::Pass => {
                if self.set_files_keyboard(None, FilesFocusArrival::Keyboard)
                    && self.refresh_chrome()
                {
                    self.present_chrome_change()?;
                }
                return Ok(true);
            }
            git_graph::GraphKeyAction::Select(row) => row,
            // Enter is the row's own press, minus the pointer: whatever a click
            // on it would have done, through the same door — so a changed file
            // opens its diff and a commit turns over, and neither gesture had to
            // be written twice. `press_git_row` moves the selection itself —
            // and on the one row it declines to move it onto, the sub-group
            // header, the arrows that got here are already standing on it.
            git_graph::GraphKeyAction::Toggle(row) => {
                self.press_git_row(seat, row)?;
                return Ok(true);
            }
            git_graph::GraphKeyAction::Collapse(row) => {
                let hash = page.rows.get(row).and_then(|row| match row {
                    git_panel::GitRow::Commit(commit) => Some(commit.hash.clone()),
                    _ => None,
                });
                if let Some(hash) = hash {
                    self.expand_commit(seat, &hash)?;
                }
                // Standing on what was just folded shut, and not on wherever the
                // selection had wandered to inside it: the rows it was in have
                // gone, and the row it came out of is the one still on screen.
                row
            }
            // **The panel gives no comparison** — 「面板不给比较,是裁决不是缺口」
            // (2026-08-16) — so `panel_key` never answers with one. The arms are
            // here so that the day the ruling changes, this function is asked.
            git_graph::GraphKeyAction::Compare(_) | git_graph::GraphKeyAction::LeaveCompare => {
                return Ok(true);
            }
        };
        self.select_git_row(seat, selected);
        self.reveal_git_row(seat, selected);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// One of a Git row's verbs, pressed.
    ///
    /// **The gate is here and not in the worker** (R14): a discard is stopped
    /// *before* a question is built, so the confirmation is a door in front of the
    /// verb rather than a message about one that has already run.
    pub(in crate::runtime) fn press_git_act(
        &mut self,
        seat: SeatId,
        index: usize,
        act: git_panel::GitAct,
    ) -> Result<()> {
        let Some(page) = self.window.git_pages_shown.get(&seat) else {
            return Ok(());
        };
        let Some(row) = page.rows.get(index) else {
            return Ok(());
        };
        // **Three of the verbs are about no file at all** and are answered
        // before a pathspec is gathered, because there is none to gather: one
        // asks the repository for another page of history, one puts a document on
        // the preview seat, one re-reads the whole repository. All three still go
        // through `press_outcome`, so the judgement stays in one place even for
        // the verbs that need nothing from the row they were pressed on.
        match git_panel::press_outcome(act, false) {
            git_panel::GitPress::MoreCommits => return self.load_more_commits(seat),
            git_panel::GitPress::Graph => return self.open_git_graph(seat),
            git_panel::GitPress::Reread => return self.refresh_git_column(seat),
            git_panel::GitPress::Write(_) | git_panel::GitPress::Gate => {}
        }
        // Which files this verb is about: one row's, or a whole group's. The
        // group's list is read off the page that is on screen, which is the same
        // list the user is looking at — asking the cache again could stage a file
        // that arrived between the frame and the press.
        let (paths, untracked) = match row {
            git_panel::GitRow::Change(change) => (vec![change.path.clone()], change.untracked),
            git_panel::GitRow::Heading {
                group: Some(group), ..
            } => {
                let group = *group;
                let paths: Vec<String> = page
                    .rows
                    .iter()
                    .filter_map(|row| match row {
                        git_panel::GitRow::Change(change) if change.group == group => {
                            Some(change.path.clone())
                        }
                        _ => None,
                    })
                    .collect();
                (paths, group == crate::git::GitGroup::Untracked)
            }
            _ => return Ok(()),
        };
        if paths.is_empty() {
            return Ok(());
        }
        // **No judgement here.** What a verb becomes is
        // [`git_panel::press_outcome`]'s answer and this only carries it out, so
        // that "a discard cannot reach git without the gate" is a fact one small
        // function holds rather than a property of this one.
        match git_panel::press_outcome(act, untracked) {
            git_panel::GitPress::Gate => {
                // One file by construction — R14 puts no group discard on the
                // page — so the gate can name what it is about.
                let Some(path) = paths.into_iter().next() else {
                    return Ok(());
                };
                self.raise_dirty_gate(restore::GateRequest::GitDiscard {
                    origin: GitOrigin::Column(seat),
                    path,
                    untracked,
                })?;
                Ok(())
            }
            git_panel::GitPress::Write(verb) => self.write_to_repository(seat, verb, paths),
            git_panel::GitPress::MoreCommits => self.load_more_commits(seat),
            git_panel::GitPress::Graph => self.open_git_graph(seat),
            git_panel::GitPress::Reread => self.refresh_git_column(seat),
        }
    }

    /// Send one write, and dim the rows it is about (R13).
    fn write_to_repository(
        &mut self,
        seat: SeatId,
        verb: crate::git::GitWriteVerb,
        paths: Vec<String>,
    ) -> Result<()> {
        self.issue_git_write(&GitOrigin::Column(seat), verb, paths)
    }

    /// Put the whole commit graph on the preview seat (G24/G100).
    ///
    /// **The document is the repository's, not the column's.** The column is
    /// only where the press happened; what is opened is keyed on the root it
    /// found, so pressing `Graph` in two columns rooted in one repository is
    /// pressing it twice on one document — which is what the preview pool's own
    /// single-instance contract already means by "the same buffer".
    fn open_git_graph(&mut self, seat: SeatId) -> Result<()> {
        let active = self.window.active_tab;
        let Some(root) = self.window.tabs[active]
            .git_trees
            .get(&seat)
            .and_then(git::GitCache::root)
            .map(Path::to_path_buf)
        else {
            return Ok(());
        };
        let source = preview::PreviewSource::GitGraph { root: root.clone() };
        let name = root.file_name().map_or_else(
            || root.to_string_lossy().into_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        // The state before the seat, so the first frame after the seat is taken
        // already has a cache to ask its questions from.
        self.window.tabs[active]
            .git_graphs
            .entry(root)
            .or_insert_with_key(|root| git_graph::GraphState::new(root.clone()));
        let Some(surface) = self.preview_landing_surface() else {
            return Ok(());
        };
        self.open_preview_source_on(surface, source, name)?;
        Ok(())
    }

    /// Ask every open graph what it still needs.
    ///
    /// [`Self::ask_git_for_column`]'s twin, and idempotent for its reason: the
    /// plan is derived from the cache rather than remembered, so a slot already
    /// in flight asks for nothing however often this is called.
    /// **Asked of the tabs that are actually showing a graph**, and not of the
    /// tab on screen. A document torn off into a window keeps reading the plane
    /// of the tab it was opened in (§7.1.3, [`float::FloatPreview`]), so a
    /// floating graph whose reader has since switched tabs would otherwise sit
    /// on a cache nobody ever asked a question for — "Reading the repository…"
    /// for as long as you stayed away.
    fn ask_git_for_graphs(&mut self, tabs: &[usize]) {
        for index in tabs.iter().copied() {
            let tab_id = self.window.tabs[index].id;
            let roots: Vec<std::path::PathBuf> =
                self.window.tabs[index].git_graphs.keys().cloned().collect();
            for root in roots {
                let Some(state) = self.window.tabs[index].git_graphs.get_mut(&root) else {
                    continue;
                };
                let questions = state.cache.pending_questions();
                for question in &questions {
                    state.cache.mark_pending(question);
                }
                for question in questions {
                    if !self.app.git_worker.request(git::GitRequest {
                        window: self.window_id(),
                        host: git::GitHost::Graph {
                            tab: tab_id,
                            root: root.clone(),
                        },
                        question,
                    }) {
                        self.disable_git_worker();
                        return;
                    }
                }
            }
        }
    }

    /// One more page of history for a graph (R23's auto-paging).
    ///
    /// `index` is the tab whose plane holds the document, for
    /// [`Self::ask_git_for_graphs`]'s reason exactly.
    fn extend_graph(&mut self, index: usize, root: &Path) {
        let tab_id = self.window.tabs[index].id;
        let Some(state) = self.window.tabs[index].git_graphs.get(root) else {
            return;
        };
        let Some(question) = state.cache.more_commits() else {
            return;
        };
        // **The guard against asking every frame** is the same one `Load more`
        // relies on: `GitCache::accept` files a later page only when it starts
        // exactly where the list ends, so a duplicate in flight is dropped
        // rather than appended. What stops the flood is that the list is one
        // page longer the moment the first answer lands, and `wants_more` then
        // asks about a further page rather than the same one.
        if !self.app.git_worker.request(git::GitRequest {
            window: self.window_id(),
            host: git::GitHost::Graph {
                tab: tab_id,
                root: root.to_owned(),
            },
            question,
        }) {
            self.disable_git_worker();
        }
    }

    /// What two places differ by (D6) — the compare block's own question.
    ///
    /// Idempotent, and the guard is the cache's rather than a flag here: this is
    /// reached from a derivation that runs every frame, so "have I asked this
    /// already" has to be answerable by the thing holding the answer.
    fn ask_graph_compare(&mut self, index: usize, root: &Path, a: &str, b: Option<&str>) {
        let tab_id = self.window.tabs[index].id;
        let Some(state) = self.window.tabs[index].git_graphs.get_mut(root) else {
            return;
        };
        let Some(question) = state.cache.begin_compare_files(a, b) else {
            return;
        };
        if !self.app.git_worker.request(git::GitRequest {
            window: self.window_id(),
            host: git::GitHost::Graph {
                tab: tab_id,
                root: root.to_owned(),
            },
            question,
        }) {
            self.disable_git_worker();
        }
    }

    /// Take every running seek one step (D2).
    ///
    /// **Driven by the pages arriving and not by the paint**, which is R31's own
    /// discipline applied to a gesture: a seek is a question the reader asked,
    /// each page is one answer to it, and the moment the next question can be
    /// decided is the moment the last answer landed. A version of this that ran
    /// on every frame would be a timer with extra steps.
    ///
    /// Answers whether anything moved, so the caller can decide about a frame.
    fn step_graph_seeks(&mut self) -> Result<bool> {
        // Every tab's seeks and not the tab on screen's, for
        // [`Self::ask_git_for_graphs`]'s reason: a floating graph reads the plane
        // of the tab it was opened in, and a walk that stopped when you switched
        // tabs would be a gesture the reader started and the app quietly dropped.
        let running: Vec<(usize, PreviewSurface)> = self
            .window
            .tabs
            .iter()
            .enumerate()
            .flat_map(|(index, tab)| {
                tab.git_graph_view
                    .iter()
                    .filter(|(_, view)| view.seek.is_some())
                    .map(move |(surface, _)| (index, *surface))
            })
            .collect();
        let mut moved = false;
        for (index, surface) in running {
            let Some((root, seek)) = self.window.tabs[index]
                .git_graph_view
                .get(&surface)
                .and_then(|view| Some((view.root.clone(), view.seek.clone()?)))
            else {
                continue;
            };
            let Some(state) = self.window.tabs[index].git_graphs.get(&root) else {
                continue;
            };
            match git_graph::graph_seek_step(state, &seek) {
                git_graph::GraphSeekStep::Arrived(at) => {
                    let row = self
                        .window
                        .git_graphs_shown
                        .get(&surface)
                        .map(|content| content.commit_row(at));
                    if let Some(view) = self.window.tabs[index].git_graph_view.get_mut(&surface) {
                        view.seek = None;
                    }
                    if let Some(row) = row {
                        self.select_graph_row(surface, row);
                        self.reveal_graph_row(surface, row);
                    }
                    moved = true;
                }
                git_graph::GraphSeekStep::NeedPage => {
                    if let Some(view) = self.window.tabs[index].git_graph_view.get_mut(&surface) {
                        view.seek = Some(git_graph::GraphSeek {
                            pages: seek.pages + 1,
                            ..seek
                        });
                    }
                    self.extend_graph(index, &root);
                }
                git_graph::GraphSeekStep::Waiting => {}
                git_graph::GraphSeekStep::GaveUp => {
                    if let Some(view) = self.window.tabs[index].git_graph_view.get_mut(&surface) {
                        view.seek = None;
                    }
                    let anchor = surface.toast_anchor();
                    self.toast(
                        toast::ToastKind::Info,
                        anchor,
                        None,
                        git_graph::graph_seek_gave_up(&seek.hash),
                    )?;
                    moved = true;
                }
            }
        }
        Ok(moved)
    }

    /// Which graph each preview **surface** is drawing, and how far down it is.
    ///
    /// **Asked of [`Self::preview_surfaces`] and not of the seat tree** (user
    /// report, 2026-08-20). `preview_seats()` is the docked half of that list,
    /// and asking it here was the whole of the defect: a graph torn off into a
    /// window was a graph nobody built, so the window drew its head, its foot
    /// and an empty rectangle — the picture is chrome ([`git_graph::push_graph`])
    /// and the document's body is empty by construction, so there was nothing
    /// else on the surface to give it away.
    pub(in crate::runtime) fn git_graphs(
        &mut self,
        scale: f32,
    ) -> BTreeMap<PreviewSurface, git_graph::GraphContent> {
        let active = self.window.active_tab;
        let showing: Vec<(PreviewSurface, std::path::PathBuf)> = self
            .preview_surfaces()
            .into_iter()
            .filter_map(|surface| {
                let source = self.preview_buffer_on(surface)?.source.clone();
                match source {
                    preview::PreviewSource::GitGraph { root } => Some((surface, root)),
                    _ => None,
                }
            })
            .collect();
        // A surface that has stopped showing a graph keeps no scroll and no open
        // commit: it is not looking at that list any more.
        //
        // **Only the surfaces this pass had standing to judge**, which is
        // [`Self::sweep_preview_panes`]'s own split: a seat lives in its own
        // tab's map and only the tab on screen has its seats in the list above,
        // so a seat entry in any other tab is not stale — it is an entry this
        // frame was never shown. A window is in the list wherever it was born,
        // so its entry answers everywhere.
        for (index, tab) in self.window.tabs.iter_mut().enumerate() {
            tab.git_graph_view.retain(|surface, _| {
                let judged = match surface {
                    PreviewSurface::Seat(_) => index == active,
                    PreviewSurface::Float(_) => true,
                    // Never a key here: the glance card is not in
                    // `preview_surfaces` and has no graph to hold.
                    PreviewSurface::Peek => false,
                };
                !judged || showing.iter().any(|(shown, _)| shown == surface)
            });
        }
        if showing.is_empty() {
            return BTreeMap::new();
        }
        let mut pages = BTreeMap::new();
        for (surface, root) in showing {
            // **The docked ones only.** A window's graph is built in the overlay
            // pass instead ([`Self::preview_float_layer`]), through the very same
            // [`Self::build_git_graph`] — because a float's body moves under a
            // drag that never runs this pass at all, and a picture built against
            // last frame's rectangle would be a list drawn outside its own window.
            // It is the arrangement the float's document already had: "a float's
            // document is built into its layer".
            let PreviewSurface::Seat(_) = surface else {
                continue;
            };
            let Some(body) = self.preview_surface_body_rect(surface, scale) else {
                continue;
            };
            if let Some(content) = self.build_git_graph(surface, &root, body, scale) {
                pages.insert(surface, content);
            }
        }
        pages
    }

    /// **Build one surface's graph, heal it, and raise what it asks git for.**
    ///
    /// The one derivation both hosts run. A preview pane reaches it through
    /// [`Self::git_graphs`] in the chrome pass and a floating window through
    /// [`Self::preview_float_layer`] in the overlay pass, and what they hand in
    /// is the same two facts — which surface, and the rectangle it is drawing
    /// into. Everything a graph is made of is on the far side of this call, so
    /// the two hosts cannot be showing two different readings of one repository.
    fn build_git_graph(
        &mut self,
        surface: PreviewSurface,
        root: &Path,
        body: [f32; 4],
        scale: f32,
    ) -> Option<git_graph::GraphContent> {
        let mut extend: Vec<(usize, std::path::PathBuf)> = Vec::new();
        let mut compares: Vec<(usize, std::path::PathBuf, String, Option<String>)> = Vec::new();
        let root = root.to_path_buf();
        // The three questions a build can raise are collected here and asked at
        // the end, which is why this is a block and not a straight run: every one
        // of them wants `&mut self`, and the build in the middle of it is holding
        // the renderer.
        let (asking, built) = {
            // **The tab whose plane holds this document**, which for a seat is
            // the tab on screen and for a window is the tab it was opened in
            // (§7.1.3). Reading `active` here would have a floating graph change
            // repositories under the reader the moment they switched tabs.
            let index = self.preview_tab_index(surface);
            // The state may not exist yet on the very first frame after a
            // restore put a graph buffer back on a seat; making it here is the
            // same "vivify and ask" the columns do.
            self.window.tabs[index]
                .git_graphs
                .entry(root.clone())
                .or_insert_with_key(|root| git_graph::GraphState::new(root.clone()));
            let view = self.window.tabs[index]
                .git_graph_view
                .entry(surface)
                .or_default();
            if view.root != root {
                *view = GraphView {
                    root: root.clone(),
                    ..GraphView::default()
                };
            }
            let (scroll, expanded, compare, selected, hold) = (
                view.scroll_px,
                view.expanded.clone(),
                view.compare.clone(),
                view.selected,
                view.lane_hold,
            );
            let filter = view.filter.clone();
            let typed = view.search.text().to_owned();
            let before_caret = view.search.before_caret().to_owned();
            let preedit = view.search.preedit().to_owned();
            let search_focused = view.search_focused;
            let search_at = view.search_at;
            let asked = view.search_asked.clone();
            let state = self.window.tabs[index].git_graphs.get(&root)?.clone();
            // What git said about the query that was actually *asked*, which is
            // not necessarily what is in the field: a reader typing on past a
            // result keeps seeing the result they pressed Enter for until they
            // press it again, which is what every search field does.
            let matches = asked
                .as_deref()
                .and_then(|query| state.cache.search(query))
                .and_then(git::GitSlot::ready)
                .map(Vec::as_slice);
            let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
            let mut measure = |text: &str, size: f32, face: git_panel::MeasureFace| {
                renderer.measure_chrome_label(
                    gpu,
                    text,
                    size,
                    face.weight,
                    face.letter_spacing_em,
                    face.tabular_numerals,
                )
            };
            let mut content = git_graph::build(
                &state,
                git_graph::GraphLook {
                    filter: &filter,
                    search: git_graph::GraphSearchLook {
                        text: &typed,
                        before_caret: &before_caret,
                        preedit: &preedit,
                        focused: search_focused,
                        matches,
                        at: search_at,
                    },
                    expanded: expanded.as_deref(),
                    compare: compare.as_deref(),
                    selected,
                },
                body,
                scroll,
                hold,
                scale,
                &mut measure,
            );
            // R2 乙案 again: the painter believes the stored scroll, so a list
            // that got shorter — a checkout that replaced the history — is
            // healed here rather than by the layout disagreeing with the number.
            let healed = git_graph::clamp_graph_scroll(body, &content, content.scroll_px, scale);
            content.scroll_px = healed;
            // The selection gets the same healing and for the same reason: a
            // list that got shorter under a stored row number — a checkout that
            // replaced the history, an accordion folded from elsewhere — would
            // otherwise leave the selected ground on a row that is not there.
            let selected = selected.filter(|row| *row < content.total_rows);
            content.selected = selected;
            if content.wants_more {
                extend.push((index, root.clone()));
            }
            let view = self.window.tabs[index]
                .git_graph_view
                .entry(surface)
                .or_default();
            view.scroll_px = healed;
            view.selected = selected;
            view.lane_hold = git_graph::LaneWidthHold {
                width: content.lane_width,
                until: content.lane_width_until,
            };
            // **The comparison's file list is asked from the derivation** (D6)
            // and not from the gesture that started it, because the pair can
            // become answerable without anything being pressed: the far end may
            // only arrive with a later page. What keeps it to one subprocess per
            // pair is `GitCache::begin_compare_files`'s own guard, which is
            // where "have I asked this already" belongs — beside the answer.
            if let Some((a, b)) = content.compare_pair.clone() {
                compares.push((index, root.clone(), a, b));
            }
            (index, content)
        };
        for (index, root) in extend {
            self.extend_graph(index, &root);
        }
        for (index, root, a, b) in compares {
            self.ask_graph_compare(index, &root, &a, b.as_deref());
        }
        self.ask_git_for_graphs(&[asking]);
        Some(built)
    }

    /// A graph row's body, pressed — one click.
    ///
    /// **On whichever surface is showing the graph.** A pane and a torn-off
    /// window are one document in two hosts (§7.1.3), so the verbs are one set
    /// of verbs addressed by [`PreviewSurface`] rather than two written twice.
    pub(in crate::runtime) fn press_graph_row(
        &mut self,
        surface: PreviewSurface,
        index: usize,
    ) -> Result<()> {
        let tab = self.preview_tab_index(surface);
        let Some(root) = self.window.tabs[tab]
            .git_graph_view
            .get(&surface)
            .map(|view| view.root.clone())
        else {
            return Ok(());
        };
        let Some(row) = self
            .window
            .git_graphs_shown
            .get(&surface)
            .and_then(|page| page.rows.iter().find(|row| row.index() == index))
            .cloned()
        else {
            return Ok(());
        };
        // **`Ctrl` is the compare gesture** (D6), and it is answered before
        // anything else this press could mean: with a row already open, a
        // `Ctrl`+click is a claim about *two* rows and never about the one under
        // the pointer, so it neither opens a document nor turns an accordion
        // over. Without a row open it is not a comparison at all and falls
        // through to the ordinary press, because there is no first end for it to
        // be the second end of.
        //
        // **`⌘` is the compare gesture on a Mac** (§13.45 ①), through the one
        // function that knows which key hands a pointer gesture over. Control
        // could not stay: it is the secondary click on that desk, so the same
        // press would have been asking for a context menu.
        if input::pointer_chord_held(self.window.modifiers_held) {
            let hash = match &row {
                git_graph::GraphViewRow::Uncommitted(_) => {
                    Some(git_graph::GRAPH_UNCOMMITTED_HASH.to_owned())
                }
                git_graph::GraphViewRow::Commit(commit) => Some(commit.hash.clone()),
                git_graph::GraphViewRow::File(_) | git_graph::GraphViewRow::Detail(_) => None,
            };
            if let Some(hash) = hash
                && self.set_graph_compare(surface, &hash)
            {
                self.select_graph_row(surface, index);
                if self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
                return Ok(());
            }
        }
        // **A graph row has no second-click verb**, so no pair of clicks is
        // synthesised here any more — see [`git_graph`]'s note where
        // `row_double_open` used to stand. The one verb it answered was a
        // detached checkout, and a checkout is not something this page may do
        // because a pointer was in one place twice.
        //
        // **A plain press leaves compare mode** (D6). The gesture that entered it
        // said "these two"; an unmodified click says "this one", and a page that
        // went on holding two rows lit after that would be remembering a question
        // the reader has stopped asking.
        self.clear_graph_compare(surface);
        // **A press is a selection** (V8), whatever the press then goes on to do:
        // the keyboard walks from where the pointer last was, which is what makes
        // clicking a row and then pressing `↓` mean the row under it.
        self.select_graph_row(surface, index);
        let Some(open) = git_graph::row_open(&row, &root) else {
            return Ok(());
        };
        match open {
            git_panel::GitRowOpen::Document {
                source,
                name,
                renamed_from,
            } => self.open_git_document_for_graph(surface, &root, source, name, renamed_from),
            git_panel::GitRowOpen::Expand { hash } => {
                self.expand_graph_commit(surface, &root, &hash)
            }
            // No row of a graph answers a press with a checkout — the branch
            // rows that do live on the docked page, and this arm is here because
            // the two surfaces share one `GitRowOpen`.
            git_panel::GitRowOpen::Checkout { .. } => Ok(()),
        }
    }

    /// Point the far end of a comparison at this row (D6).
    ///
    /// Answers whether the gesture meant anything here, which is the whole of
    /// the modifier's fall-through: without a row open there is no first end, so
    /// `Ctrl`+click is not a comparison and the press goes on to mean what it
    /// would have meant anyway.
    ///
    /// **The far end may not be the open row itself.** The ticket's own wording
    /// lumps "the expanded row" in with "a third row" under *moves the second
    /// endpoint*, and taken literally that is a commit compared with itself —
    /// which is not a comparison, and whose file list is empty by construction.
    /// So the honest reading of that gesture is the other one it could have:
    /// **stop comparing**, keeping the row open. Which is also what a second
    /// `Ctrl`+click on the row already at the far end means, for the reason every
    /// toggle in this window works that way.
    fn set_graph_compare(&mut self, surface: PreviewSurface, hash: &str) -> bool {
        let tab = self.preview_tab_index(surface);
        let Some(view) = self.window.tabs[tab].git_graph_view.get_mut(&surface) else {
            return false;
        };
        let Some(expanded) = view.expanded.clone() else {
            return false;
        };
        view.compare = if expanded == hash || view.compare.as_deref() == Some(hash) {
            None
        } else {
            Some(hash.to_owned())
        };
        true
    }

    /// Stop comparing, keeping the open row open.
    fn clear_graph_compare(&mut self, surface: PreviewSurface) -> bool {
        let tab = self.preview_tab_index(surface);
        self.window.tabs[tab]
            .git_graph_view
            .get_mut(&surface)
            .is_some_and(|view| view.compare.take().is_some())
    }

    /// One of the detail block's own parts, pressed (D2/D6/D7).
    pub(in crate::runtime) fn press_graph_detail(
        &mut self,
        surface: PreviewSurface,
        part: git_graph::GraphDetailPart,
    ) -> Result<()> {
        let Some(detail) = self
            .window
            .git_graphs_shown
            .get(&surface)
            .and_then(|content| {
                content.rows.iter().find_map(|row| match row {
                    git_graph::GraphViewRow::Detail(detail) => Some(detail.clone()),
                    _ => None,
                })
            })
        else {
            return Ok(());
        };
        match (part, &detail.detail) {
            // **A parent is a place to go** (D2). It is looked for in what is
            // loaded first, and only a miss starts a seek — the overwhelmingly
            // common case is a parent one row down.
            (git_graph::GraphDetailPart::Parent(at), git_graph::GraphDetail::Commit(commit)) => {
                let Some(parent) = commit.parents.get(at) else {
                    return Ok(());
                };
                self.seek_graph_commit(surface, parent.hash.clone())?;
            }
            (git_graph::GraphDetailPart::CopyHash, git_graph::GraphDetail::Commit(commit)) => {
                self.copy_from_graph(surface, &commit.hash, &commit.short)?;
            }
            (git_graph::GraphDetailPart::CopySubject, git_graph::GraphDetail::Commit(commit)) => {
                self.copy_from_graph(surface, &commit.subject, &commit.subject)?;
            }
            (git_graph::GraphDetailPart::LeaveCompare, _) => {
                if self.clear_graph_compare(surface) && self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Put one of a commit's facts on the clipboard, and say so (D7).
    ///
    /// **The first `Ok` notice this window raises.** The kind has been drawn,
    /// tested and legible on both canvases since the toast host was built and
    /// had no caller, which was deliberate — a host that could only carry
    /// failures would be a failure host — and this is the call site it was
    /// waiting for: a copy is invisible, and a verb whose whole effect is
    /// somewhere the reader cannot see has to say that it happened.
    fn copy_from_graph(&mut self, surface: PreviewSurface, text: &str, said: &str) -> Result<()> {
        let result = hang_watch::during(hang_watch::Station::ClipboardWrite, || {
            bt_platform::set_clipboard_text(text)
        })
        .map_err(|error| anyhow!(error))
        .context("copy a commit's own words to the clipboard");
        if !recoverable_clipboard_write(result, "commit copy") {
            return Ok(());
        }
        let anchor = surface.toast_anchor();
        self.toast(
            toast::ToastKind::Ok,
            anchor,
            None,
            git_graph::graph_copied(said),
        )
    }

    /// Go to a commit, paging until it turns up if it has to (D2).
    fn seek_graph_commit(&mut self, surface: PreviewSurface, hash: String) -> Result<()> {
        let tab = self.preview_tab_index(surface);
        if let Some(view) = self.window.tabs[tab].git_graph_view.get_mut(&surface) {
            view.seek = Some(git_graph::GraphSeek { hash, pages: 0 });
        }
        if self.step_graph_seeks()? && self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Put the selected ground on one row of a graph (V8).
    ///
    /// Written whether or not it moved: the caller is a gesture that *means* this
    /// row, and a press on the row already selected is still that.
    fn select_graph_row(&mut self, surface: PreviewSurface, index: usize) {
        let tab = self.preview_tab_index(surface);
        let view = self.window.tabs[tab]
            .git_graph_view
            .entry(surface)
            .or_default();
        view.selected = Some(index);
    }

    /// Turn one commit's file list over, in the graph (R15) — or the working
    /// tree's (V5).
    fn expand_graph_commit(
        &mut self,
        surface: PreviewSurface,
        root: &Path,
        hash: &str,
    ) -> Result<()> {
        let tab = self.preview_tab_index(surface);
        let tab_id = self.window.tabs[tab].id;
        let opened = {
            let view = self.window.tabs[tab]
                .git_graph_view
                .entry(surface)
                .or_default();
            let opened = git_graph::toggled_expansion(view.expanded.as_deref(), hash);
            view.expanded.clone_from(&opened);
            // **A comparison belongs to the row it was started from** (D6/D9), so
            // turning another row over ends it: the pair was two rows and one of
            // them is no longer the open one.
            view.compare = None;
            opened
        };
        // **The working tree's row asks git nothing** (V5). Its files are the
        // status this cache already holds — the one answer every role has asked
        // for since G-1 — so unfolding it spends no subprocess at all, which is
        // what let the row exist at all under R31's two-condition gate.
        if let Some(hash) = opened
            && hash != git_graph::GRAPH_UNCOMMITTED_HASH
            && let Some(state) = self.window.tabs[tab].git_graphs.get_mut(root)
            && let Some(question) = state.cache.begin_commit_files(&hash)
            && !self.app.git_worker.request(git::GitRequest {
                window: self.window_id(),
                host: git::GitHost::Graph {
                    tab: tab_id,
                    root: root.to_owned(),
                },
                question,
            })
        {
            self.disable_git_worker();
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// One key, with a commit graph holding the keyboard (V14).
    ///
    /// Answers whether the key was the graph's *verb*. `false` is not "give it to
    /// the shell" — a focused preview owns every key it is offered — it is "this
    /// key is not one of the six, so the surface under this one may have it",
    /// which for `Esc` with nothing open is the whole point: the float dismissal
    /// and, under that, `vim` are entitled to an `Esc` this page has no use for.
    pub(in crate::runtime) fn graph_key(
        &mut self,
        surface: PreviewSurface,
        key: git_graph::GraphKey,
    ) -> Result<bool> {
        let Some(content) = self.window.git_graphs_shown.get(&surface).cloned() else {
            return Ok(false);
        };
        let tab = self.preview_tab_index(surface);
        let Some(root) = self.window.tabs[tab]
            .git_graph_view
            .get(&surface)
            .map(|view| view.root.clone())
        else {
            return Ok(false);
        };
        let action = git_graph::graph_key(&content, key);
        let selected = match action {
            git_graph::GraphKeyAction::Pass => return Ok(false),
            git_graph::GraphKeyAction::None => return Ok(true),
            git_graph::GraphKeyAction::Select(row) => row,
            git_graph::GraphKeyAction::Toggle(row) => {
                // Enter is the row's own press, minus the pointer: whatever a
                // click on it would have done, through the same door — so a file
                // row opens its document and a commit row turns over, and neither
                // gesture had to be written twice.
                let Some(drawn) = content
                    .rows
                    .iter()
                    .find(|drawn| drawn.index() == row)
                    .cloned()
                else {
                    // **A wheel can carry the selection off the window** — the
                    // arrows always scroll it back, but a notch does not move it
                    // — and R23 only builds what is on screen. Rather than act on
                    // a row nobody can see, bring it back and let the next press
                    // mean what it says.
                    self.select_graph_row(surface, row);
                    self.reveal_graph_row(surface, row);
                    if self.refresh_chrome() {
                        self.present_chrome_change()?;
                    }
                    return Ok(true);
                };
                self.select_graph_row(surface, drawn.index());
                if let Some(open) = git_graph::row_open(&drawn, &root) {
                    match open {
                        git_panel::GitRowOpen::Document {
                            source,
                            name,
                            renamed_from,
                        } => self.open_git_document_for_graph(
                            surface,
                            &root,
                            source,
                            name,
                            renamed_from,
                        )?,
                        git_panel::GitRowOpen::Expand { hash } => {
                            self.expand_graph_commit(surface, &root, &hash)?;
                        }
                        git_panel::GitRowOpen::Checkout { .. } => {}
                    }
                }
                return Ok(true);
            }
            // `Ctrl+Enter` is `Ctrl`+click without the pointer, through the same
            // door — so the ruling about what the far end may be is written once.
            git_graph::GraphKeyAction::Compare(row) => {
                let hash = content.rows.iter().find_map(|drawn| match drawn {
                    git_graph::GraphViewRow::Uncommitted(head) if head.index == row => {
                        Some(git_graph::GRAPH_UNCOMMITTED_HASH.to_owned())
                    }
                    git_graph::GraphViewRow::Commit(commit) if commit.index == row => {
                        Some(commit.hash.clone())
                    }
                    _ => None,
                });
                if let Some(hash) = hash {
                    self.set_graph_compare(surface, &hash);
                }
                row
            }
            git_graph::GraphKeyAction::LeaveCompare => {
                self.clear_graph_compare(surface);
                if self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
                return Ok(true);
            }
            git_graph::GraphKeyAction::Collapse(row) => {
                let hash = self.window.tabs[tab]
                    .git_graph_view
                    .get(&surface)
                    .and_then(|view| view.expanded.clone());
                if let Some(hash) = hash {
                    self.expand_graph_commit(surface, &root, &hash)?;
                }
                // Standing on what was just folded shut, and not on wherever the
                // selection had wandered to inside it: the rows it was in have
                // gone, and the row it came out of is the one still on screen.
                row
            }
        };
        self.select_graph_row(surface, selected);
        self.reveal_graph_row(surface, selected);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// Scroll a graph until one of its rows is whole on screen (V14).
    fn reveal_graph_row(&mut self, surface: PreviewSurface, index: usize) {
        let scale = self.window.renderer.scale_factor() as f32;
        let Some(body) = self.preview_surface_body_rect(surface, scale) else {
            return;
        };
        let Some(content) = self.window.git_graphs_shown.get(&surface) else {
            return;
        };
        let wanted = git_graph::graph_geometry(body, content, scale).reveal(index);
        let tab = self.preview_tab_index(surface);
        if let Some(view) = self.window.tabs[tab].git_graph_view.get_mut(&surface) {
            view.scroll_px = wanted;
        }
    }

    /// A document opened from the graph rather than from a column.
    ///
    /// The same two steps [`Self::open_git_document`] takes; what differs is
    /// only which cache the question is addressed to, because the answer has to
    /// come home to the graph's host.
    ///
    /// `from` is the surface the row was pressed on, and it is here for one
    /// fact: which tab's graph cache the answer must come home to. A pane's is
    /// the tab on screen; a torn-off window's is the tab it was opened in
    /// (§7.1.3), and addressing the question to whichever tab happened to be in
    /// front would file a floating graph's diff in a cache the window never
    /// reads.
    fn open_git_document_for_graph(
        &mut self,
        from: PreviewSurface,
        root: &Path,
        source: preview::PreviewSource,
        name: String,
        renamed_from: Option<String>,
    ) -> Result<()> {
        let Some(landing) = self.preview_landing_surface() else {
            return Ok(());
        };
        self.open_preview_source_on(landing, source.clone(), name)?;
        let unread = self
            .preview_pool
            .get(&source)
            .is_some_and(|buffer| buffer.load == preview::PreviewLoad::Pending);
        if !unread {
            return Ok(());
        }
        let Some(question) = git_document_question(&source, renamed_from) else {
            return Ok(());
        };
        let tab_id = self.window.tabs[self.preview_tab_index(from)].id;
        if !self.app.git_worker.request(git::GitRequest {
            window: self.window_id(),
            host: git::GitHost::Graph {
                tab: tab_id,
                root: root.to_owned(),
            },
            question,
        }) {
            self.disable_git_worker();
        }
        Ok(())
    }

    /// **Stand somewhere else** (R10) — from a branch row.
    fn checkout_from_column(&mut self, seat: SeatId, target: String, detach: bool) -> Result<()> {
        let active = self.window.active_tab;
        let tab_id = self.window.tabs[active].id;
        let Some(cache) = self.window.tabs[active].git_trees.get_mut(&seat) else {
            return Ok(());
        };
        let Some(question) = cache.begin_checkout(target, detach) else {
            return Ok(());
        };
        if !self.app.git_worker.request(git::GitRequest {
            window: self.window_id(),
            host: git::GitHost::Column(LeafId { tab: tab_id, seat }),
            question,
        }) {
            self.disable_git_worker();
            return Ok(());
        }
        // The rows dim now, so the frame that shows them dimmed is this one.
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        Ok(())
    }

    /// The same, from a floating Git page (user ruling, 2026-08-19) — the
    /// column's own three steps, into the window's own cache and out under the
    /// window's own host.
    fn checkout_from_float(
        &mut self,
        id: float::FloatId,
        target: String,
        detach: bool,
    ) -> Result<()> {
        let tab = self.window.tabs[self.window.active_tab].id;
        let Some(question) = self
            .window
            .float
            .live_mut(id)
            .and_then(float::FloatWin::files_mut)
            .and_then(|files| files.git.begin_checkout(target, detach))
        else {
            return Ok(());
        };
        if !self.app.git_worker.request(git::GitRequest {
            window: self.window_id(),
            host: git::GitHost::Float { id, tab },
            question,
        }) {
            self.disable_git_worker();
            return Ok(());
        }
        // The rows dim now, so the frame that shows them dimmed is this one.
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        Ok(())
    }

    /// The same, from the graph — which is a document keyed by root and belongs
    /// to no column, so it names the repository rather than a seat.
    ///
    /// **Every column on this repository is invalidated too**, and that is not
    /// tidiness: after this the branch head, the status and the history are
    /// about a different place, and a panel still drawing the old branch beside
    /// a graph drawing the new one is the one disagreement this whole subsystem
    /// was built to prevent.
    fn checkout_in_graph(&mut self, root: &Path, target: String, detach: bool) -> Result<()> {
        let active = self.window.active_tab;
        let tab_id = self.window.tabs[active].id;
        let Some(state) = self.window.tabs[active].git_graphs.get_mut(root) else {
            return Ok(());
        };
        let Some(question) = state.cache.begin_checkout(target, detach) else {
            return Ok(());
        };
        if !self.app.git_worker.request(git::GitRequest {
            window: self.window_id(),
            host: git::GitHost::Graph {
                tab: tab_id,
                root: root.to_owned(),
            },
            question,
        }) {
            self.disable_git_worker();
            return Ok(());
        }
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        Ok(())
    }

    /// A notch over a graph (R23's virtual list scrolls like every other list).
    pub(in crate::runtime) fn scroll_git_graph(
        &mut self,
        surface: PreviewSurface,
        body: [f32; 4],
        delta: MouseScrollDelta,
    ) -> Result<()> {
        let tab = self.preview_tab_index(surface);
        let Some(content) = self.window.git_graphs_shown.get(&surface) else {
            return Ok(());
        };
        let scale = self.window.renderer.scale_factor() as f32;
        let extent = (content.total_rows.max(1)) as f32;
        let travel = self.vertical_wheel_travel(delta, extent);
        let stored = self.window.tabs[tab]
            .git_graph_view
            .get(&surface)
            .map_or(0.0, |view| view.scroll_px);
        let wanted = git_graph::clamp_graph_scroll(body, content, stored - travel, scale);
        if (wanted - stored).abs() < f32::EPSILON {
            return Ok(());
        }
        if let Some(view) = self.window.tabs[tab].git_graph_view.get_mut(&surface) {
            view.scroll_px = wanted;
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Whether a Files column offers its Git page at all (the master switch).
    ///
    /// One reader, called by the paint, the hit test, the wheel and the chord,
    /// for the reason [`Self::default_profile`] is one reader: a setting consulted
    /// in four places is four places to consult it differently, and the symptom
    /// of that here would be a strip you can press but cannot see.
    pub(in crate::runtime) fn git_panel_on(&self) -> bool {
        self.app.settings_store.loaded().git_panel
    }

    /// The Git page each column showing one draws this frame.
    ///
    /// Built from the cache and thrown away, like every other content map here:
    /// the page holds no state of its own, so a frame is a pure function of what
    /// the repository last said.
    pub(in crate::runtime) fn git_pages(
        &mut self,
        scale: f32,
        views: &BTreeMap<SeatId, seats::FilesViewContent>,
    ) -> BTreeMap<SeatId, git_panel::GitPanelContent> {
        let showing: Vec<SeatId> = views
            .iter()
            .filter(|(_, content)| content.view == seats::FilesView::Git)
            .map(|(seat, _)| *seat)
            .collect();
        if showing.is_empty() {
            return BTreeMap::new();
        }
        let active = self.window.active_tab;
        let mut pages = BTreeMap::new();
        for seat in showing {
            // A column that has never had a Git page has no cache yet, and the
            // page it draws for one frame is "reading" — which is true: the ask
            // rides the same walk and will have gone out by the time this frame
            // is on screen.
            let Some(cache) = self.window.tabs[active].git_trees.get(&seat).cloned() else {
                pages.insert(seat, git_panel::GitPanelContent::default());
                continue;
            };
            // Which commit is open is the *column's* (R15), so it is read off
            // the leaf beside the cache rather than out of it — the cache holds
            // the answer, this decides whether it is on screen.
            let expanded = self.window.tabs[active]
                .files
                .get(&seat)
                .and_then(|state| state.git_expanded.clone());
            // And so is whether the REMOTES sub-group is unfolded (T9) — the
            // same kind of fact, off the same leaf, and durable like `view`.
            let remotes_open = self.window.tabs[active]
                .files
                .get(&seat)
                .is_some_and(|state| state.git_remotes_open);
            // **The column this page's one sentence is wrapped to**, read before
            // the measurer is built because that closure takes the renderer with
            // it. A seat with no rectangle this frame gets a zero column, which
            // is the honest reading — there is nothing to wrap into, and the page
            // is not being drawn either.
            let column = seats::files_pane_rect(&self.seat_layout, seat)
                .map(|rect| seats::files_pane_geometry(rect, scale, true).body)
                .map_or(0.0, |body| git_panel::empty_width(body, scale));
            let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
            let mut measure = |text: &str, size: f32, face: git_panel::MeasureFace| {
                renderer.measure_chrome_label(
                    gpu,
                    text,
                    size,
                    face.weight,
                    face.letter_spacing_em,
                    face.tabular_numerals,
                )
            };
            let mut content = git_panel::build(
                &cache,
                git_panel::GitPanelLook {
                    expanded: expanded.as_deref(),
                    remotes_open,
                },
                scale,
                column,
                &mut measure,
            );
            content.scroll_px = self.window.tabs[active]
                .git_scroll
                .get(&seat)
                .copied()
                .unwrap_or(0.0);
            // **Where this page's keyboard is standing**, clamped to the list
            // that was just built (焦点跟随可见视图). The column remembers a row
            // number and the repository rebuilds the list under it every frame,
            // so the clamp belongs here — beside the scroll's, which is owed for
            // exactly the same reason. The clamp is `git_panel`'s own, so a
            // number that has landed on the page's furniture is moved to a row
            // rather than lighting a heading (user report, 2026-08-25).
            content.selected = git_panel::clamp_git_selection(
                &content.rows,
                self.window.tabs[active]
                    .files
                    .get(&seat)
                    .and_then(|state| state.git_sel),
            );
            pages.insert(seat, content);
        }
        // R2 乙案 again, one list along: the painter believes the stored scroll,
        // so a page that got shorter — a group emptied by a `git add` — is
        // answered here rather than by the layout disagreeing with the number.
        for (seat, content) in &mut pages {
            let Some(rect) = seats::files_pane_rect(&self.seat_layout, *seat) else {
                continue;
            };
            let body = seats::files_pane_geometry(rect, scale, true).body;
            let healed = git_panel::clamp_git_scroll(body, content, content.scroll_px, scale);
            content.scroll_px = healed;
            self.window.tabs[active].git_scroll.insert(*seat, healed);
        }
        self.settle_git_pending(&mut pages, Instant::now());
        pages
    }

    /// **A row waiting on a write dims into it rather than jumping** (the
    /// animation slice's second half, `bt_render::GIT_PENDING_FADE_SPAN`).
    ///
    /// `git_panel` has already filled each row's `fade` with the still picture —
    /// [`git_panel::GIT_PENDING_FADE`] or full strength — and this eases between
    /// them. Which means the panel keeps the whole of the *state* and this owns
    /// only the ninety milliseconds: under reduced motion the register answers
    /// the target on the first frame and every row is exactly as dim as it always
    /// was.
    ///
    /// **A row that has left the list is forgotten rather than eased.** Staging a
    /// file moves it out of `CHANGES` and into `STAGED` — two different rows with
    /// two different keys — so the row that was dimming is not coming back, and
    /// there is nothing left on the glass for a fade home to be drawn on.
    fn settle_git_pending(
        &mut self,
        pages: &mut BTreeMap<SeatId, git_panel::GitPanelContent>,
        now: Instant,
    ) {
        let motion = self.app.motion;
        let mut asked: Vec<Fading> = Vec::new();
        for (seat, content) in pages.iter_mut() {
            for row in &mut content.rows {
                let (key, fade) = match row {
                    git_panel::GitRow::Change(change) => (
                        Fading::GitPending(
                            *seat,
                            GitRowKey::Change(change.group, change.path.clone()),
                        ),
                        &mut change.fade,
                    ),
                    git_panel::GitRow::Branch(branch) => (
                        Fading::GitPending(*seat, GitRowKey::Branch(branch.name.clone())),
                        &mut branch.fade,
                    ),
                    _ => continue,
                };
                // Full strength is home: a row is bright unless something is
                // being done to it, which is the sentence `GIT_PENDING_FADE`
                // makes about the repository read back as a resting value.
                *fade = self.window.settling.settle(
                    &key,
                    1.0,
                    settling::Toward::eased(*fade, bt_render::GIT_PENDING_FADE_SPAN),
                    now,
                    motion,
                );
                asked.push(key);
            }
        }
        for key in self.window.settling.held() {
            if matches!(key, Fading::GitPending(..)) && !asked.contains(&key) {
                self.window.settling.forget(&key);
            }
        }
    }

    /// The same, for a row of a column's Git page — the *page's* answer
    /// ([`git_panel::row_peek`]) with this column's repository put in front of
    /// it, which is all this window contributes.
    pub(in crate::runtime) fn git_peek_row(
        &self,
        seat: SeatId,
        index: usize,
    ) -> Option<(String, String, preview::PreviewSource)> {
        let root = self.git_trees.get(&seat).and_then(git::GitCache::root)?;
        let row = self.window.git_pages_shown.get(&seat)?.rows.get(index)?;
        let peek = git_panel::row_peek(row, root)?;
        Some((peek.key, peek.name, peek.source))
    }

    /// **Commit whatever this window still has open, and take its picture**
    /// (slice E2 phase ②).
    ///
    /// Read-only about everything except the editor, and the editor is §7.1.4's
    /// standing rule: "未提交的重命名在序列化前提交（blur 语义）". Before the
    /// snapshot and not after it, which is where `close_window` puts it and for
    /// the same reason — the name has to be on the tab by the time the tab is
    /// written down. By the time this runs the card's answer has already settled
    /// any editor it named, so what is left here is the draft nobody was asked
    /// about, which blur has always committed.
    ///
    /// **Nothing is torn down.** That is the whole of what separates this from
    /// `close_window`, and it is acceptance gate 2.
    pub(crate) fn photograph_for_quit(&mut self) -> Result<()> {
        self.finish_rename(RenameExit::Blur)?;
        self.mark_session_dirty(Instant::now());
        Ok(())
    }

    /// The menu's own state, copied out of `self` so the renderer can be
    /// borrowed to measure with.
    ///
    /// One clone per frame the menu is up, and the alternative is the borrow
    /// checker's own problem written into the design: [`profiles::GitMenuLook`]
    /// holds references into the state, and the measure closure needs the
    /// renderer mutably. The file menu solves the same thing the same way, one
    /// field at a time; this one has six, so they travel together.
    fn git_menu_draw(&self) -> Option<GitMenuDraw> {
        let menu = self.window.git_menu.as_ref()?;
        Some(GitMenuDraw {
            point: menu.point,
            target: menu.target.clone(),
            hover: menu.hover,
            prompt: menu.prompt.as_ref().map(|prompt| GitPromptDraw {
                kind: prompt.kind,
                caption: prompt.caption.clone(),
                // **The composition opens a space at the caret** and the caret
                // stands after it — T4's rule, kept here so a name typed with an
                // IME reads the way a query typed with one does.
                text: format!(
                    "{}{}{}",
                    prompt.field.before_caret(),
                    prompt.field.preedit(),
                    &prompt.field.text()[prompt
                        .field
                        .before_caret()
                        .len()
                        .min(prompt.field.text().len())..]
                ),
                before_caret: prompt.field.before_caret().to_owned(),
                preedit: prompt.field.preedit().to_owned(),
                fault: prompt.fault(),
            }),
        })
    }

    /// Where the git context menu is, if one is up.
    ///
    /// It cannot fold for want of an anchor — the anchor is the point the
    /// pointer was at — which is [`Runtime::file_menu_layout`]'s ruling and
    /// doubly load-bearing here: the row this menu is about lives in a list that
    /// pages, scrolls and is rebuilt by every repository answer.
    pub(in crate::runtime) fn git_menu_layout(&mut self) -> Option<profiles::GitMenuLayout> {
        let draw = self.git_menu_draw()?;
        let scale = self.window.renderer.scale_factor() as f32;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        Some(profiles::git_menu_layout(
            draw.point,
            (width as f32, height as f32),
            scale,
            &draw.look(),
            &mut measure,
        ))
    }

    /// The git menu's own level of the overlay stack.
    pub(in crate::runtime) fn git_menu_layer(&mut self) -> MenuPaint {
        let Some(draw) = self.git_menu_draw() else {
            return MenuPaint::none();
        };
        let Some(layout) = self.git_menu_layout() else {
            return MenuPaint::none();
        };
        let travel = layout.travel();
        MenuPaint::plain(profiles::git_menu_build(&layout, &draw.look()), travel)
    }

    pub(in crate::runtime) fn close_git_menu(&mut self) -> Result<bool> {
        if self.window.git_menu.take().is_none() {
            return Ok(false);
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// **What a right press landed on**, resolved to a menu's worth of facts.
    ///
    /// `None` for everything that has no repository verb — a heading, the
    /// masthead, the "load more" row, a detail block, a graph file row — and for
    /// the working tree's row when there is nothing open to compare it against.
    /// A `None` here is what makes the press fall through and mean whatever it
    /// meant before this slice existed.
    fn git_menu_target_at(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Option<(GitOrigin, PathBuf, profiles::GitMenuTarget, Option<usize>)> {
        let active = self.window.active_tab;
        match self.chrome_target_at(position)? {
            // A right press on a hover verb is a right press on the row it sits
            // in. The pane head's own menu takes the same position for the same
            // reason: an 18-pixel hole in a row that silently means something
            // else is exactly the edge a hand finds by accident.
            seats::ChromeTarget::GitRow { seat, index }
            | seats::ChromeTarget::GitAct { seat, index, .. } => {
                let root = self.window.tabs[active]
                    .git_trees
                    .get(&seat)
                    .and_then(git::GitCache::root)
                    .map(Path::to_path_buf)?;
                let row = self.window.git_pages_shown.get(&seat)?.rows.get(index)?;
                let target = match row {
                    git_panel::GitRow::Change(change) => profiles::GitMenuTarget::Change {
                        path: change.path.clone(),
                        group: change.group,
                        untracked: change.untracked,
                        renamed_from: change.renamed_from.clone(),
                    },
                    git_panel::GitRow::Branch(branch) if branch.remote => {
                        profiles::GitMenuTarget::Remote {
                            name: branch.name.clone(),
                        }
                    }
                    git_panel::GitRow::Branch(branch) => profiles::GitMenuTarget::LocalBranch {
                        name: branch.name.clone(),
                        current: branch.current,
                    },
                    // **The panel offers no comparison** (D6 is the graph's).
                    // Compare mode is a state of the *graph's* view — two rows
                    // lit, a block between them, a file list under it — and a
                    // 240-pixel column has none of that furniture. A row that
                    // entered a mode the surface cannot draw would be a verb
                    // that appears to do nothing.
                    git_panel::GitRow::Commit(commit) => profiles::GitMenuTarget::Commit {
                        hash: commit.hash.clone(),
                        short: commit.short.clone(),
                        subject: commit.subject.clone(),
                        can_compare: false,
                        compare_ready: false,
                    },
                    _ => return None,
                };
                Some((GitOrigin::Column(seat), root, target, None))
            }
            seats::ChromeTarget::GitGraphRow { seat, index } => {
                let surface = self.preview_here(seat);
                let root = self.window.tabs[active]
                    .git_graph_view
                    .get(&surface)?
                    .root
                    .clone();
                let expanded = self.window.tabs[active]
                    .git_graph_view
                    .get(&surface)
                    .and_then(|view| view.expanded.clone());
                let content = self.window.git_graphs_shown.get(&surface)?;
                let row = content.rows.iter().find(|row| row.index() == index)?;
                let target = match row {
                    git_graph::GraphViewRow::Commit(commit) => {
                        // **A pill is more specific than the row it is on.** The
                        // rectangles come from the same run the painter draws
                        // from, so a pill you can see is a pill you can press.
                        let pill = self.graph_row_rect(surface, index).and_then(|rect| {
                            git_graph::graph_ref_pill_at(
                                commit,
                                rect,
                                content.lane_width,
                                content.columns,
                                self.window.renderer.scale_factor() as f32,
                                position.x as f32,
                                position.y as f32,
                            )
                            .and_then(|at| commit.refs.get(at))
                        });
                        match pill {
                            Some(pill) => match pill.kind {
                                git::GitRefKind::Local => profiles::GitMenuTarget::LocalBranch {
                                    name: pill.name.clone(),
                                    current: pill.head,
                                },
                                git::GitRefKind::Remote => profiles::GitMenuTarget::Remote {
                                    name: pill.name.clone(),
                                },
                                git::GitRefKind::Tag => profiles::GitMenuTarget::Tag {
                                    name: pill.name.clone(),
                                },
                            },
                            None => profiles::GitMenuTarget::Commit {
                                hash: commit.hash.clone(),
                                short: commit.short.clone(),
                                subject: commit.subject.clone(),
                                can_compare: true,
                                // The near end of a comparison is the *open*
                                // row, and a commit is never the far end of a
                                // comparison with itself.
                                compare_ready: expanded
                                    .as_deref()
                                    .is_some_and(|open| open != commit.hash),
                            },
                        }
                    }
                    git_graph::GraphViewRow::Uncommitted(_) => {
                        profiles::GitMenuTarget::Uncommitted {
                            // A commit has to be open for there to be anything
                            // to compare the working tree with — and the working
                            // tree being open is not that.
                            compare_ready: expanded
                                .as_deref()
                                .is_some_and(|open| open != git_graph::GRAPH_UNCOMMITTED_HASH),
                        }
                    }
                    git_graph::GraphViewRow::File(_) | git_graph::GraphViewRow::Detail(_) => {
                        return None;
                    }
                };
                Some((GitOrigin::Graph(root.clone()), root, target, Some(index)))
            }
            _ => None,
        }
    }

    /// Where one row of a graph is drawn, this frame.
    fn graph_row_rect(&self, surface: PreviewSurface, index: usize) -> Option<[f32; 4]> {
        let scale = self.window.renderer.scale_factor() as f32;
        let content = self.window.git_graphs_shown.get(&surface)?;
        let body = self.preview_surface_body_rect(surface, scale)?;
        Some(git_graph::graph_geometry(body, content, scale).row_rect(index))
    }

    /// Raise the menu the right press asked for, and say whether one came up.
    ///
    /// **An empty menu does not open**, which is the working tree row's whole
    /// ruling: with nothing to compare against there is no verb to offer, and a
    /// popup that opened in order to show one greyed line would be worse than
    /// the press doing nothing.
    pub(in crate::runtime) fn open_git_menu_at(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let Some((origin, root, target, graph_row)) = self.git_menu_target_at(position) else {
            return Ok(false);
        };
        if profiles::git_menu(&target).rows.is_empty() {
            return Ok(false);
        }
        // E61: the opener closes the others.
        self.close_popups_except(Popup::GitMenu);
        self.window.git_menu = Some(GitMenuState {
            point: [position.x as f32, position.y as f32],
            origin,
            root,
            target,
            graph_row,
            hover: None,
            prompt: None,
        });
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// Turn the menu into a prompt (v2 ④).
    ///
    /// The menu is not closed and not replaced: it keeps its anchor, its target
    /// and its origin, and only what is drawn inside it changes. That is what
    /// makes Esc able to put the field away and leave the verbs standing.
    fn open_git_prompt(&mut self, kind: profiles::GitPromptKind) -> Result<()> {
        let Some(menu) = self.window.git_menu.as_mut() else {
            return Ok(());
        };
        let subject = match &menu.target {
            profiles::GitMenuTarget::Commit { short, .. } => short.clone(),
            profiles::GitMenuTarget::LocalBranch { name, .. }
            | profiles::GitMenuTarget::Remote { name }
            | profiles::GitMenuTarget::Tag { name } => name.clone(),
            profiles::GitMenuTarget::Change { path, .. } => path.clone(),
            profiles::GitMenuTarget::Uncommitted { .. } => String::new(),
        };
        menu.prompt = Some(GitPromptState {
            kind,
            caption: kind.caption(&subject),
            // **A rename opens holding the name it is about**, selected, so that
            // typing replaces it and Backspace edits it — which is what every
            // rename field in this window does (J103). The other two open empty,
            // because there is no name yet to edit.
            field: match kind {
                profiles::GitPromptKind::RenameBranch => {
                    let mut field = text_field::TextField::holding(&subject);
                    field.select_all();
                    field
                }
                _ => text_field::TextField::default(),
            },
            asked_empty: false,
        });
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Carry out what the prompt was asked for, if the name is one git will take.
    ///
    /// A name that is not closes nothing and asks nothing: the red line under
    /// the field turns on and the field keeps the keyboard. **No card** — see
    /// [`profiles::GitPromptLook::fault`].
    fn commit_git_prompt(&mut self) -> Result<()> {
        let Some(menu) = self.window.git_menu.as_ref() else {
            return Ok(());
        };
        let Some(prompt) = menu.prompt.as_ref() else {
            return Ok(());
        };
        let name = prompt.field.text().to_owned();
        let kind = prompt.kind;
        if git::ref_name_fault(&name).is_some() {
            if let Some(prompt) = self
                .window
                .git_menu
                .as_mut()
                .and_then(|menu| menu.prompt.as_mut())
            {
                prompt.asked_empty = true;
            }
            if self.refresh_chrome() {
                self.present_chrome_change()?;
            }
            return Ok(());
        }
        let verb = match (kind, &menu.target) {
            (
                profiles::GitPromptKind::CreateBranch,
                profiles::GitMenuTarget::Commit { hash, .. },
            ) => git::GitWriteVerb::CreateBranch {
                name,
                at: hash.clone(),
            },
            (profiles::GitPromptKind::CreateTag, profiles::GitMenuTarget::Commit { hash, .. }) => {
                git::GitWriteVerb::CreateTag {
                    name,
                    at: hash.clone(),
                }
            }
            (
                profiles::GitPromptKind::RenameBranch,
                profiles::GitMenuTarget::LocalBranch { name: from, .. },
            ) => git::GitWriteVerb::RenameBranch {
                from: from.clone(),
                to: name,
            },
            // A prompt kind that does not match its target is a bug that has
            // already happened somewhere else; it asks the repository nothing.
            _ => return self.close_git_menu().map(|_| ()),
        };
        let origin = menu.origin.clone();
        self.close_git_menu()?;
        self.issue_git_write(&origin, verb, Vec::new())
    }

    /// One row of a git context menu, pressed.
    ///
    /// **The menu closes first**, on [`Runtime::run_file_menu_row`]'s order and
    /// for its reason: several of these verbs raise a gate or a card, and a menu
    /// still standing over the answer would be a menu covering the thing it
    /// asked for. The three prompt rows are the exception, and they do not
    /// close the menu — they *are* the menu, one state further on.
    pub(in crate::runtime) fn run_git_menu_row(&mut self, row: profiles::GitMenuRow) -> Result<()> {
        if let Some(kind) = row.prompt() {
            return self.open_git_prompt(kind);
        }
        let Some(menu) = self.window.git_menu.take() else {
            return Ok(());
        };
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        let GitMenuState {
            origin,
            root,
            target,
            graph_row,
            ..
        } = menu;
        match (row, &target) {
            // ── standing somewhere else ──
            //
            // **The two that detach go behind the gate** (user report,
            // 2026-08-19). A commit and a tag are places with no branch behind
            // them, and coming out of one standing nowhere is the consequence a
            // reader cannot see from the row they pressed — see
            // [`restore::GateRequest::GitCheckout`]. The branch arm below asks
            // nothing, because there the name *is* the answer.
            (
                profiles::GitMenuRow::Checkout,
                profiles::GitMenuTarget::Commit {
                    hash,
                    short,
                    subject,
                    ..
                },
            ) => self.ask_to_checkout(
                &origin,
                root,
                hash.clone(),
                git_graph::checkout_said(short, subject),
                restore::GitCheckoutKind::Detach,
            ),
            // A tag is a name for a commit, so standing on it is a detached
            // checkout — exactly as it is for the commit under it. `--detach`
            // and not a bare name for [`git::GitQuestion::Checkout::detach`]'s
            // own reason: a tag whose name also spells a branch would otherwise
            // silently move the branch.
            (profiles::GitMenuRow::Checkout, profiles::GitMenuTarget::Tag { name }) => self
                .ask_to_checkout(
                    &origin,
                    root,
                    name.clone(),
                    name.clone(),
                    restore::GitCheckoutKind::Detach,
                ),
            (
                profiles::GitMenuRow::Checkout,
                profiles::GitMenuTarget::LocalBranch {
                    name,
                    current: false,
                },
            ) => self.ask_to_checkout(
                &origin,
                root,
                name.clone(),
                name.clone(),
                restore::GitCheckoutKind::Stand,
            ),
            // **Prefer the local that is already there** (M10). `git checkout -b`
            // on a name that is taken is a refusal, and what the reader meant by
            // pressing this row is "put me on that branch".
            (profiles::GitMenuRow::CheckoutTracking, profiles::GitMenuTarget::Remote { name }) => {
                let local = git::tracking_local_name(name).to_owned();
                if self
                    .git_cache_at(&origin)
                    .is_some_and(|cache| cache.has_local_branch(&local))
                {
                    let said = local.clone();
                    return self.ask_to_checkout(
                        &origin,
                        root,
                        local,
                        said,
                        restore::GitCheckoutKind::Stand,
                    );
                }
                // **And the other half of the same row through the same door**
                // (user report, 2026-08-19). `git checkout -b <local> --track`
                // moves `HEAD` exactly as the branch arm above does; it used to go
                // straight to `issue_git_write`, which asks nothing whatever is in
                // the working tree — so one menu row was gated or ungated
                // depending on whether the local branch happened to exist already.
                // The gate names the *local* branch, because that is where the
                // reader lands and what the card afterwards offers to leave.
                self.ask_to_checkout(
                    &origin,
                    root,
                    name.clone(),
                    local,
                    restore::GitCheckoutKind::Track,
                )
            }
            // ── the two deletions, both behind the gate ──
            (
                profiles::GitMenuRow::DeleteBranch,
                profiles::GitMenuTarget::LocalBranch {
                    name,
                    current: false,
                },
            ) => {
                self.raise_dirty_gate(restore::GateRequest::GitDeleteBranch {
                    root,
                    name: name.clone(),
                })?;
                Ok(())
            }
            (profiles::GitMenuRow::DeleteTag, profiles::GitMenuTarget::Tag { name }) => {
                self.raise_dirty_gate(restore::GateRequest::GitDeleteTag {
                    root,
                    name: name.clone(),
                })?;
                Ok(())
            }
            // ── the changed file's own four ──
            (profiles::GitMenuRow::Stage, profiles::GitMenuTarget::Change { path, .. }) => {
                self.issue_git_write(&origin, git::GitWriteVerb::Stage, vec![path.clone()])
            }
            (profiles::GitMenuRow::Unstage, profiles::GitMenuTarget::Change { path, .. }) => {
                self.issue_git_write(&origin, git::GitWriteVerb::Unstage, vec![path.clone()])
            }
            (
                profiles::GitMenuRow::Discard,
                profiles::GitMenuTarget::Change {
                    path, untracked, ..
                },
            ) => {
                // The same gate the row's own `×` raises, which is the point: one
                // discard, one question, whichever gesture asked it.
                self.raise_dirty_gate(restore::GateRequest::GitDiscard {
                    origin: origin.clone(),
                    path: path.clone(),
                    untracked: *untracked,
                })?;
                Ok(())
            }
            (
                profiles::GitMenuRow::OpenDiff,
                profiles::GitMenuTarget::Change {
                    path,
                    group,
                    renamed_from,
                    ..
                },
            ) => {
                let GitOrigin::Column(seat) = origin else {
                    return Ok(());
                };
                self.open_git_document(
                    seat,
                    preview::PreviewSource::GitDiff {
                        root,
                        path: path.clone(),
                        against: group.diff_against(),
                    },
                    git_panel::git_document_name(path),
                    renamed_from.clone(),
                )
            }
            (
                profiles::GitMenuRow::RevealInExplorer,
                profiles::GitMenuTarget::Change { path, .. },
            ) => {
                self.reveal_in_explorer(&git_full_path(&root, path));
                Ok(())
            }
            // ── the readings ──
            (profiles::GitMenuRow::CopyPath, profiles::GitMenuTarget::Change { path, .. }) => {
                let full = git_full_path(&root, path);
                let text = full.to_string_lossy().into_owned();
                self.copy_from_git(&origin, &root, &text, &text)
            }
            (
                profiles::GitMenuRow::CopyHash,
                profiles::GitMenuTarget::Commit { hash, short, .. },
            ) => {
                let (hash, short) = (hash.clone(), short.clone());
                self.copy_from_git(&origin, &root, &hash, &short)
            }
            (
                profiles::GitMenuRow::CopySubject,
                profiles::GitMenuTarget::Commit { subject, .. },
            ) => {
                let subject = subject.clone();
                self.copy_from_git(&origin, &root, &subject, &subject)
            }
            (
                profiles::GitMenuRow::CopyName,
                profiles::GitMenuTarget::LocalBranch { name, .. }
                | profiles::GitMenuTarget::Remote { name }
                | profiles::GitMenuTarget::Tag { name },
            ) => {
                let name = name.clone();
                self.copy_from_git(&origin, &root, &name, &name)
            }
            // ── the two comparisons (D6) ──
            (
                profiles::GitMenuRow::CompareWithSelected,
                profiles::GitMenuTarget::Commit { hash, .. },
            ) => {
                let GitOrigin::Graph(_) = origin else {
                    return Ok(());
                };
                let (Some(surface), hash) = (self.graph_surface_for(&root), hash.clone()) else {
                    return Ok(());
                };
                if self.set_graph_compare(surface, &hash) {
                    if let Some(index) = graph_row {
                        self.select_graph_row(surface, index);
                    }
                    if self.refresh_chrome() {
                        self.present_chrome_change()?;
                    }
                }
                Ok(())
            }
            (
                profiles::GitMenuRow::CompareWithSelected,
                profiles::GitMenuTarget::Uncommitted { .. },
            ) => {
                let Some(surface) = self.graph_surface_for(&root) else {
                    return Ok(());
                };
                if self.set_graph_compare(surface, git_graph::GRAPH_UNCOMMITTED_HASH) {
                    if let Some(index) = graph_row {
                        self.select_graph_row(surface, index);
                    }
                    if self.refresh_chrome() {
                        self.present_chrome_change()?;
                    }
                }
                Ok(())
            }
            (
                profiles::GitMenuRow::CompareWithWorkingTree,
                profiles::GitMenuTarget::Commit { hash, .. },
            ) => {
                let Some(surface) = self.graph_surface_for(&root) else {
                    return Ok(());
                };
                let hash = hash.clone();
                self.compare_graph_with_working_tree(surface, &root, &hash)
            }
            // Every other pair is a row that is not on this target's menu, which
            // the hit test cannot produce and the keyboard walk cannot reach.
            _ => Ok(()),
        }
    }

    /// **This commit against what is on disk** (D6, `b: None`).
    ///
    /// The open row is the near end of every comparison, so the commit is opened
    /// first — and only when it is not already open, because opening is a
    /// *toggle* and a second press on the row you asked about would shut it.
    /// Then the far end is pointed at the working tree, which is spelled with
    /// [`git_graph::GRAPH_UNCOMMITTED_HASH`] exactly as a `Ctrl`+click on the
    /// working tree's own row spells it.
    fn compare_graph_with_working_tree(
        &mut self,
        surface: PreviewSurface,
        root: &Path,
        hash: &str,
    ) -> Result<()> {
        let tab = self.preview_tab_index(surface);
        let open = self.window.tabs[tab]
            .git_graph_view
            .get(&surface)
            .and_then(|view| view.expanded.clone());
        if open.as_deref() != Some(hash) {
            self.expand_graph_commit(surface, root, hash)?;
        }
        if let Some(view) = self.window.tabs[tab].git_graph_view.get_mut(&surface) {
            view.compare = Some(git_graph::GRAPH_UNCOMMITTED_HASH.to_owned());
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Which preview surface is showing this repository's graph, if one is.
    ///
    /// **Every tab's map**, because a window torn off in one tab goes on drawing
    /// while you read another (§7.1.2) and is still the surface this verb is
    /// about. The `git_graphs_shown` test is what keeps the answer to a graph
    /// that is on the glass, exactly as it always did.
    fn graph_surface_for(&self, root: &Path) -> Option<PreviewSurface> {
        self.window
            .tabs
            .iter()
            .flat_map(|tab| tab.git_graph_view.iter())
            .find(|(surface, view)| {
                view.root == root && self.window.git_graphs_shown.contains_key(surface)
            })
            .map(|(surface, _)| *surface)
    }

    /// Which tab's plane holds the graph state of this repository.
    ///
    /// The tab on screen when it has one, so an ordinary docked graph is
    /// addressed exactly as it was; otherwise the tab that does, which is the
    /// one a floating graph is reading out of. An answer filed against any other
    /// tab is an answer the response gate drops on arrival.
    fn graph_tab_index(&self, root: &Path) -> usize {
        let active = self.window.active_tab;
        if self.window.tabs[active].git_graphs.contains_key(root) {
            return active;
        }
        self.window
            .tabs
            .iter()
            .position(|tab| tab.git_graphs.contains_key(root))
            .unwrap_or(active)
    }

    /// The cache one origin reads from.
    fn git_cache_at(&self, origin: &GitOrigin) -> Option<&git::GitCache> {
        let active = self.window.active_tab;
        match origin {
            GitOrigin::Column(seat) => self.window.tabs[active].git_trees.get(seat),
            GitOrigin::Graph(root) => self.window.tabs[self.graph_tab_index(root)]
                .git_graphs
                .get(root)
                .map(|state| &state.cache),
            GitOrigin::Float(id) => self
                .window
                .float
                .live(*id)
                .and_then(float::FloatWin::files)
                .map(|files| &files.git),
        }
    }

    /// Which surface a repository answer for this origin comes back to.
    fn git_host_at(&self, origin: &GitOrigin) -> git::GitHost {
        let tab = self.window.tabs[self.window.active_tab].id;
        match origin {
            GitOrigin::Column(seat) => git::GitHost::Column(LeafId { tab, seat: *seat }),
            // **The tab whose plane holds the graph**, which is the tab on screen
            // for a docked one and the tab it was opened in for a torn-off one.
            // Naming the active tab unconditionally filed a floating graph's
            // reread against a cache that tab does not have, and the response
            // gate then dropped the answer — a refresh that did nothing.
            GitOrigin::Graph(root) => git::GitHost::Graph {
                tab: self.window.tabs[self.graph_tab_index(root)].id,
                root: root.clone(),
            },
            // The tab travels with it for the pool a *document* answer would
            // land in — see [`git::GitHost::Float`]; the page's own five answers
            // come home by the epoch.
            GitOrigin::Float(id) => git::GitHost::Float { id: *id, tab },
        }
    }

    /// **Issue a write into whichever cache asked for it** (v2 ④).
    ///
    /// [`Self::write_to_repository`]'s general form, and that one now delegates
    /// here. The generalisation is the whole of what the context menus needed
    /// from this layer: a `git branch -d` raised from a graph has no column, and
    /// a `git add` raised from a panel has no root of its own — one function that
    /// takes the origin answers both without either surface learning about the
    /// other.
    pub(in crate::runtime) fn issue_git_write(
        &mut self,
        origin: &GitOrigin,
        verb: git::GitWriteVerb,
        paths: Vec<String>,
    ) -> Result<()> {
        let active = self.window.active_tab;
        let host = self.git_host_at(origin);
        let question = match origin {
            GitOrigin::Column(seat) => self.window.tabs[active]
                .git_trees
                .get_mut(seat)
                .and_then(|cache| cache.begin_write(verb, paths)),
            GitOrigin::Graph(root) => self.window.tabs[active]
                .git_graphs
                .get_mut(root)
                .and_then(|state| state.cache.begin_write(verb, paths)),
            GitOrigin::Float(id) => self
                .window
                .float
                .live_mut(*id)
                .and_then(float::FloatWin::files_mut)
                .and_then(|files| files.git.begin_write(verb, paths)),
        };
        let Some(question) = question else {
            return Ok(());
        };
        let window = self.window_id();
        if !self.app.git_worker.request(git::GitRequest {
            window,
            host,
            question,
        }) {
            self.disable_git_worker();
            return Ok(());
        }
        // The rows dim now, so the frame that shows them dimmed is this one.
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        Ok(())
    }

    /// **Every checkout this window issues goes through here** (user ruling,
    /// 2026-08-19), and here is where it is decided whether a question is put
    /// first.
    ///
    /// Three tiers, in the ruling's own words and in the order they are asked:
    ///
    /// 1. **A gate, always, when the working tree has changes.** git refuses the
    ///    subset of these it would have to overwrite, in its own words, and the
    ///    page prints them — but the ones it *allows* still carry uncommitted
    ///    work across into somewhere else, which is the case a reader cannot
    ///    undo by pressing the same button again.
    /// 2. **A gate, always, when the move detaches `HEAD`.** Nothing is lost and
    ///    the sentence does not pretend otherwise; what it names is the state you
    ///    come out in, which is the thing the row you pressed does not show. This
    ///    is the one that bit a reader browsing the graph.
    /// 3. **No gate at all** for a clean tree moving onto a named branch. It is
    ///    the ordinary verb this whole page is built around, the masthead says
    ///    where you landed, and undoing it is the same verb again.
    ///
    /// The rule itself is [`restore::checkout_needs_gate`] and is not restated
    /// here: it is one sentence three doors have to answer the same way, and the
    /// door that answered it differently — `Checkout tracking`, which reached the
    /// repository through `issue_git_write` — is why it is written down once.
    ///
    /// `said` is what the gate names out loud — a short hash and a subject, or a
    /// branch's own name — worked out by the caller, which is the only place that
    /// still has the row.
    pub(crate) fn ask_to_checkout(
        &mut self,
        origin: &GitOrigin,
        root: std::path::PathBuf,
        target: String,
        said: String,
        kind: restore::GitCheckoutKind,
    ) -> Result<()> {
        let dirty = self
            .git_cache_at(origin)
            .and_then(|cache| cache.status().ready())
            .is_some_and(|status| !status.entries.is_empty());
        if !restore::checkout_needs_gate(kind, dirty) {
            return self.checkout_at(origin, target, kind);
        }
        self.raise_dirty_gate(restore::GateRequest::GitCheckout {
            root,
            target,
            said,
            kind,
            dirty,
        })?;
        Ok(())
    }

    /// Stand somewhere else, from whichever surface asked — **the one place a
    /// move onto somewhere else is actually performed**, whichever of the three
    /// spellings it is.
    ///
    /// Reached only from [`Self::ask_to_checkout`] and from the gate's confirmed
    /// answer, which is what makes "every verb that moves `HEAD` has answered the
    /// tiers" a property of the call graph rather than of three call sites
    /// remembering to.
    pub(in crate::runtime) fn checkout_at(
        &mut self,
        origin: &GitOrigin,
        target: String,
        kind: restore::GitCheckoutKind,
    ) -> Result<()> {
        let detach = matches!(kind, restore::GitCheckoutKind::Detach);
        // **The branch being left is noted now or never** — see
        // [`WindowRuntime::checkout_from`]. Noted for every checkout, including
        // the ones git is about to refuse: a refusal clears it along with
        // everything else the receipt does, and a note nobody reads costs one
        // `String`.
        self.window.checkout_from = self.git_cache_at(origin).and_then(|cache| {
            Some((
                cache.root()?.to_path_buf(),
                cache
                    .status()
                    .ready()
                    .and_then(|status| status.branch.clone()),
                detach,
            ))
        });
        // **The tracking spelling is a ref write and still a checkout.** git is
        // given one command that creates the local branch and stands on it, so it
        // goes out through the writing door — but it arrived here through the same
        // question every other move answers, and the card afterwards is raised
        // from the same place (see the `GitAnswer::Write` arm).
        if matches!(kind, restore::GitCheckoutKind::Track) {
            return self.issue_git_write(
                origin,
                git::GitWriteVerb::CheckoutTracking { name: target },
                Vec::new(),
            );
        }
        match origin {
            GitOrigin::Column(seat) => self.checkout_from_column(*seat, target, detach),
            GitOrigin::Graph(root) => {
                let root = root.clone();
                self.checkout_in_graph(&root, target, detach)
            }
            GitOrigin::Float(id) => self.checkout_from_float(*id, target, detach),
        }
    }

    /// Put one of a repository's own words on the clipboard, and say so.
    ///
    /// [`Self::copy_from_graph`]'s general form — the same clipboard door and the
    /// same green card, anchored over whichever surface asked rather than over a
    /// preview seat the panel does not have.
    fn copy_from_git(
        &mut self,
        origin: &GitOrigin,
        root: &Path,
        text: &str,
        said: &str,
    ) -> Result<()> {
        let result = hang_watch::during(hang_watch::Station::ClipboardWrite, || {
            bt_platform::set_clipboard_text(text)
        })
        .map_err(|error| anyhow!(error))
        .context("copy a repository's own words to the clipboard");
        if !recoverable_clipboard_write(result, "git menu copy") {
            return Ok(());
        }
        let anchor = match origin {
            GitOrigin::Column(seat) => toast::ToastAnchor::FilesColumn(*seat),
            GitOrigin::Graph(_) => self.git_toast_anchor(&git::GitHost::Graph {
                tab: self.window.tabs[self.window.active_tab].id,
                root: root.to_owned(),
            }),
            // A window is not a seat, so there is no pane for the card to stand
            // over — which is precisely the case `Window` is the answer to.
            GitOrigin::Float(_) => toast::ToastAnchor::Window,
        };
        self.toast(
            toast::ToastKind::Ok,
            anchor,
            None,
            git_graph::graph_copied(said),
        )
    }

    /// Which origin a request keyed by root should be issued into.
    ///
    /// The gate's two ref deletions name a repository and not a pane — a branch
    /// is a fact about the one and not the other — so the surface is looked up
    /// again when the answer is spent. A graph is preferred because a graph is
    /// where these menus mostly come from, and either surface's cache re-reads
    /// the whole repository when the receipt arrives anyway (see
    /// [`git::GitWriteVerb::moves_refs`]).
    pub(in crate::runtime) fn git_origin_for_root(&self, root: &Path) -> Option<GitOrigin> {
        let active = self.window.active_tab;
        if self.window.tabs[active].git_graphs.contains_key(root) {
            return Some(GitOrigin::Graph(root.to_owned()));
        }
        if let Some(seat) = self.window.tabs[active]
            .git_trees
            .iter()
            .find(|(_, cache)| cache.root() == Some(root))
            .map(|(seat, _)| *seat)
        {
            return Some(GitOrigin::Column(seat));
        }
        // **And last, a floating page** (user ruling, 2026-08-19). Last because
        // the docked surfaces are where these menus mostly come from, and a
        // window on this repository is still a place the receipt can be spent —
        // which it has to be, or a `git branch -d` raised from a float would
        // find nowhere to come home to when the gate is answered.
        self.window
            .float
            .live_windows()
            .find(|win| {
                win.files()
                    .is_some_and(|files| files.git.root() == Some(root))
            })
            .map(|win| GitOrigin::Float(win.epoch))
    }

    /// One key, with a git context menu up.
    ///
    /// **Total**, on the file menu's own rule: a menu that can be walked has to
    /// keep the keys it walks with, and with a menu on screen there is nothing
    /// behind it to type into. What changes when the menu has become a prompt is
    /// only *what* the keys mean — there is now something to type into, and it
    /// takes every key exactly as the graph's search field does.
    pub(in crate::runtime) fn git_menu_key(&mut self, event: &KeyEvent) -> Result<()> {
        use text_field::TextMove;
        // The application's modifier — see `search_field_key`'s note (M1-7).
        let control = input::is_command_chord(self.window.modifiers);
        let shift = self.window.modifiers.shift_key();
        if self
            .window
            .git_menu
            .as_ref()
            .is_some_and(|menu| menu.prompt.is_some())
        {
            match &event.logical_key {
                // **One press, one layer** — §7.1.5's ladder read inside a single
                // popup, which is exactly what the pane menu's submenu does. The
                // field goes and the verbs come back; a second Esc closes the
                // menu.
                Key::Named(NamedKey::Escape) => {
                    if !event.repeat
                        && let Some(menu) = self.window.git_menu.as_mut()
                    {
                        menu.prompt = None;
                        if self.refresh_chrome() {
                            self.present_chrome_change()?;
                        }
                    }
                    return Ok(());
                }
                Key::Named(NamedKey::Enter) => {
                    if !event.repeat {
                        self.commit_git_prompt()?;
                    }
                    return Ok(());
                }
                _ => {}
            }
            let Some(field) = self
                .window
                .git_menu
                .as_mut()
                .and_then(|menu| menu.prompt.as_mut())
                .map(|prompt| &mut prompt.field)
            else {
                return Ok(());
            };
            match &event.logical_key {
                Key::Named(NamedKey::Backspace) => {
                    field.backspace();
                }
                Key::Named(NamedKey::Delete) => {
                    field.delete();
                }
                Key::Named(NamedKey::Home) => field.step(TextMove::Home, shift),
                Key::Named(NamedKey::End) => field.step(TextMove::End, shift),
                Key::Named(NamedKey::ArrowLeft) => field.step(
                    if control {
                        TextMove::WordLeft
                    } else {
                        TextMove::Left
                    },
                    shift,
                ),
                Key::Named(NamedKey::ArrowRight) => field.step(
                    if control {
                        TextMove::WordRight
                    } else {
                        TextMove::Right
                    },
                    shift,
                ),
                Key::Character(text) if control => {
                    if text.eq_ignore_ascii_case("a") {
                        field.select_all();
                    }
                }
                // A chord is not text (M1-7) — see `search_field_key`'s own arm.
                Key::Character(text) if input::types_a_character(self.window.modifiers) => {
                    field.insert(text);
                }
                // A modifier on its own, a function key, anything else: the field
                // owns it and does nothing with it.
                _ => {}
            }
            if self.refresh_chrome() {
                self.present_chrome_change()?;
            }
            return Ok(());
        }
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                if !event.repeat {
                    self.close_git_menu()?;
                }
            }
            // Repeats are honoured on the travel keys and nowhere else, for the
            // file menu's own reason: holding an arrow down is one continuous
            // "further", and holding Enter is not one continuous "again".
            Key::Named(NamedKey::ArrowDown) | Key::Named(NamedKey::ArrowUp) => {
                let forwards = matches!(event.logical_key, Key::Named(NamedKey::ArrowDown));
                if let Some(menu) = self.window.git_menu.as_mut() {
                    let rows = profiles::git_menu(&menu.target).rows;
                    menu.hover = profiles::git_menu_step(&rows, &menu.target, menu.hover, forwards);
                }
                if self.refresh_overlay() {
                    self.present_chrome_change()?;
                }
            }
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                if !event.repeat
                    && let Some(row) = self.window.git_menu.as_ref().and_then(|menu| menu.hover)
                {
                    self.run_git_menu_row(row)?;
                }
            }
            // Everything else is swallowed rather than passed down: with a menu
            // on screen there is nothing to type into.
            _ => {}
        }
        Ok(())
    }

    /// A composition, with a git prompt holding the keyboard (v2 ④).
    pub(in crate::runtime) fn git_prompt_ime(&mut self, event: &Ime) -> Result<()> {
        let Some(prompt) = self
            .window
            .git_menu
            .as_mut()
            .and_then(|menu| menu.prompt.as_mut())
        else {
            return Ok(());
        };
        match event {
            Ime::Preedit(text, _) => prompt.field.set_preedit(text),
            Ime::Commit(text) => prompt.field.insert(text),
            Ime::Enabled | Ime::Disabled => return Ok(()),
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Where the graph toolbar's controls are, for one surface.
    ///
    /// Re-derived from the frame that surface last drew rather than remembered,
    /// which is the same `&self`-hit-test discipline `git_pages_shown` exists
    /// for: the rectangle a control can be pressed in has to be the rectangle it
    /// was drawn in, by one derivation and not by two that agree today.
    pub(in crate::runtime) fn graph_toolbar_rects(
        &self,
        surface: PreviewSurface,
    ) -> Option<git_graph::GraphToolbarRects> {
        let scale = self.window.renderer.scale_factor() as f32;
        let content = self.window.git_graphs_shown.get(&surface)?;
        let body = self.preview_surface_body_rect(surface, scale)?;
        let geometry = git_graph::graph_geometry(body, content, scale);
        let toolbar = content.toolbar.as_ref()?;
        Some(git_graph::graph_toolbar_rects(
            geometry.head?,
            toolbar,
            scale,
        ))
    }

    /// One of the toolbar's four controls, pressed.
    pub(in crate::runtime) fn press_graph_tool(
        &mut self,
        surface: PreviewSurface,
        tool: git_graph::GraphTool,
    ) -> Result<()> {
        match tool {
            git_graph::GraphTool::Filter => self.toggle_graph_filter_menu(surface)?,
            git_graph::GraphTool::Search => self.focus_graph_search(surface)?,
            git_graph::GraphTool::SearchClear => self.clear_graph_search(surface)?,
            git_graph::GraphTool::Refresh => self.refresh_graph(surface)?,
            git_graph::GraphTool::LeaveDetached => self.leave_detached_head(surface)?,
        }
        Ok(())
    }

    /// **The toolbar button this menu hangs from**, or `None` when the graph
    /// that carries it is not on the glass.
    ///
    /// [`Self::preview_menu_stand`]'s third reader: the layout below draws
    /// exactly when this answers, and [`Self::popups_up`] counts the popup as up
    /// exactly then. Its rows never fold — a repository with no branch still
    /// lists `All branches` — so the stand is the anchor alone.
    pub(in crate::runtime) fn graph_filter_menu_stand(
        &self,
        surface: PreviewSurface,
    ) -> Option<[f32; 4]> {
        Some(self.graph_toolbar_rects(surface)?.filter)
    }

    /// Where the filter menu hangs, or nothing when it is not up.
    pub(in crate::runtime) fn graph_filter_menu_layout(
        &mut self,
    ) -> Option<profiles::GitFilterMenuLayout> {
        let surface = self.window.graph_filter_menu.as_ref()?.surface;
        let anchor = self.graph_filter_menu_stand(surface)?;
        let rows = profiles::git_filter_rows(&self.graph_filter_branches(surface));
        let scale = self.window.renderer.scale_factor() as f32;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        Some(profiles::git_filter_menu_layout(
            anchor,
            (width as f32, height as f32),
            scale,
            rows,
            &mut measure,
        ))
    }

    /// Every local branch of the repository this surface's graph is of.
    ///
    /// Off the graph's own cache, so the menu lists what the *graph* was told
    /// rather than what some column beside it happens to know: two surfaces
    /// looking at one repository can be at two different points in their reading,
    /// and a menu that borrowed the other one's answer would offer a branch this
    /// graph cannot walk.
    fn graph_filter_branches(&self, surface: PreviewSurface) -> Vec<String> {
        let tab = self.preview_tab_index(surface);
        let Some(root) = self.window.tabs[tab]
            .git_graph_view
            .get(&surface)
            .map(|view| view.root.clone())
        else {
            return Vec::new();
        };
        self.window.tabs[tab]
            .git_graphs
            .get(&root)
            .and_then(|state| state.cache.refs().ready())
            .map(|refs| {
                git::local_branches(refs)
                    .map(|entry| entry.name.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether this surface's search field holds the keyboard (T4).
    pub(in crate::runtime) fn graph_search_focused(&self, surface: PreviewSurface) -> bool {
        self.window.tabs[self.preview_tab_index(surface)]
            .git_graph_view
            .get(&surface)
            .is_some_and(|view| view.search_focused)
    }

    /// One key, with the search field holding the keyboard (T4).
    ///
    /// **Total**, on [`preview_edit::command`]'s own rule: once a text field has
    /// the focus every key is the field's, and a key that fell through would land
    /// in the list underneath as a travel command. What "the field's" means for a
    /// key it has no use for is *nothing*, said by returning `true` — which is
    /// the answer the files column and the read-only preview both already give.
    pub(in crate::runtime) fn graph_search_key(
        &mut self,
        surface: PreviewSurface,
        event: &KeyEvent,
    ) -> Result<bool> {
        use text_field::TextMove;
        // The application's modifier — see `search_field_key`'s note (M1-7).
        let control = input::is_command_chord(self.window.modifiers);
        let shift = self.window.modifiers.shift_key();
        let tab = self.preview_tab_index(surface);
        enum Then {
            /// Nothing but a repaint.
            Draw,
            /// Ask git about what is in the field now.
            Ask,
            /// Walk to the next match, or the previous one.
            Step(bool),
            /// Give the keyboard back to the graph and forget the search.
            Clear,
        }
        let Some(view) = self.window.tabs[tab].git_graph_view.get_mut(&surface) else {
            return Ok(false);
        };
        let then = match &event.logical_key {
            Key::Named(NamedKey::Escape) => Then::Clear,
            // **Enter is two verbs and the field decides which** (T4): the first
            // press runs the search, and every press after it walks the matches.
            // Told apart by whether git has been asked about *this* text, which
            // is the only honest reading — a reader who edits the query and
            // presses Enter means "search again", and one who does not means
            // "next".
            Key::Named(NamedKey::Enter) => {
                if view.search_asked.as_deref() == Some(view.search.text()) {
                    Then::Step(!shift)
                } else {
                    Then::Ask
                }
            }
            Key::Named(NamedKey::Backspace) => {
                view.search.backspace();
                Then::Draw
            }
            Key::Named(NamedKey::Delete) => {
                view.search.delete();
                Then::Draw
            }
            Key::Named(NamedKey::Home) => {
                view.search.step(TextMove::Home, shift);
                Then::Draw
            }
            Key::Named(NamedKey::End) => {
                view.search.step(TextMove::End, shift);
                Then::Draw
            }
            Key::Named(NamedKey::ArrowLeft) => {
                view.search.step(
                    if control {
                        TextMove::WordLeft
                    } else {
                        TextMove::Left
                    },
                    shift,
                );
                Then::Draw
            }
            Key::Named(NamedKey::ArrowRight) => {
                view.search.step(
                    if control {
                        TextMove::WordRight
                    } else {
                        TextMove::Right
                    },
                    shift,
                );
                Then::Draw
            }
            Key::Character(text) if control => {
                if text.eq_ignore_ascii_case("a") {
                    view.search.select_all();
                }
                Then::Draw
            }
            // A chord is not text (M1-7) — see `search_field_key`'s own arm.
            Key::Character(text) if input::types_a_character(self.window.modifiers) => {
                view.search.insert(text);
                Then::Draw
            }
            // A modifier on its own, a function key, anything else: the field
            // owns it and does nothing with it.
            _ => Then::Draw,
        };
        match then {
            Then::Draw => {}
            Then::Ask => self.ask_graph_search(surface)?,
            Then::Step(forwards) => self.step_graph_search(surface, forwards)?,
            Then::Clear => {
                self.clear_graph_search(surface)?;
                return Ok(true);
            }
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// A composition, with the search field holding the keyboard (T4).
    pub(in crate::runtime) fn graph_search_ime(&mut self, event: &Ime) -> Result<()> {
        let Some(surface) = self.preview_keyboard_surface() else {
            return Ok(());
        };
        let tab = self.preview_tab_index(surface);
        let Some(view) = self.window.tabs[tab].git_graph_view.get_mut(&surface) else {
            return Ok(());
        };
        match event {
            Ime::Preedit(text, _) => view.search.set_preedit(text),
            Ime::Commit(text) => view.search.insert(text),
            Ime::Enabled | Ime::Disabled => return Ok(()),
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Put what is in the field to git (T4).
    fn ask_graph_search(&mut self, surface: PreviewSurface) -> Result<()> {
        let tab = self.preview_tab_index(surface);
        let tab_id = self.window.tabs[tab].id;
        let Some(view) = self.window.tabs[tab].git_graph_view.get_mut(&surface) else {
            return Ok(());
        };
        let query = view.search.text().to_owned();
        let root = view.root.clone();
        // An empty field is not a search. Pressing Enter in one forgets the last
        // one rather than asking git about the empty string, which every commit
        // matches.
        if view.search.is_empty() {
            view.search_asked = None;
            view.search_at = None;
            if let Some(state) = self.window.tabs[tab].git_graphs.get_mut(&root) {
                state.cache.clear_search();
            }
            return Ok(());
        }
        view.search_asked = Some(query.clone());
        view.search_at = None;
        let Some(state) = self.window.tabs[tab].git_graphs.get_mut(&root) else {
            return Ok(());
        };
        let Some(question) = state.cache.begin_search(&query) else {
            return Ok(());
        };
        if !self.app.git_worker.request(git::GitRequest {
            window: self.window_id(),
            host: git::GitHost::Graph { tab: tab_id, root },
            question,
        }) {
            self.disable_git_worker();
        }
        Ok(())
    }

    /// Walk to the next match, or the previous one, and reveal it (T4).
    ///
    /// **Reuses the parent-seek** (D2) for a match that is not in the loaded
    /// pages: `git rev-list --all` sees the whole history and the graph has read
    /// fifty commits of it, so a match nine hundred rows down is a hash the page
    /// does not contain — which is exactly the case the seek was written for.
    fn step_graph_search(&mut self, surface: PreviewSurface, forwards: bool) -> Result<()> {
        let tab = self.preview_tab_index(surface);
        let Some(view) = self.window.tabs[tab].git_graph_view.get(&surface) else {
            return Ok(());
        };
        let root = view.root.clone();
        let Some(query) = view.search_asked.clone() else {
            return Ok(());
        };
        let Some(matches) = self.window.tabs[tab]
            .git_graphs
            .get(&root)
            .and_then(|state| state.cache.search(&query))
            .and_then(git::GitSlot::ready)
            .cloned()
        else {
            return Ok(());
        };
        if matches.is_empty() {
            return Ok(());
        }
        // The ring is [`git_graph::search_step`]'s — see there for why stepping
        // off the end starts again rather than stopping.
        let Some(at) = git_graph::search_step(
            self.window.tabs[tab]
                .git_graph_view
                .get(&surface)
                .and_then(|view| view.search_at),
            matches.len(),
            forwards,
        ) else {
            return Ok(());
        };
        if let Some(view) = self.window.tabs[tab].git_graph_view.get_mut(&surface) {
            view.search_at = Some(at);
            view.seek = Some(git_graph::GraphSeek {
                hash: matches[at].clone(),
                pages: 0,
            });
        }
        self.step_graph_seeks()?;
        Ok(())
    }

    /// **E61: the opener closes the others**, stated here as it is at every other
    /// opener in this window.
    fn toggle_graph_filter_menu(&mut self, surface: PreviewSurface) -> Result<()> {
        let already = self
            .window
            .graph_filter_menu
            .as_ref()
            .is_some_and(|menu| menu.surface == surface);
        self.close_popups_except(Popup::GraphFilter);
        self.window.graph_filter_menu = (!already).then_some(GraphFilterMenuState {
            surface,
            hover: None,
        });
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    pub(in crate::runtime) fn close_graph_filter_menu(&mut self) -> Result<bool> {
        if self.window.graph_filter_menu.take().is_none() {
            return Ok(false);
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// Do what one row of the filter menu says.
    ///
    /// **The menu stays up**, which is the one place this popup parts company
    /// with its four neighbours — and it parts company because it is a different
    /// kind of thing. Those four are lists of *verbs*: you pick one and it
    /// happens, so the menu has done its job. This is a list of *settings*, and
    /// picking two branches means picking one and then the other; a menu that
    /// shut on the first would make the second gesture a second opening.
    pub(in crate::runtime) fn run_graph_filter_row(
        &mut self,
        row: &profiles::GitFilterRow,
    ) -> Result<()> {
        let Some(surface) = self
            .window
            .graph_filter_menu
            .as_ref()
            .map(|menu| menu.surface)
        else {
            return Ok(());
        };
        let tab = self.preview_tab_index(surface);
        let Some(view) = self.window.tabs[tab].git_graph_view.get_mut(&surface) else {
            return Ok(());
        };
        match row {
            profiles::GitFilterRow::All => view.filter.branches.clear(),
            profiles::GitFilterRow::Branch(name) => view.filter.toggle_branch(name),
            profiles::GitFilterRow::Remotes => view.filter.remotes = !view.filter.remotes,
            profiles::GitFilterRow::Tags => view.filter.tags = !view.filter.tags,
        }
        let (root, refs) = (view.root.clone(), view.filter.log_refs());
        // **A filter change is a different history**, so the walk is thrown away
        // and asked again — see `GitCache::set_log_refs`. A branch that was not
        // walked has no commits on hand to reveal, so there is nothing here that
        // could have been done by filtering what is already loaded.
        if let Some(state) = self.window.tabs[tab].git_graphs.get_mut(&root)
            && state.cache.set_log_refs(refs)
        {
            state.invalidate();
            self.ask_git_for_graphs(&[tab]);
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The search field takes the keyboard (T4).
    fn focus_graph_search(&mut self, surface: PreviewSurface) -> Result<()> {
        let tab = self.preview_tab_index(surface);
        if let Some(view) = self.window.tabs[tab].git_graph_view.get_mut(&surface) {
            view.search_focused = true;
        }
        // A press in the field is a press on a control, so it closes the popups
        // for E61's reason exactly as every other opener does.
        self.window.graph_filter_menu = None;
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The `×`, or `Esc` in the field: forget the search and give the keyboard
    /// back to the graph (T4).
    fn clear_graph_search(&mut self, surface: PreviewSurface) -> Result<()> {
        let tab = self.preview_tab_index(surface);
        let Some(view) = self.window.tabs[tab].git_graph_view.get_mut(&surface) else {
            return Ok(());
        };
        view.search.clear();
        view.search_asked = None;
        view.search_at = None;
        view.search_focused = false;
        let root = view.root.clone();
        if let Some(state) = self.window.tabs[tab].git_graphs.get_mut(&root) {
            state.cache.clear_search();
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// **Read the repository again** (T5).
    ///
    /// Three questions and exactly three: the refs, the status and the first page
    /// of the log. It is allowed past R31's read boundary because it is an
    /// explicit gesture — the whole of that rule is that a repository is not read
    /// because time passed, and this is a reader asking.
    ///
    /// The picture is invalidated with them: a history that has been rewritten
    /// under the lane walker is not a history it can resume, and a refresh is
    /// exactly the moment that may have happened.
    fn refresh_graph(&mut self, surface: PreviewSurface) -> Result<()> {
        let Some(root) = self.window.tabs[self.preview_tab_index(surface)]
            .git_graph_view
            .get(&surface)
            .map(|view| view.root.clone())
        else {
            return Ok(());
        };
        self.reread_git_origin(&GitOrigin::Graph(root), Ask::Always);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The docked Git page's own refresh (R31's third moment, the pressed one).
    ///
    /// [`Self::refresh_graph`]'s twin down to the line, which is why both are
    /// four lines around [`Self::reread_git_origin`]: the two surfaces differ in
    /// how they are addressed and in nothing else that matters here.
    fn refresh_git_column(&mut self, seat: SeatId) -> Result<()> {
        self.reread_git_origin(&GitOrigin::Column(seat), Ask::Always);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// **Ask one surface's three questions again, without taking its page down.**
    ///
    /// The one place a re-read is issued, whichever of the four moments asked for
    /// it. See `GitCache::begin_reread` for why this is not `refresh`: what is on
    /// screen when a reader presses "check" — or when a command they ran ends — is
    /// still perfectly true, it may simply have got older, and blanking a history
    /// to `Reading the repository…` in answer to that is the page punishing them
    /// for asking.
    ///
    /// Answers whether anything was actually asked, which is what [`Ask::Settled`]
    /// makes a real question: a burst of command-ends finds the first re-read
    /// still owed and issues nothing.
    ///
    /// **A graph's lanes are left exactly where they are.** The rule this page
    /// keeps for its rows it has to keep for its roads: what is on screen is
    /// still true, and a re-read does not know yet whether the history moved.
    /// The lanes used to be thrown away here, on the chance that it had, which
    /// drew every dot in lane zero with no lines at all — and no Uncommitted
    /// Changes row, so every row below it shifted up — for the length of a `git
    /// log`, and then drew the identical picture again. Under a repository
    /// something else is writing to, the kernel says so every couple of seconds
    /// and the whole graph blinks. `GraphState::sync` decides it from the answer
    /// instead, by asking whether the list it got back is still the list the rows
    /// were laid out over.
    fn reread_git_origin(&mut self, origin: &GitOrigin, ask: Ask) -> bool {
        let host = self.git_host_at(origin);
        let active = self.window.active_tab;
        let questions = match origin {
            GitOrigin::Column(seat) => match self.window.tabs[active].git_trees.get_mut(seat) {
                Some(cache) => ask.begin(cache),
                None => Vec::new(),
            },
            GitOrigin::Graph(root) => match self.window.tabs[active].git_graphs.get_mut(root) {
                Some(state) => ask.begin(&mut state.cache),
                None => Vec::new(),
            },
            GitOrigin::Float(id) => match self
                .window
                .float
                .live_mut(*id)
                .and_then(float::FloatWin::files_mut)
            {
                Some(files) => ask.begin(&mut files.git),
                None => Vec::new(),
            },
        };
        if questions.is_empty() {
            return false;
        }
        for question in questions {
            if !self.app.git_worker.request(git::GitRequest {
                window: self.window_id(),
                host: host.clone(),
                question,
            }) {
                self.disable_git_worker();
                break;
            }
        }
        true
    }

    /// **Every git surface on screen, and whether it is on screen** (R31).
    ///
    /// The `showing` flag is carried rather than filtered out here so that
    /// [`git_surfaces_wanting_reread`] — the free function that can be tested
    /// without a window — is the thing that drops the ones nobody is looking at.
    /// A column that is on its Files page is in this list, with `false`, and that
    /// is the fact the pin is about.
    fn git_surfaces_on_screen(&self) -> Vec<(GitOrigin, PathBuf, bool)> {
        let active = self.window.active_tab;
        let mut surfaces: Vec<(GitOrigin, PathBuf, bool)> = self.window.tabs[active]
            .files
            .iter()
            .filter_map(|(seat, state)| {
                let root = self.window.tabs[active]
                    .git_trees
                    .get(seat)?
                    .root()?
                    .to_path_buf();
                Some((
                    GitOrigin::Column(*seat),
                    root,
                    // **Drawn, not merely turned to.** `git_pages_shown` is the
                    // frame's own record of which columns put a Git page on the
                    // glass; a seat that is collapsed or not laid out is not a
                    // surface looking at a repository, however its `view` is set.
                    state.view == seats::FilesView::Git
                        && self.window.git_pages_shown.contains_key(seat),
                ))
            })
            .collect();
        // **And the floating pages** (user ruling, 2026-08-19), on the identical
        // terms: a window standing on its Git page is a surface looking at a
        // repository, so it subscribes to the kernel's news and re-reads with
        // everything else. `float_git_pages_shown` is its "drawn, not merely
        // turned to" — the float's own answer to the flag above it.
        for win in self.window.float.live_windows() {
            let Some(root) = win
                .files()
                .and_then(|files| files.git.root())
                .map(Path::to_path_buf)
            else {
                continue;
            };
            surfaces.push((
                GitOrigin::Float(win.epoch),
                root,
                self.window.float_git_pages_shown.contains_key(&win.epoch),
            ));
        }
        // A graph is addressed by its root, so two seats showing one repository
        // are one surface — and the second would find the first's re-read already
        // owed in any case.
        let mut graphs: Vec<PathBuf> = Vec::new();
        for (seat, view) in &self.window.tabs[active].git_graph_view {
            if !self.window.git_graphs_shown.contains_key(seat)
                || !self.window.tabs[active].git_graphs.contains_key(&view.root)
                || graphs.contains(&view.root)
            {
                continue;
            }
            graphs.push(view.root.clone());
            surfaces.push((GitOrigin::Graph(view.root.clone()), view.root.clone(), true));
        }
        surfaces
    }

    /// **Keep the kernel's subscriptions level with what is on screen, and act
    /// on anything it has said** (R31's D).
    ///
    /// Both halves in one step because they are one question asked at one
    /// moment: which repositories is this window looking at, and which of those
    /// have news that has ripened. The set is derived from
    /// [`Self::git_surfaces_on_screen`] and the master switch — the same two
    /// conditions the first reading is gated on — so a page that is left, a tab
    /// that is switched away from and a switch that is turned off all drop their
    /// handles here, by the set no longer containing them.
    ///
    /// **A subscription costs nothing while nothing happens.** No timer is armed
    /// unless a notification has already arrived, which is why this can be called
    /// on every turn of the loop beside every other clock in this window without
    /// being the polling R31 forbids.
    pub(in crate::runtime) fn advance_git_watch(&mut self, now: Instant) -> Result<()> {
        // Asked on every turn of the loop, so the switch is read before the list
        // is built rather than used to filter one: with the panel off there is
        // nothing to enumerate and `sync` is handed an empty set, which drops
        // every handle and then costs nothing on every turn after that.
        let on_screen = if self.git_panel_on() {
            self.git_surfaces_on_screen()
        } else {
            Vec::new()
        };
        let wanted: std::collections::BTreeSet<PathBuf> = on_screen
            .iter()
            .filter(|(_, _, showing)| *showing)
            .map(|(_, root, _)| root.clone())
            .collect();
        self.app.git_watch.sync(&wanted, &self.app.event_proxy);
        let due = self.app.git_watch.due(now);
        if due.is_empty() {
            return Ok(());
        }
        // Every surface showing one of those repositories, which is not the same
        // list as the roots: two columns and a graph can be looking at one
        // repository, and all three are about to be out of date together.
        let mut asked = false;
        for (origin, root, showing) in &on_screen {
            if *showing && due.contains(root) {
                asked |= self.reread_git_origin(origin, Ask::Settled);
            }
        }
        if asked && self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// **R31's third invalidation moment, carried out**: read every surface that
    /// this thing the user did could have changed.
    ///
    /// `standing_in` is the whole of the difference between the two automatic
    /// triggers — see [`git_surfaces_wanting_reread`]. Both go through
    /// [`Ask::Settled`]: neither is a gesture at this page, so neither may stack.
    pub(crate) fn reread_git_surfaces(&mut self, standing_in: Option<&[PathBuf]>) -> Result<()> {
        let surfaces = self.git_surfaces_on_screen();
        let wanted = git_surfaces_wanting_reread(self.git_panel_on(), &surfaces, standing_in);
        let mut asked = false;
        for origin in wanted {
            asked |= self.reread_git_origin(&origin, Ask::Settled);
        }
        if asked && self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// **What a move between windows that did not happen says** (multiwindow
    /// slice F1c; M147 — 「拒绝必须看得见」).
    ///
    /// Every one of [`TransferRefusal`]'s answers is an ordinary state of the
    /// world rather than a fault, and none of them is visible: a tab that stayed
    /// where it was looks exactly like a menu row nobody pressed. So each one
    /// gets a card, in this window, saying what is true — and no card says what
    /// the reader should do about it, because for the one refusal they can
    /// actually meet there is nothing they can do about it and inventing an
    /// instruction would be worse than the silence it replaced.
    ///
    /// **The corner and not a pane** ([`toast::ToastAnchor`]'s own fallback,
    /// spent deliberately). The subject of this card is a *tab*, and half the
    /// time it is a tab that is not on screen — a promoted pane lands in the
    /// strip without being activated (N157) — so there is no surface the card
    /// could stand on that would be the thing it is about.
    pub(crate) fn report_move_refusal(
        &mut self,
        refusal: &TransferRefusal,
        promoted: bool,
    ) -> Result<()> {
        self.toast(
            toast::ToastKind::Error,
            toast::ToastAnchor::Window,
            None,
            i18n::move_refusal_notice(&refusal.notice(), promoted),
        )
    }

    /// What the floating **Git pages** have not asked their repositories yet
    /// (user ruling, 2026-08-19).
    ///
    /// [`Self::ask_git_for_column`] for the windows that are not columns, and
    /// idempotent for its reason: the plan is derived from each cache
    /// ([`git::GitCache::pending_questions`]) rather than remembered here, so a
    /// slot already in flight or already answered asks for nothing however often
    /// this is called — which matters more here than there, because this rides
    /// the float clock and is called on every turn of the loop.
    ///
    /// **R31's gate is two conditions and it is the column's own** (§7.1.6g ②):
    /// the master switch is on *and* this window is actually standing on its Git
    /// page. A float on its tree costs exactly what a float cost before this
    /// slice — not one process — and a window that never turned to the page
    /// never reads a repository, however git-shaped the folder it is looking at.
    pub(in crate::runtime) fn ask_git_for_floats(&mut self) {
        // The float host first, because it is the cheaper of the two questions
        // and the one that is false far more often (closure review 2,
        // 2026-09-18).
        if self.window.float.is_empty() || !self.git_panel_on() {
            return;
        }
        // Which pool a document answer would land in. Read once, before the
        // host is walked, because it is a fact about the window and not about
        // any one float.
        let tab = self.window.tabs[self.window.active_tab].id;
        let window = self.window_id();
        let mut asks = Vec::new();
        for win in self.window.float.live_windows_mut() {
            let id = win.epoch;
            let Some(files) = win.files_mut() else {
                continue;
            };
            if files.files.view != seats::FilesView::Git {
                continue;
            }
            let root = files.files.root.clone();
            if root.trim().is_empty() {
                continue;
            }
            files.git.retarget(std::path::Path::new(&root));
            let questions = files.git.pending_questions();
            // Marked before any send, so a second turn of the loop on the same
            // frame sees questions already asked rather than asking them twice.
            for question in &questions {
                files.git.mark_pending(question);
            }
            asks.extend(questions.into_iter().map(|question| git::GitRequest {
                window,
                host: git::GitHost::Float { id, tab },
                question,
            }));
        }
        for ask in asks {
            if !self.app.git_worker.request(ask) {
                self.disable_git_worker();
                break;
            }
        }
    }

    /// **A Git row's body, pressed, in a floating window** (user ruling,
    /// 2026-08-19) — [`Self::press_git_row`] with the window for a column.
    ///
    /// The same three verbs the docked page has and through the same doors, so
    /// the two hosts cannot come to disagree about what a row does: the REMOTES
    /// sub-group opens and shuts, a changed file opens its diff, a commit turns
    /// its file list over, and a branch row is a checkout that answers the three
    /// tiers at [`Self::ask_to_checkout`].
    pub(in crate::runtime) fn press_float_git_row(
        &mut self,
        id: float::FloatId,
        index: usize,
    ) -> Result<()> {
        // A press does not reach the page's furniture — the docked page's own
        // first line, and it is first here for the same reason: the guard has to
        // stand ahead of the selection or the selection is the bug (user report,
        // 2026-08-25).
        if self
            .window
            .float_git_pages_shown
            .get(&id)
            .and_then(|page| page.rows.get(index))
            .is_none_or(git_panel::GitRow::is_furniture)
        {
            return Ok(());
        }
        // The selection follows the hand, before anything else this press could
        // mean — the docked page's own first line, **and onto an item rather
        // than onto a header** for the docked page's own reason (user report,
        // 2026-09-14): a window's `REMOTES (n)` is the same control wearing the
        // same one picture for the keyboard and the pointer, so a press that
        // seated the keyboard on it would take its hover away here too.
        if self
            .window
            .float_git_pages_shown
            .get(&id)
            .and_then(|page| page.rows.get(index))
            .is_some_and(git_panel::GitRow::seats_the_keyboard)
        {
            self.select_float_git_row(id, index);
        }
        let Some(root) = self
            .window
            .float
            .live(id)
            .and_then(float::FloatWin::files)
            .and_then(|files| files.git.root())
            .map(Path::to_path_buf)
        else {
            return Ok(());
        };
        let Some(row) = self
            .window
            .float_git_pages_shown
            .get(&id)
            .and_then(|page| page.rows.get(index))
            .cloned()
        else {
            return Ok(());
        };
        if git_panel::row_toggles_remotes(&row) {
            if let Some(files) = self
                .window
                .float
                .live_mut(id)
                .and_then(float::FloatWin::files_mut)
            {
                files.files.git_remotes_open = !files.files.git_remotes_open;
            }
            if self.refresh_chrome() {
                self.present_chrome_change()?;
            }
            return Ok(());
        }
        let Some(open) = git_panel::row_document(&row, &root) else {
            if self.refresh_chrome() {
                self.present_chrome_change()?;
            }
            return Ok(());
        };
        match open {
            git_panel::GitRowOpen::Document {
                source,
                name,
                renamed_from,
            } => self.open_float_git_document(id, source, name, renamed_from),
            git_panel::GitRowOpen::Expand { hash } => self.expand_float_commit(id, &hash),
            git_panel::GitRowOpen::Checkout { target, detach } => {
                let said = target.clone();
                let kind = if detach {
                    restore::GitCheckoutKind::Detach
                } else {
                    restore::GitCheckoutKind::Stand
                };
                self.ask_to_checkout(&GitOrigin::Float(id), root, target, said, kind)
            }
        }
    }

    /// Put this window's Git page selection on one row.
    fn select_float_git_row(&mut self, id: float::FloatId, index: usize) {
        if let Some(files) = self
            .window
            .float
            .live_mut(id)
            .and_then(float::FloatWin::files_mut)
        {
            files.files.git_sel = Some(index);
        }
    }

    /// A document opened from a floating Git page.
    ///
    /// [`Self::open_git_document`]'s two steps, with the window's host on the
    /// question: the seat is taken first and the content arrives afterwards,
    /// because a diff is not on a disk to be read synchronously. **The pane it
    /// lands on is the tab's**, which is the same answer the docked page gives —
    /// a window is where you read the repository from, not a second place for
    /// documents to live.
    fn open_float_git_document(
        &mut self,
        id: float::FloatId,
        source: preview::PreviewSource,
        name: String,
        renamed_from: Option<String>,
    ) -> Result<()> {
        let Some(surface) = self.preview_landing_surface() else {
            return Ok(());
        };
        self.open_preview_source_on(surface, source.clone(), name)?;
        let unread = self
            .preview_pool
            .get(&source)
            .is_some_and(|buffer| buffer.load == preview::PreviewLoad::Pending);
        if !unread {
            return Ok(());
        }
        let Some(question) = git_document_question(&source, renamed_from) else {
            return Ok(());
        };
        let tab = self.window.tabs[self.window.active_tab].id;
        if !self.app.git_worker.request(git::GitRequest {
            window: self.window_id(),
            host: git::GitHost::Float { id, tab },
            question,
        }) {
            self.disable_git_worker();
        }
        Ok(())
    }

    /// One of a floating Git row's verbs, pressed — [`Self::press_git_act`] with
    /// the window for a column, and the judgement is still
    /// [`git_panel::press_outcome`]'s alone.
    pub(in crate::runtime) fn press_float_git_act(
        &mut self,
        id: float::FloatId,
        index: usize,
        act: git_panel::GitAct,
    ) -> Result<()> {
        let Some(page) = self.window.float_git_pages_shown.get(&id) else {
            return Ok(());
        };
        let Some(row) = page.rows.get(index).cloned() else {
            return Ok(());
        };
        // The three verbs that are about no file at all, answered before a
        // pathspec is gathered because there is none to gather.
        match git_panel::press_outcome(act, false) {
            git_panel::GitPress::MoreCommits => return self.load_more_float_commits(id),
            // **The graph door works from a window too**, and it opens where
            // every graph opens: on a preview seat in the tab you are looking
            // at. A graph is a document about a repository (G-4) and has never
            // belonged to the surface that summoned it.
            git_panel::GitPress::Graph => return self.open_float_git_graph(id),
            git_panel::GitPress::Reread => {
                self.reread_git_origin(&GitOrigin::Float(id), Ask::Always);
                if self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
                return Ok(());
            }
            git_panel::GitPress::Write(_) | git_panel::GitPress::Gate => {}
        }
        let (paths, untracked) = match &row {
            git_panel::GitRow::Change(change) => (vec![change.path.clone()], change.untracked),
            git_panel::GitRow::Heading {
                group: Some(group), ..
            } => {
                let group = *group;
                let paths: Vec<String> = page
                    .rows
                    .iter()
                    .filter_map(|row| match row {
                        git_panel::GitRow::Change(change) if change.group == group => {
                            Some(change.path.clone())
                        }
                        _ => None,
                    })
                    .collect();
                (paths, group == crate::git::GitGroup::Untracked)
            }
            _ => return Ok(()),
        };
        if paths.is_empty() {
            return Ok(());
        }
        match git_panel::press_outcome(act, untracked) {
            git_panel::GitPress::Gate => {
                let Some(path) = paths.into_iter().next() else {
                    return Ok(());
                };
                self.raise_dirty_gate(restore::GateRequest::GitDiscard {
                    origin: GitOrigin::Float(id),
                    path,
                    untracked,
                })?;
                Ok(())
            }
            git_panel::GitPress::Write(verb) => {
                self.issue_git_write(&GitOrigin::Float(id), verb, paths)
            }
            git_panel::GitPress::MoreCommits => self.load_more_float_commits(id),
            git_panel::GitPress::Graph => self.open_float_git_graph(id),
            git_panel::GitPress::Reread => {
                self.reread_git_origin(&GitOrigin::Float(id), Ask::Always);
                if self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
                Ok(())
            }
        }
    }

    /// The graph door, from a floating page — the same document on the same kind
    /// of seat [`Self::open_git_graph`] opens it on.
    fn open_float_git_graph(&mut self, id: float::FloatId) -> Result<()> {
        let Some(root) = self
            .window
            .float
            .live(id)
            .and_then(float::FloatWin::files)
            .and_then(|files| files.git.root())
            .map(Path::to_path_buf)
        else {
            return Ok(());
        };
        let source = preview::PreviewSource::GitGraph { root: root.clone() };
        let name = root.file_name().map_or_else(
            || root.to_string_lossy().into_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        let active = self.window.active_tab;
        self.window.tabs[active]
            .git_graphs
            .entry(root)
            .or_insert_with_key(|root| git_graph::GraphState::new(root.clone()));
        let Some(surface) = self.preview_landing_surface() else {
            return Ok(());
        };
        self.open_preview_source_on(surface, source, name)
    }

    /// Whether this window is standing on its **Git page** this frame.
    ///
    /// The float's answer to [`column_keyboard`]'s question, and it is made of
    /// the same two facts a column's `git_pages_shown` entry is: the master
    /// switch, and the page the view is on. A window has no seat layout to be
    /// collapsed out of and no tab to be switched away from, so for a float
    /// those two are the whole of it.
    ///
    /// One reader for the paint, the hit test, the wheel and — the day a float
    /// takes the keyboard — the keys, which is exactly the arrangement
    /// `git_pages_shown` gives a column and exactly what the docked bug was the
    /// absence of.
    pub(in crate::runtime) fn float_shows_git_page(&self, id: float::FloatId) -> bool {
        self.window
            .float
            .drawn()
            .find(|win| win.epoch == id)
            .and_then(float::FloatWin::files)
            .is_some_and(|files| float_git_page_shown(self.git_panel_on(), files.files.view))
    }

    /// Build one floating commit graph, draw it, and record what it drew.
    ///
    /// [`Self::push_float_git_page`]'s twin one tenant along, down to the record
    /// it keeps and why: the hit test, the wheel and the keyboard all read the
    /// picture that was *drawn* rather than rebuilding one under the pointer, so
    /// a press lands on the row it landed on. The record is the floating half of
    /// `git_graphs_shown`, which is the same map the docked graphs are in —
    /// because a graph is a document, and a document's address is its surface.
    pub(in crate::runtime) fn push_float_graph(
        &mut self,
        id: float::FloatId,
        body: [f32; 4],
        scale: f32,
    ) -> float::FloatBody {
        let surface = PreviewSurface::Float(id);
        let Some(preview::PreviewSource::GitGraph { root }) = self
            .preview_buffer_on(surface)
            .map(|buffer| buffer.source.clone())
        else {
            return float::FloatBody::default();
        };
        let Some(content) = self.build_git_graph(surface, &root, body, scale) else {
            return float::FloatBody::default();
        };
        let hover = self
            .window
            .float_hover
            .filter(|(hovered, _)| *hovered == id)
            .map(|(_, part)| part);
        let palette = bt_render::chrome_palette();
        let (mut quads, mut labels, mut sprites) = (Vec::new(), Vec::new(), Vec::new());
        git_graph::push_graph(
            body,
            &content,
            float_graph_hover(hover),
            scale,
            &palette,
            (&mut quads, &mut labels, &mut sprites),
        );
        self.window.git_graphs_shown.insert(surface, content);
        float::FloatBody {
            // The same lift the tree tenant's body takes: chrome fills are opaque
            // by construction, so alpha 1.0 and the layer's own opacity carries
            // the fade for all three channels at once.
            quads: quads
                .into_iter()
                .map(|quad: bt_render::ChromeQuad| bt_render::OverlayQuad {
                    rect: quad.rect,
                    color: quad.color,
                    alpha: 1.0,
                })
                .collect(),
            labels,
            sprites,
        }
    }

    /// Build one floating Git page, draw it, and record what it drew.
    ///
    /// The record is [`WindowRuntime::float_git_pages_shown`] and it is owed for
    /// `git_pages_shown`'s reason exactly: the hit test is `&self` by
    /// construction and cannot measure a string, so a press lands on the row
    /// that was drawn because it **is** the row that was drawn.
    pub(in crate::runtime) fn push_float_git_page(
        &mut self,
        id: float::FloatId,
        body: [f32; 4],
        hover: Option<float::FloatPart>,
        scale: f32,
        palette: &bt_render::ChromePalette,
        out: (
            &mut Vec<bt_render::ChromeQuad>,
            &mut Vec<bt_render::ChromeLabel>,
            &mut Vec<marks::ChromeSprite>,
        ),
    ) {
        // Everything the build needs, lifted out of the host first: the measurer
        // below wants the renderer, and a borrow of the float would be fighting
        // it for `self`.
        let Some((cache, expanded, remotes_open, stored, sel)) = self
            .window
            .float
            .drawn()
            .find(|win| win.epoch == id)
            .and_then(float::FloatWin::files)
            .map(|files| {
                (
                    files.git.clone(),
                    files.files.git_expanded.clone(),
                    files.files.git_remotes_open,
                    files.git_scroll,
                    files.files.git_sel,
                )
            })
        else {
            return;
        };
        let mut content = {
            let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
            let mut measure = |text: &str, size: f32, face: git_panel::MeasureFace| {
                renderer.measure_chrome_label(
                    gpu,
                    text,
                    size,
                    face.weight,
                    face.letter_spacing_em,
                    face.tabular_numerals,
                )
            };
            git_panel::build(
                &cache,
                git_panel::GitPanelLook {
                    expanded: expanded.as_deref(),
                    remotes_open,
                },
                scale,
                git_panel::empty_width(body, scale),
                &mut measure,
            )
        };
        // R2 乙案, one host along: the painter believes the stored scroll, so a
        // page that got shorter is answered here rather than by the window
        // quietly disagreeing with the number it was handed. The healed number
        // goes back where it came from, so the picture and the window cannot
        // disagree even on the frame the heal happens.
        content.scroll_px = git_panel::clamp_git_scroll(body, &content, stored, scale);
        content.selected = git_panel::clamp_git_selection(&content.rows, sel);
        if let Some(files) = self
            .window
            .float
            .live_mut(id)
            .and_then(float::FloatWin::files_mut)
        {
            files.git_scroll = content.scroll_px;
        }
        git_panel::push_git_panel(body, &content, float_git_hover(hover), scale, palette, out);
        self.window.float_git_pages_shown.insert(id, content);
    }

    /// The same notch, one page along — [`Self::scroll_git_panel`] in a window.
    ///
    /// Its own function rather than a branch inside the tree's, for the docked
    /// pair's stated reason: the two lists have different row heights and
    /// different bounds and share only the rectangle. The bound is read from the
    /// page that was actually **drawn** (`float_git_pages_shown`) and never from
    /// a recomputation.
    pub(in crate::runtime) fn scroll_float_git_page(
        &mut self,
        id: float::FloatId,
        delta: MouseScrollDelta,
    ) -> Result<()> {
        let Some((geometry, _)) = self.float_geometry_of(id) else {
            return Ok(());
        };
        let body = geometry.body;
        let travel = self.vertical_wheel_travel(delta, body[3] - body[1]);
        let scale = self.window.renderer.scale_factor() as f32;
        let Some(page) = self.window.float_git_pages_shown.get(&id) else {
            return Ok(());
        };
        let stored = self
            .window
            .float
            .live(id)
            .and_then(float::FloatWin::files)
            .map_or(0.0, |files| files.git_scroll);
        let scrolled = git_panel::clamp_git_scroll(body, page, stored - travel, scale);
        if scrolled == stored {
            return Ok(());
        }
        if let Some(files) = self
            .window
            .float
            .live_mut(id)
            .and_then(float::FloatWin::files_mut)
        {
            files.git_scroll = scrolled;
        }
        // The rows under a still pointer changed, so the hover row did too.
        if let Some(position) = self.window.pointer_position {
            self.window.float_hover = self.float_hit_at(position);
        }
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The same notch, one page along.
    ///
    /// Its own function rather than a branch inside the tree's, because the two
    /// lists have different row heights and different bounds and share only the
    /// rectangle — and because the bound has to be read from the page that is
    /// actually drawn, which is `git_pages_shown` and not a recomputation.
    pub(in crate::runtime) fn scroll_git_panel(
        &mut self,
        seat: SeatId,
        body: [f32; 4],
        delta: MouseScrollDelta,
    ) -> Result<()> {
        let travel = self.vertical_wheel_travel(delta, body[3] - body[1]);
        let scale = self.window.renderer.scale_factor() as f32;
        let Some(page) = self.window.git_pages_shown.get(&seat) else {
            return Ok(());
        };
        let active = self.window.active_tab;
        let stored = self.window.tabs[active]
            .git_scroll
            .get(&seat)
            .copied()
            .unwrap_or(0.0);
        let scrolled = git_panel::clamp_git_scroll(body, page, stored - travel, scale);
        if scrolled == stored {
            return Ok(());
        }
        self.window.tabs[active].git_scroll.insert(seat, scrolled);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Ask again what the pointer is on, when the Git page it was on has been
    /// rebuilt under it (user report, 2026-09-14).
    ///
    /// [`Self::heal_files_scroll`]'s twin, and it is owed for the same reason
    /// one state along: **a number that was true when it was written stops being
    /// true when the list changes under it.** The hover this window keeps is a
    /// [`seats::ChromeTarget::GitRow`], and a `GitRow` is an *index* — the very
    /// thing [`git_panel::GitRowPeek`] refuses to be keyed by, in a doc that
    /// says why: an index is stale the moment a group above it grows a file.
    /// Nothing but a pointer event wrote the hover, so a wheel notch over the
    /// column, a `git add` the watcher picked up, or a commit turning over all
    /// moved the rows under a hand that never moved — and whatever row inherited
    /// the old number lit up under a pointer that was somewhere else. The
    /// reported picture was `REMOTES (5)` wearing a hover it had never been
    /// given (`git_panel::GitRow::wears_ground` has the other half of that
    /// report, the half a wash on a *header* was).
    ///
    /// **Re-derived, not cleared.** The state's law is "the hover is what the
    /// pointer is on", and clearing it would be a second, weaker law — the row
    /// genuinely under the hand would go dark for as long as the reader held
    /// still. This asks [`seats::hit_git_panel`] the question the pointer's own
    /// handler asks, against the pages this pass is about to draw, so the answer
    /// is right whether the rows moved, changed or went away entirely. `None` is
    /// then a fact and not a reset: the pointer is over no row of this page.
    ///
    /// **Only when the stored hover is already this page's.** Everything else
    /// the pointer can be on is chrome whose geometry this pass did not rebuild,
    /// and re-running the whole router here would be this function answering for
    /// surfaces it knows nothing about — including a float's, which is opaque to
    /// the docked hit test and is what the 2026-09-12 ruling put in
    /// [`Self::pointer_target_at`].
    ///
    /// The floating host has the same two wheels and already re-asks on both
    /// (`scroll_float_tree`, `scroll_float_git_page`); its pages are rebuilt in
    /// `float_layer`, which runs after this and does its own asking.
    pub(in crate::runtime) fn heal_git_hover(&mut self, scale: f32) {
        let Some(seats::ChromeTarget::GitRow { .. } | seats::ChromeTarget::GitAct { .. }) =
            self.window.seat_pointer.hover
        else {
            return;
        };
        // No pointer in this window at all: `pointer_left` has already taken the
        // hover with it, and a page rebuilt after that has nothing to heal.
        let Some(position) = self.window.pointer_position else {
            return;
        };
        let healed = seats::hit_git_panel(
            &self.seat_layout,
            &self.window.git_pages_shown,
            scale,
            position.x,
            position.y,
        );
        self.window.seat_pointer.hover = healed;
    }

    /// **Go and photograph the pages the cards could not draw** — the web half
    /// of [`Self::arm_card_reads`], and the whole of W2 slice ⑥'s asking side
    /// (§7.11).
    ///
    /// Split out for the same reason its two siblings are: the walk above holds
    /// the tab list immutably and every one of these calls is `&mut`. The
    /// *decision* is not here — `web_thumb::WebThumbs::due` is a pure function
    /// over facts read off each engine, and every refusal it can make is red
    /// tested without a browser. What is here is the four things that need the
    /// window: the engine to ask, the error to report, and nothing else.
    ///
    /// The whole of the cost this adds to a frame is one syscall per seat asked,
    /// measured at **0.115 ms** and made at most once every
    /// `web_thumb::CAPTURE_INTERVAL` (gate 11). The answer arrives on a later
    /// turn of the loop and is decoded on another thread.
    pub(crate) fn photograph_pages(&mut self, demands: Vec<web_thumb::PageDemand>, now: Instant) {
        if demands.is_empty() {
            return;
        }
        for leaf in self.window.web_thumbs.due(&demands, now) {
            if let Some(web) = self.window.web.get_mut(&leaf)
                && let Err(error) = web.capture_page()
            {
                eprintln!("BT_WEB capture failed: {error}");
            }
        }
    }

    pub(crate) fn service_ime_report(&mut self, now: Instant) -> Option<Instant> {
        if self.window.ime_report_due.is_some_and(|due| now >= due) {
            self.window.ime_report_due = None;
            self.write_ime_observation("first-focus+1s");
        }
        self.window.ime_report_due
    }
}
