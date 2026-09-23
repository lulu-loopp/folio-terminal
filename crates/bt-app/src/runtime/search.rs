//! `search` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{
    Runtime, SearchScanCache, frame_row_of_anchor, hang_watch, input, marks, native_window, search,
    seats, text_field, tooltip, trace_sink,
};
use anyhow::Result;
use bt_layout::SeatId;
use std::sync::Arc;
use std::time::Instant;
use winit::dpi::PhysicalPosition;
use winit::event::{Ime, KeyEvent};
use winit::keyboard::{Key, NamedKey};

impl Runtime<'_> {
    /// Note which rail the pointer is on, open or fold the bucket under it, and
    /// aim every rail's clocks at where the hand has left them. Returns whether
    /// the rail owns this pointer — a press there is a jump and not a selection.
    ///
    /// **One hot rail at a time** (mock 8478-8483): the mock-up's `pointermove` is
    /// delegated on `#termhost` and resets every *other* rail before it touches
    /// the one under the pointer, so two panes can never both be lit. Here that is
    /// the cooling loop below, and it is also the whole of `pointerleave` — a
    /// `None` position cools everything, which is what [`Self::pointer_left`]
    /// calls it with.
    /// Which of the capsule's controls the pointer is on, and whether it is on the capsule at all.
    ///
    /// The `bool` is what the routing below it reads: a pointer standing on the capsule is not
    /// standing on the rail, on a hyperlink or on a cell, and the pane must not be told about it
    /// (R5 — the two surfaces share the pane's top-right corner and a short pane puts the rail's
    /// own block under the capsule's box).
    pub(in crate::runtime) fn drive_search_hover(
        &mut self,
        position: Option<PhysicalPosition<f64>>,
    ) -> Result<bool> {
        let hover = position
            .zip(self.window.search_layout)
            .and_then(|(at, capsule)| search::hit(&capsule, at.x as f32, at.y as f32));
        if self.window.search_hover != hover {
            self.window.search_hover = hover;
            if self.refresh_overlay() {
                self.present_chrome_change()?;
            }
        }
        Ok(hover.is_some())
    }

    /// The tip anchors the capsule's own controls register (B66's `title=` texts).
    ///
    /// The field and the capsule's padding register nothing: a box you are typing into does not
    /// need a sentence about what it is, and the placeholder already says `Find`. The six that do
    /// are marks and two-letter labels — `Aa`, `ab`, `.*`, a chevron each way and a cross — which
    /// is exactly the case a tip exists for: an idiom is a guess until something says what it does.
    pub(in crate::runtime) fn search_tip_anchors(&self, anchors: &mut tooltip::TooltipAnchors) {
        let Some(capsule) = self.window.search_layout else {
            return;
        };
        for element in [
            search::SearchElement::Toggle(search::SearchFlag::Case),
            search::SearchElement::Toggle(search::SearchFlag::Word),
            search::SearchElement::Toggle(search::SearchFlag::Regex),
            search::SearchElement::Previous,
            search::SearchElement::Next,
            search::SearchElement::Close,
        ] {
            let rect = match element {
                search::SearchElement::Toggle(flag) => capsule.toggle(flag),
                search::SearchElement::Previous => capsule.previous,
                search::SearchElement::Next => capsule.next,
                _ => capsule.close,
            };
            anchors.push(
                tooltip::TooltipAnchorId::SearchControl(element),
                rect,
                search::tip_text(element),
            );
        }
    }

    /// Where the capsule stands this frame, or `None` when there is nothing to stand on.
    ///
    /// Re-laid every frame rather than cached, and the reason is the counter: `1/17` and `10/17`
    /// are different widths, so the capsule's own width is a function of what it is saying. It is
    /// four `max`es and one row of additions — cheaper than deciding whether it is stale.
    pub(in crate::runtime) fn search_capsule(&mut self) -> Option<search::Capsule> {
        let seat = self.window.search.seat()?;
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let (rect, head) = seats::search_capsule_host(&self.seats, &self.seat_layout, seat, scale)?;
        let counter = self.window.search.counter();
        let width = self.window.renderer.measure_chrome_text(
            &mut self.app.gpu,
            &counter,
            search::COUNTER_FONT_LOGICAL_PX * scale,
        ) + 2.0 * search::COUNTER_PADDING_X_LOGICAL_PX * scale;
        Some(search::lay_out(rect, head, scale, width))
    }

    /// Which toggles the capsule's current host can honour (§7.7 ②).
    ///
    /// A terminal answers all three — the regex engine behind it is this
    /// product's own. A page answers the case fold and no more:
    /// `ICoreWebView2FindOptions` carries a term, a case fold and a
    /// highlight-all, and neither a word boundary nor a pattern appears
    /// anywhere in that interface.
    fn search_flags_offered(&self) -> search::SearchFlags {
        let on_a_page = self
            .window
            .search
            .seat()
            .is_some_and(|seat| self.seat_holds_a_page(seat));
        search::SearchFlags {
            case_sensitive: true,
            whole_word: !on_a_page,
            regex: !on_a_page,
        }
    }

    /// `Ctrl+F` / `Ctrl+Shift+F`, and the `Find…` a menu row will send here.
    ///
    /// *"Reopening refocuses the capsule and keeps the last query: search is a staying state, not a
    /// popup"* (B80). So this is one verb for both cases — a fresh capsule and a re-focus — and the
    /// difference between them is entirely inside [`search::SearchState::open`].
    pub(in crate::runtime) fn open_search(&mut self, seat: SeatId) -> Result<()> {
        if !self.seat_can_search(seat) {
            return Ok(());
        }
        // Opening on a second pane closes the first, which is what "one search, one pane" means at
        // the point it is enforced: the old pane's highlights have to go before the new pane's
        // arrive, or a window with two panes would end up wearing two sets.
        if let Some(previous) = self
            .window
            .search
            .seat()
            .filter(|previous| *previous != seat)
        {
            self.clear_search_highlights(previous);
        }
        self.window.search.open(seat);
        // Which kind of host is under it, so the counter knows whether an empty
        // tally means "none" or "nobody has been asked".
        self.window
            .search
            .set_counts_its_own(self.seat_holds_a_page(seat));
        // **The capsule takes the keyboard back from a page** (§7.7 ②), and it
        // has to for the address field's reason one door over: a page keeps
        // every key this window's table does not claim, and §7.8 ④ measured that
        // **a bare printable key never enters `AcceleratorKeyPressed` at all**.
        // A field raised over a page that still holds the keys is a field you
        // can open, switch and close, and never type in.
        //
        // Only over a page. A terminal never had the Win32 focus to begin with,
        // and calling `SetFocus` on the window it is already in would be this
        // window taking something from itself.
        if self.seat_holds_a_page(seat)
            && let Ok(native) = native_window(&self.window.window)
        {
            let _ = bt_platform::take_keyboard_focus(native);
        }
        self.refresh_search(true)?;
        self.after_search_change()
    }

    /// Put the capsule away. Returns whether there was one, so Esc's ladder can tell whether this
    /// rung answered.
    ///
    /// **Nothing scrolls** (B63) and **the query stays** (B62, D-8): what is emptied is the hit
    /// set, the current match and the seat, and what survives is every character typed and every
    /// toggle switched. That is what makes `Ctrl+F` after a tab switch a continuation rather than a
    /// fresh start.
    pub(in crate::runtime) fn close_search(&mut self) -> Result<bool> {
        let Some(seat) = self.window.search.seat() else {
            return Ok(false);
        };
        self.clear_search_highlights(seat);
        // A page keeps its own highlights, so putting the capsule away has to
        // tell the engine as well — otherwise the marks stay on a document
        // nobody is searching any more.
        if let Some(web) = self.web_on_mut(seat)
            && let Err(error) = web.find_stop()
        {
            eprintln!("BT_WEB {error}");
        }
        self.window.search.close();
        self.window.search_layout = None;
        self.window.search_hover = None;
        self.window.search_scan = None;
        self.after_search_change()?;
        Ok(true)
    }

    /// Take the highlights off one pane and repaint it.
    pub(in crate::runtime) fn clear_search_highlights(&mut self, seat: SeatId) {
        if let Some(leaf) = self.sessions.get_mut(&seat) {
            leaf.projection.set_search_highlights(None);
        }
    }

    /// One frame's worth of everything a **reader-caused** change to the search owes the window:
    /// the searched pane is repainted, and the chrome is rebuilt around the new count.
    ///
    /// **Only ever called for a change the reader made.** The rebuilds that *output* causes go
    /// through [`Self::refresh_search`]'s quiet path instead, and the difference is not a
    /// preference: that path runs at the top of [`Self::publish_frame_inner`], with a frame already
    /// being composed, so asking for another one from inside it would be the publish re-entering
    /// itself once per line the shell prints.
    pub(crate) fn after_search_change(&mut self) -> Result<()> {
        if let Some(seat) = self.window.search.seat() {
            self.repaint_pane_change(seat)?;
        } else {
            self.publish_interaction_frame()?;
        }
        // **The chrome as well as the overlay, since §7.1.6i.** Which seat the
        // capsule stands on is a *chrome* fact now: a lone pane's corner ghost
        // shares the capsule's corner and its lane and stands down whole while
        // one is up, so the pass that draws it has to be re-run on both edges of
        // the capsule's life. Before this the chrome was untouched here, and a
        // ghost would have sat under an open capsule until some unrelated event
        // happened to rebuild the layer.
        //
        // Both rebuilds run, and then one present: `||` would skip the overlay
        // on every frame the chrome happened to move.
        let chrome_changed = self.refresh_chrome();
        let overlay_changed = self.refresh_overlay();
        if chrome_changed || overlay_changed {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The same, for a rebuild that arrived under a frame already being built.
    ///
    /// The pane needs nothing: the highlights were installed on its projection a moment ago and the
    /// frame about to be projected will carry them. The *chrome* does need rebuilding, because the
    /// counter is part of it and a line freezing can change `3/17` to `3/18` — but it is rebuilt
    /// without asking for a present, since the present is already on its way.
    fn after_quiet_search_change(&mut self) {
        let _ = self.refresh_overlay();
    }

    /// Re-scan, if anything the answer depends on has moved.
    ///
    /// `forced` says the reader did something — typed, flipped a toggle, opened the capsule — and
    /// suspends the "nothing changed, leave it alone" shortcut. Its other half is B58's rule about
    /// *which* match becomes current: a rebuild the reader caused starts from where the eye is,
    /// while one that output caused keeps the match the eye is on.
    ///
    /// **No debounce anywhere.** A keystroke's result is on the next frame; the thing that makes
    /// that affordable is [`SearchScanCache`]'s split by plane, not a timer.
    pub(in crate::runtime) fn refresh_search(&mut self, forced: bool) -> Result<()> {
        let Some(seat) = self.window.search.seat() else {
            self.window.search_scan = None;
            return Ok(());
        };
        // **The page's own branch, and it stops here** (§7.7 ②). Everything
        // below this line reads a transcript: frozen lines, a live grid, a scan
        // cache keyed on a source generation. A document inside an engine has
        // none of those, and the engine counts its own matches — so what this
        // host owes is the term and the one flag the engine can honour, and the
        // count arrives later as `WebOutcome::FindMatches`.
        if self.seat_holds_a_page(seat) {
            // **Typing does not search a page** (§7.7 ②, ruled 2026-08-22 on the
            // machine). A terminal's capsule searches on every keystroke because
            // the search is this window's own regex over its own transcript and
            // touches nothing; `ICoreWebView2Find::Start` **moves the keyboard
            // into the page**, and there is no option that says otherwise — so a
            // live find over a page is a field that takes one character and then
            // types into the document. What runs the find is the ask: `Enter`,
            // the two walk buttons, `F3`. See `WebSeat::find_or_step`.
            //
            // What a keystroke *does* do is take the last answer down: a tally
            // is about a term, and the term has just changed.
            let query = self.window.search.query().to_owned();
            // **Only when the term has actually moved.** This runs on the quiet
            // path too — once per published frame — and a tally forgotten every
            // frame is a counter that can never be filled in (found on the
            // machine, 2026-08-22: the matches lit up and the count stayed
            // blank). What the seat was asked is the one place that knows.
            let asked = self
                .web_on(seat)
                .map(|web| web.found().to_owned())
                .unwrap_or_default();
            if asked != query {
                self.window.search.forget_engine_matches();
            }
            if query.is_empty()
                && let Some(web) = self.web_on_mut(seat)
                && let Err(error) = web.find_stop()
            {
                eprintln!("BT_WEB {error}");
            }
            self.window.search_scan = None;
            return self.settle_search_change(forced);
        }
        // A pane that has gone — closed, torn into another tab, turned into a preview — ends the
        // search silently (B64/B77). There is nothing to search and nothing to draw on.
        if !self.sessions.contains_key(&seat) {
            self.close_search()?;
            return Ok(());
        }
        // A program that took the alternate screen while the capsule was up takes the capsule with
        // it (D-5, R3). The query survives, exactly as a tab switch leaves it.
        if !self.seat_can_search(seat) {
            self.close_search()?;
            return Ok(());
        }
        let compiled = match search::engine(self.window.search.flags(), self.window.search.query())
        {
            Ok(compiled) => Some(compiled),
            // An empty query is the state the capsule opens in, not a fault: no hits, no count, no
            // red. A pattern the engine refused is the red one, and its own message is the tip.
            Err(bt_transcript::search::SearchError::Empty) => None,
            Err(bt_transcript::search::SearchError::Pattern(message)) => {
                let changed =
                    !self.window.search.hits().is_empty() || self.window.search.error().is_none();
                self.window
                    .search
                    .install(Vec::new(), Some(message), false, None);
                self.window.search_scan = None;
                if changed {
                    self.install_search_highlights(seat);
                    self.settle_search_change(forced)?;
                }
                return Ok(());
            }
        };
        let Some(compiled) = compiled else {
            let changed =
                !self.window.search.hits().is_empty() || self.window.search.error().is_some();
            self.window.search.install(Vec::new(), None, false, None);
            self.window.search_scan = None;
            if changed {
                self.install_search_highlights(seat);
                self.settle_search_change(forced)?;
            }
            return Ok(());
        };

        let revision = self.window.search_revision;
        let leaf = self.sessions.get(&seat).expect("the seat was just checked");
        let transcript = leaf.session.transcript();
        // **The previous answer, and only while it is an answer to the same question.** Seat and
        // revision are what "the same question" means here; the plane having moved under it is not
        // a reason to throw it away, it is the reason it is passed in.
        let previous = self
            .window
            .search_scan
            .as_ref()
            .filter(|cache| cache.seat == seat && cache.revision == revision);
        let scan_started = self.app.trace_perf.then(Instant::now);
        // **The pattern over the frozen lines, under its own name** (T-STATION-SPLIT) — see
        // [`hang_watch::Station::SearchScan`]. Bracketed around the scan itself and not around
        // this function, which leaves through eight doors; there is no early return between
        // these two lines.
        let leaving_history = hang_watch::enter(hang_watch::Station::SearchScan);
        let history =
            search::scan_history_after(&compiled, transcript, previous.map(|cache| &cache.history));
        hang_watch::at(leaving_history);
        let history_us = scan_started.map_or(0, |at| at.elapsed().as_micros());
        // The two volatile planes, every time: fifty rows of grid and whatever has scrolled out but
        // not frozen. Their cost is a property of the screen, so re-scanning them unconditionally
        // is what buys "the word you are typing is findable the instant it is echoed".
        let live: Vec<search::LiveRow> = leaf
            .session
            .live_rows()
            .iter()
            .enumerate()
            .map(|(row, captured)| search::live_row(row as u32, &captured.cells))
            .collect();
        let leaving_scan = hang_watch::enter(hang_watch::Station::SearchScan);
        let volatile_hits =
            search::scan_volatile(&compiled, transcript, &live, leaf.session.grid_generation());
        hang_watch::at(leaving_scan);
        // **What the scan cost, and how much of it was new** — one line per frame the capsule is
        // open on a terminal. `lines_scanned` is the number this split exists to hold down: the
        // whole plane on the frame a question changes, and the lines the shell has frozen since on
        // every other one. A recording where it tracks `frozen_lines` while a shell prints is the
        // incremental step having been lost.
        if self.app.trace_perf {
            trace_sink::stderr_line(format!(
                "BT_PERF_TRACE search_scan lines_scanned={} frozen_lines={} history_us={history_us} history_hits={} live_rows={} volatile_hits={}",
                history.lines_scanned,
                history.scan.window().len,
                history.scan.hits().len(),
                live.len(),
                volatile_hits.len(),
            ));
        }
        // Nothing happened when the question, the plane's window and the volatile hits are all the
        // ones the last scan saw. The window stands in for the history hits because it is what they
        // are a function of: same seat, same revision, same window, same answer.
        let unchanged = previous.is_some_and(|cache| {
            cache.history.window() == history.scan.window() && cache.volatile_hits == volatile_hits
        });
        if unchanged && !forced {
            return Ok(());
        }
        // Where the eye is: the viewport's own anchor when the pane has been scrolled, and nothing
        // when it is riding the live bottom — where "the first match at or below the top" is the
        // first match of all.
        let from = leaf
            .projection
            .scroll_anchor()
            .map(|anchor| anchor.source.clone());
        let mut hits = history.scan.hits().to_vec();
        hits.extend(volatile_hits.iter().cloned());
        self.window
            .search
            .install(hits, None, !forced, from.as_ref());
        self.window.search_scan = Some(SearchScanCache {
            seat,
            revision,
            history: history.scan,
            volatile_hits,
        });
        self.install_search_highlights(seat);
        self.settle_search_change(forced)
    }

    /// Which of the two roads a finished rebuild takes back to the glass.
    fn settle_search_change(&mut self, forced: bool) -> Result<()> {
        if forced {
            self.after_search_change()
        } else {
            self.after_quiet_search_change();
            Ok(())
        }
    }

    /// Hand the searched pane's projection the hit set it paints from.
    pub(crate) fn install_search_highlights(&mut self, seat: SeatId) {
        let highlights = Arc::clone(self.window.search.highlights());
        if let Some(leaf) = self.sessions.get_mut(&seat) {
            leaf.projection.set_search_highlights(Some(highlights));
        }
    }

    /// `Enter` / `F3` / the `▲▼` buttons — walk one match, wrapping at both ends.
    pub(in crate::runtime) fn step_search(&mut self, forwards: bool) -> Result<()> {
        // On a page the walk is the engine's: it owns the highlights, the
        // scroll and which match is current, and it reports the new tally back
        // through `WebOutcome::FindMatches`.
        if let Some(seat) = self.window.search.seat()
            && self.seat_holds_a_page(seat)
        {
            let query = self.window.search.query().to_owned();
            let case_sensitive = self.window.search.flags().case_sensitive;
            if let Some(web) = self.web_on_mut(seat)
                && let Err(error) = web.find_or_step(&query, case_sensitive, forwards)
            {
                eprintln!("BT_WEB {error}");
            }
            return Ok(());
        }
        if self.window.search.step(forwards).is_none() {
            return Ok(());
        }
        self.after_current_hit_moved()
    }

    /// **A press on a match tick of the results rail** (B40-B41, S4).
    ///
    /// Neither a step forwards nor a step back — *"this one"* — which is why
    /// `SearchState` has had `set_current` since S3 waiting for exactly this
    /// caller. Everything after the selection is the walk's own tail, shared
    /// below rather than restated, so a tick and the `▼` button leave the window
    /// in states that cannot drift apart.
    ///
    /// An index the hit set no longer holds does nothing at all. It is reachable
    /// only through a rail built against a hit set that has since been replaced —
    /// one frame's worth of staleness at most — and moving the reader to a hit
    /// they did not point at would be worse than the press appearing not to land.
    pub(in crate::runtime) fn select_search_hit(&mut self, index: usize) -> Result<()> {
        if self.window.search.set_current(index).is_none() {
            return Ok(());
        }
        self.after_current_hit_moved()
    }

    /// Scroll to the current match — **but only if it is not already on screen** (B54).
    ///
    /// *"Typing toward a visible match must not yank the viewport."* So the question asked is about
    /// the picture on the glass, not about the projection's arithmetic: the pane's last presented
    /// frame is asked which row is showing this anchor, and a row that is wholly inside the pane is
    /// a row the reader can already read.
    ///
    /// When it does scroll, the match lands **a third of the way down** rather than at the top,
    /// which is the one place this differs from the command rail's jump: a command is the start of
    /// output you read downwards, and a match is a point you read *around*.
    pub(crate) fn reveal_current_search_hit(&mut self) -> Result<()> {
        let Some(seat) = self.window.search.seat() else {
            return Ok(());
        };
        let Some(anchor) = self.window.search.current().map(|hit| hit.anchor.clone()) else {
            return Ok(());
        };
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let Some(body) = seats::pane_body_viewport(&self.seats, &self.seat_layout, seat, scale)
        else {
            return Ok(());
        };
        let pane_height = i64::from(body.height) * bt_viewport::SUBPIXELS_PER_PX;
        let visible = self
            .pane_frame(seat)
            .and_then(|frame| frame_row_of_anchor(frame, &anchor))
            .is_some_and(|row| {
                search::row_is_wholly_visible(row.top_subpixels, row.height_subpixels, pane_height)
            });
        if visible {
            return Ok(());
        }
        let Some(leaf) = self.sessions.get_mut(&seat) else {
            return Ok(());
        };
        leaf.projection
            .set_scroll_anchor(Some(bt_viewport::ScrollAnchor {
                source: anchor,
                local_offset: search::landing_offset_subpixels(pane_height),
            }));
        // A hit scrolled to is a scroll, and the bar says where it landed
        // (P2-9 slice 1).
        self.wake_terminal_thumb(seat);
        Ok(())
    }

    /// One key, with the capsule's field holding the keyboard.
    ///
    /// Returns whether the key was the field's at all — **everything is** (B69: *"search keys are
    /// the search box's; the terminal must not hear them"*), which is why every branch below
    /// returns `true` and why this rung sits beside the files column's and the preview's rather
    /// than above the shortcut table: a field holding the keyboard is not a modal, so the window's
    /// own chords still work over it, and only *typing* is claimed.
    pub(in crate::runtime) fn search_field_key(&mut self, event: &KeyEvent) -> Result<bool> {
        if !self.window.search.is_focused() {
            return Ok(false);
        }
        use text_field::TextMove;
        let shift = self.window.modifiers.shift_key();
        // The application's modifier, whichever key it is printed on here
        // (M1-7): this box's own `Ctrl+A` and `Ctrl+F` are `Cmd+A` and `Cmd+F` on
        // a Mac, where `Ctrl+A` is the child's start-of-line and not a field's.
        let control = input::is_command_chord(self.window.modifiers);
        let mut edited = false;
        match &event.logical_key {
            // Enter walks the matches; `Shift+Enter` walks them backwards (B70). It is a walk and
            // not a commit because there is nothing to commit — the search is already live.
            // Repeats travel: holding Enter walks on through the matches, which
            // is what a held arrow does in every list in this window and what a
            // find bar does everywhere else. It is not a verb being repeated —
            // it is one continuous "further".
            Key::Named(NamedKey::Enter) => {
                self.step_search(!shift)?;
                return Ok(true);
            }
            // *"The box is single-line — vertical arrows are free to mean prev/next"* (B71, user
            // ask 2026-07-18). There is no line below to move to, so the key that would have been
            // wasted is given the verb the reader wants next.
            Key::Named(NamedKey::ArrowDown) => {
                self.step_search(true)?;
                return Ok(true);
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.step_search(false)?;
                return Ok(true);
            }
            Key::Named(NamedKey::Backspace) => edited = self.window.search.field_mut().backspace(),
            Key::Named(NamedKey::Delete) => edited = self.window.search.field_mut().delete(),
            Key::Named(NamedKey::ArrowLeft) => self.window.search.field_mut().step(
                if control {
                    TextMove::WordLeft
                } else {
                    TextMove::Left
                },
                shift,
            ),
            Key::Named(NamedKey::ArrowRight) => self.window.search.field_mut().step(
                if control {
                    TextMove::WordRight
                } else {
                    TextMove::Right
                },
                shift,
            ),
            Key::Named(NamedKey::Home) => {
                self.window.search.field_mut().step(TextMove::Home, shift)
            }
            Key::Named(NamedKey::End) => self.window.search.field_mut().step(TextMove::End, shift),
            // The space is text and answers the guard the character arm below
            // answers, for its reason (M1-7).
            Key::Named(NamedKey::Space) if input::types_a_character(self.window.modifiers) => {
                self.window.search.field_mut().insert(" ");
                edited = true;
            }
            Key::Character(text) if control => {
                match text.as_str() {
                    "a" | "A" => self.window.search.field_mut().select_all(),
                    // `Ctrl+F` with the caret already in the box selects what is there (B73), so
                    // the chord means the same thing wherever it is pressed: "put me in the search,
                    // ready to replace the query".
                    "f" | "F" => self.window.search.field_mut().select_all(),
                    _ => {}
                }
            }
            // **`Alt+C` / `Alt+W` / `Alt+R` — the three toggles from the keyboard**, which is what
            // VS Code's find bar binds and what a reader who never takes their hands off the keys
            // needs: the mock-up gives the toggles a click and nothing else (B74), so there is no
            // prototype rule to follow here and the reference product's is taken.
            //
            // **Not rows of `shortcuts::BINDINGS`**, and for `graph_key_of`'s stated reason: that
            // table is the chord registry the future editing panel edits, and every row in it is a
            // chord this window *claims* from the shell. These three are claimed from nothing —
            // they exist only while the capsule's field holds the keyboard, at which point there
            // is no shell listening — exactly as the graph's own six keys and the files column's
            // arrows are out of the table. Putting one of the three families in and not the others
            // would be the audit saying two things.
            Key::Character(text) if self.window.modifiers.alt_key() && !control => {
                if let Some(flag) = search::toggle_for_letter(text) {
                    self.toggle_search_flag(flag)?;
                }
                return Ok(true);
            }
            // **A chord is not text** (M1-7, X-3 §4 ③). The arm above answers
            // this platform's application modifier and the guard here answers
            // the other one, so neither a `Cmd`-modified key on a Mac nor a
            // `Win`-modified key here types its letter into the box — which is
            // what `Cmd+C` did, measured, into three of this window's fields.
            Key::Character(text) if input::types_a_character(self.window.modifiers) => {
                self.window.search.field_mut().insert(text);
                edited = true;
            }
            // Everything else is swallowed. A key the field has no use for is still not the
            // shell's while the caret is in the field.
            _ => {}
        }
        if edited {
            self.window.search_revision = self.window.search_revision.wrapping_add(1);
            self.refresh_search(true)?;
        }
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// Flip one toggle and re-ask (`Aa` / `ab` / `.*`).
    fn toggle_search_flag(&mut self, flag: search::SearchFlag) -> Result<()> {
        // A toggle the host cannot honour is not switched, and the press is the
        // whole of what happens: the box is drawn in the surface's structural
        // ink rather than a verb's, so what is refused here is what the capsule
        // already said it could not do.
        if !self.search_flags_offered().is_on(flag) {
            return Ok(());
        }
        self.window.search.flags_mut().toggle(flag);
        self.window.search_revision = self.window.search_revision.wrapping_add(1);
        // *"Any press hands the caret back"* (B74) — the capsule is one control, and a toggle you
        // pressed with the mouse leaves you able to keep typing.
        self.window.search.focus();
        self.refresh_search(true)
    }

    /// A press on the capsule. Returns whether it landed there at all.
    pub(in crate::runtime) fn press_search(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let Some(capsule) = self.window.search_layout else {
            return Ok(false);
        };
        let Some(element) = search::hit(&capsule, position.x as f32, position.y as f32) else {
            return Ok(false);
        };
        match element {
            search::SearchElement::Close => {
                self.close_search()?;
                return Ok(true);
            }
            search::SearchElement::Toggle(flag) => self.toggle_search_flag(flag)?,
            search::SearchElement::Previous => {
                self.window.search.focus();
                self.step_search(false)?;
            }
            search::SearchElement::Next => {
                self.window.search.focus();
                self.step_search(true)?;
            }
            search::SearchElement::Field | search::SearchElement::Body => {
                self.window.search.focus();
                self.after_search_change()?;
            }
        }
        Ok(true)
    }

    /// What the field is showing, and where its caret stands.
    ///
    /// The composition opens a space at the caret: what is drawn is the text with the pre-edit
    /// spliced in where the next character would go, and the caret stands after it. A field that
    /// painted the composition *over* the text would show both sharing cells neither can be read
    /// in — the bug the terminal's own preedit path was fixed for on 2026-08-13, and the shape the
    /// commit graph's field already answers.
    pub(in crate::runtime) fn search_field_look(&mut self) -> (String, bool, f32) {
        let field = self.window.search.field();
        let typed = field.text().to_owned();
        let before = field.before_caret().to_owned();
        let preedit = field.preedit().to_owned();
        let shown = format!(
            "{before}{preedit}{}",
            &typed[before.len().min(typed.len())..]
        );
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let font = search::FIELD_FONT_LOGICAL_PX * scale;
        let caret_x = self.window.renderer.measure_chrome_text(
            &mut self.app.gpu,
            &format!("{before}{preedit}"),
            font,
        );
        if shown.is_empty() {
            (search::field_placeholder().to_owned(), false, caret_x)
        } else {
            (shown, true, caret_x)
        }
    }

    /// The capsule's own level of the overlay stack, or nothing when it is down.
    pub(in crate::runtime) fn search_layers(&mut self) -> Vec<marks::OverlayLayer> {
        let Some(capsule) = self.search_capsule() else {
            self.window.search_layout = None;
            return Vec::new();
        };
        self.window.search_layout = Some(capsule);
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let palette = bt_render::chrome_palette();
        let counter = self.window.search.counter();
        let flags = self.window.search.flags();
        let broken = self.window.search.error().is_some();
        let focused = self.window.search.is_focused();
        let hover = self.window.search_hover;
        let (text, typed, caret_x) = self.search_field_look();
        vec![search::build(
            &capsule,
            &search::CapsuleLook {
                text: &text,
                typed,
                caret_x,
                focused,
                broken,
                counter: &counter,
                flags,
                offered: self.search_flags_offered(),
                hover,
            },
            &palette,
            scale,
        )]
    }

    /// **The contrast floor, changed while the window is up** (DESIGN §2.6).
    ///
    /// Hot, like the scheme rows beside it and for the same reason: what this row changes is on
    /// screen while the row is being pressed, and a floor that took effect at the next launch
    /// would be a row nobody could evaluate. The renderer's own
    /// `bt_render::set_minimum_contrast` advances the theme revision on a real change, which is
    /// what discards the composed rows shaped under the old floor — so the work here is the
    /// file, the push, and one repaint.
    /// **Where a non-address goes** (§7.7 ②, 方案 §0's five extras).
    ///
    /// One write and nothing else: the engine is read at the moment the address
    /// field commits, so there is no cached copy anywhere to push it into and
    /// nothing on the glass changes until somebody types a word.
    pub(crate) fn apply_search_engine(
        &mut self,
        engine: bt_persist::SearchEngineV1,
    ) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.search_engine = engine;
        Ok(self.app.settings_store.store(settings))
    }

    /// A composition aimed at the search capsule (§7.1.5d, D-7).
    ///
    /// The pre-edit is *not* folded into the query, which is what makes Escape
    /// during a composition un-type nothing: it is drawn at the caret, it pushes
    /// the caret along, and it leaves through the commit — an ordinary insert.
    /// So the search is re-asked on the commit and never on the pre-edit: a
    /// half-composed `ni'hao` is not a query anybody asked for, and scanning a
    /// hundred thousand lines for it on the way to the two characters it becomes
    /// would be work done for a string the reader never typed.
    pub(in crate::runtime) fn search_ime(&mut self, event: Ime) -> Result<()> {
        if !self.window.search.is_focused() {
            return Ok(());
        }
        match event {
            Ime::Preedit(text, _) => {
                self.window.search.field_mut().set_preedit(&text);
                if self.refresh_overlay() {
                    self.present_chrome_change()?;
                }
                return Ok(());
            }
            Ime::Commit(text) => self.window.search.field_mut().insert(&text),
            Ime::Enabled | Ime::Disabled => return Ok(()),
        }
        self.window.search_revision = self.window.search_revision.wrapping_add(1);
        self.refresh_search(true)?;
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }
}
