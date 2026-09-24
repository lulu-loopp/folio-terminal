//! `clipboard` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{
    Drag, DropLanding, LeafSession, PasteAnswer, PasteBody, PasteCardKey, PasteOffer, PasteTarget,
    PreparedClipboardPaste, Runtime, StagedPaste, UserInputKind, copy_selection, hang_watch,
    input_line_needs_a_space_first, offer_pty_input, paste_answer_text, paste_body,
    paste_card_step, paste_offer_is_kept, paste_target_is_live, pending_paste_in,
    prepare_clipboard_paste, prepare_dropped_paste, profile_banner_name,
    recoverable_clipboard_write, restore, seats, stage_paste, take_pending_paste, text_field,
    toast, write_selection_text, write_terminal_clipboard_text,
};
use anyhow::Result;
use bt_layout::SeatId;
use bt_render::{FrameSource, FrameTrigger};
use std::path::{Path, PathBuf};
use std::time::Instant;

impl Runtime<'_> {
    /// **What a landing is promising to write into, read off the window as it
    /// stands this instant** (review 2026-09-17 P1-a) — or `None` when it is
    /// promising no text write at all.
    ///
    /// Asked twice per gesture and never cached: once by [`Self::drive_drag`],
    /// where it becomes the offer the box on screen stands for, and once by the
    /// release, which compares its own reading against that one
    /// ([`paste_offer_survives`]). That the two readings are of *the same
    /// function at two moments* is the whole mechanism — a second implementation
    /// here would be two answers that can differ for reasons nothing in the
    /// window is about.
    ///
    /// Four refusals, and each is a case a bare seat number could not tell apart:
    /// a landing that is not a middle at all (an edge band promises a split, not
    /// a paste); a middle over a seat the tree no longer has; a middle over a
    /// seat that is not a terminal; and a terminal with **no shell in it**, which
    /// is a pane [`Self::paste_paths_into`] would find nothing to write to and
    /// leave silently — the silent refusal M147 forbids. Refusing here makes it
    /// the dashed outline instead, because [`Self::plan_for`] asks this too.
    pub(crate) fn paste_offer_at(&self, landing: Option<DropLanding>) -> Option<PasteOffer> {
        let landing = landing?;
        let DropLanding::SeatCentre { target } = landing else {
            return None;
        };
        if self.seats.tree().find_seat(target)?.kind != bt_layout::SeatKind::Terminal {
            return None;
        }
        Some(PasteOffer {
            landing,
            target: self.paste_target(target)?,
        })
    }

    /// Whether a program may put a message on the desktop (§7.6).
    ///
    /// **The whole of the switch is this one key**, and there is deliberately nothing else in
    /// here — no registry to undo, no notifier to tear down, no state to clear.
    ///
    /// *No registry to undo*, because the AppUserModelID is where Windows keeps the **user's**
    /// own choices about Folio's notifications; deleting it on `Off` would throw those away and
    /// `On` would come back as a stranger. It is also measurably a bad idea: deleting the
    /// platform's own settings key from under a live session wedges every later toast for that
    /// identity until the platform notices, which the probe behind §7.6 hit on this machine.
    ///
    /// *No notifier to tear down*, because the object is built on the first toast that passes the
    /// gate and this switch is upstream of the gate — a reader who has never let one through has
    /// nothing to release.
    ///
    /// *No state to clear*, because a notification never made any. The tab's dot, the bell latch
    /// and the unread ledger are untouched on both sides of this row, which is the sentence
    /// §7.6 is written under: this is an outlet for the ledger, never a second copy of it.
    /// Store the reader's answer about copy-on-select (丙4).
    ///
    /// Nothing else to do: [`should_copy_on_select_release`] reads the loaded
    /// settings at the moment a drag is let go, so the next release after this
    /// write already answers the new way — the same shape
    /// [`Self::apply_terminal_notifications`] has, and for the same reason.
    pub(crate) fn apply_copy_on_select(&mut self, enabled: bool) {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.copy_on_select = enabled;
        self.app.settings_store.store(settings);
    }

    /// Store the reader's answer about the multi-line paste card (0.4.4 ticket 02).
    ///
    /// [`Self::apply_copy_on_select`]'s shape: [`Self::deliver_paste`] reads the loaded settings
    /// at the moment of each paste, so the next one already answers the new way.
    pub(crate) fn apply_multiline_paste_ask(&mut self, enabled: bool) {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.multiline_paste_ask = enabled;
        self.app.settings_store.store(settings);
    }

    /// Put one row's whole path on the clipboard (K143).
    ///
    /// The path as the *user* would write it — `full_path`'s own separators —
    /// because the whole point of this verb is that the string is about to be
    /// pasted somewhere this window does not control.
    pub(in crate::runtime) fn copy_path_to_clipboard(&mut self, path: &Path) -> Result<()> {
        let text = path.to_string_lossy().into_owned();
        let result = write_terminal_clipboard_text(&text);
        recoverable_clipboard_write(result, "copy a files row's path");
        Ok(())
    }

    pub(in crate::runtime) fn copy_selection(&mut self) -> Result<()> {
        let active = self.window.active_tab;
        let Some(leaf) = self.window.tabs[active].focused_mut() else {
            return Ok(());
        };
        if !copy_selection(
            &mut leaf.session,
            &mut leaf.projection,
            write_terminal_clipboard_text,
        ) {
            return Ok(());
        }
        self.publish_interaction_frame()
    }

    /// Copy-on-select, from the pane the gesture began in.
    ///
    /// The text is that pane's, whatever pane the pointer let go over: it is the
    /// selection just made that the hand means, and there is only one pane it was
    /// ever made in.
    pub(in crate::runtime) fn copy_selection_on_release(&self, seat: SeatId) {
        let Some(leaf) = self.sessions.get(&seat) else {
            return;
        };
        write_selection_text(&leaf.session, true, write_terminal_clipboard_text);
    }

    /// **The four questions a text write answers that no other drop has to**
    /// (review 2026-09-17, both rounds).
    ///
    /// Every other landing this engine commits moves panes inside one tree: a
    /// stale aim there costs a rectangle, which the reader can see and undo by
    /// dragging again. Bytes on a command line are neither. So this one gesture
    /// pays for a second reading of everything the first reading decided, and
    /// refuses — silently to the shell, visibly as the outline the box was
    /// already wearing — on any disagreement at all. **It never redirects.**
    ///
    /// ① **Where the hand actually is.** The engine's aim is
    /// [`Self::survey_drop`]'s answer to the last *delivered* pointer move, and
    /// the release does not carry a position of its own: winit's Windows backend
    /// emits the button release without refreshing the cursor, and the router
    /// above answers a release from `pointer_position` or, failing that, from
    /// `pointer_last_seen`. Both are readings from before the hand opened. So
    /// the platform is asked where the cursor is — the same door a dropped file
    /// goes through ([`Self::platform_pointer_now`]) — and the aim is taken
    /// again, against the tree and the viewport as they stand at this instant.
    /// **A platform that will not say refuses**, because "I do not know where
    /// the hand is" is not a licence to use a reading known to be older.
    ///
    /// ② **That it is still the same offer.** The fresh aim must be the very
    /// landing the box was drawn for, and the terminal it names must be the same
    /// shell in the same tab that the box was promising — which is
    /// [`PasteOffer`] and [`paste_offer_survives`], and which is what a bare
    /// [`SeatId`] cannot say: ids are minted per tree, so the same number in the
    /// tab that got activated when this one's last shell exited is a different
    /// terminal entirely. The landing is *inside* the offer, so the comparison
    /// that answers "the same shell" answers "the same aim" in the same breath,
    /// and there is one rule here rather than four.
    ///
    /// ③ **That the plan was not refused.** [`seats::Seats::plan_content_drop`]
    /// answers a plan with no layout when the window has shrunk below what the
    /// tree needs; [`Self::dock_overlay_layers`] then takes the word off the box
    /// and draws the refusal. Writing behind that outline would make the picture
    /// a lie in the one direction that costs a command line.
    ///
    /// ④ **That the pane is the one that is actually visible there.** The
    /// pointer is captured for the whole of a drag, so the release is delivered
    /// to the window that started it wherever the hand has got to — and
    /// [`Self::survey_drop`] reads *this window's* geometry, which goes on
    /// describing a pane that another window may be sitting in front of. So the
    /// window manager is asked what is on top at the fresh point, with the same
    /// door and the same conversion [`Self::drive_drag`] uses before it surveys
    /// ([`Self::glass_here`]) — and, unlike the drag engine, a text write refuses
    /// the answer it does not get ([`glass_allows_a_text_write`]). Neither the
    /// broker's aim nor the broker's pointer would do: both are refreshed by
    /// delivered motion, which is precisely the reading ① exists to distrust.
    ///
    /// Answers the address to write to — the one **both** readings agree on —
    /// rather than a `bool`, so that no caller can take the verdict from here and
    /// the destination from somewhere older.
    pub(in crate::runtime) fn paste_offer_kept(
        &self,
        drag: &Drag,
        plan: &seats::DropPlan,
    ) -> Option<PasteTarget> {
        let released_at = self.platform_pointer_now()?;
        // The seam latch is this gesture's and the runtime holds none of it; a
        // copy is passed because this reading must not move the live one — the
        // gesture is over.
        let mut seam = drag.seam;
        let at_release = self.survey_drop(&drag.source, drag.home, released_at, &mut seam);
        paste_offer_is_kept(
            self.glass_here(released_at),
            drag.paste_offer,
            self.paste_offer_at(at_release),
            plan.fits(),
            self.a_modal_holds_the_window(),
        )
    }

    /// **What the clipboard can put into a one-line field**, or nothing when
    /// the platform will not hand it over.
    ///
    /// A failure is silent on [`Self::paste_into_preview`]'s own terms: a
    /// clipboard another process is holding open is a condition that clears
    /// itself, and a box that raised a card about it would be interrupting a
    /// reader mid-query to report an event they can simply repeat.
    pub(in crate::runtime) fn clipboard_line(&self) -> String {
        hang_watch::during(hang_watch::Station::ClipboardRead, || {
            bt_platform::clipboard_text()
        })
        .ok()
        .as_deref()
        .map(text_field::one_line)
        .unwrap_or_default()
    }

    /// `Ctrl+V` / `Shift+Insert` — the keyboard's paste, into the shell the
    /// keyboard is in.
    pub(in crate::runtime) fn paste_from_clipboard(&mut self) -> Result<()> {
        self.paste_from_clipboard_into(self.focused_leaf)
    }

    /// The same paste, **into one named pane**.
    ///
    /// One door and not two, which is the whole reason this took the seat as a
    /// parameter rather than the menu growing a paste of its own: the bytes, the
    /// bracketing, the chunking onto the single synchronous writer, the cleared
    /// selection and the return to the bottom are all decisions this window has
    /// already made once ([`paste_text`]), and a second paste path would be a
    /// second place for the multi-line policy to be decided when it lands.
    ///
    /// The seat differs from `focused_leaf` for exactly one caller — the
    /// terminal menu, which is raised by a right press, and a right press does
    /// not move the focus.
    pub(in crate::runtime) fn paste_from_clipboard_into(&mut self, seat: SeatId) -> Result<()> {
        let active = self.window.active_tab;
        let Some(leaf) = self.window.tabs[active].sessions.get(&seat) else {
            return Ok(());
        };
        // Named now, while the gesture is happening, because the picture rung
        // below spends it on a later turn (review X-1).
        let target = PasteTarget {
            tab: self.window.tabs[active].id,
            seat,
            incarnation: leaf.incarnation,
        };
        let recipient = leaf.paste_recipient.clone();
        let leading_space = input_line_needs_a_space_first(&leaf.session);
        let leaving = hang_watch::enter(hang_watch::Station::ClipboardRead);
        let payload = bt_platform::clipboard_payload();
        hang_watch::at(leaving);
        let mut prepared = prepare_clipboard_paste(payload, &recipient, leading_space);
        // **The picture cannot travel down [`Self::deliver_paste`], because there is
        // no text yet** (§7.61): what a picture pastes is the path of a file nobody
        // has written, and writing it is a PNG encode this thread must not do. It is
        // lifted out *here* rather than answered before the line below, so that a
        // refusal notice still leaves by the one door every paste's notices leave by
        // — and the line below is then an ordinary paste with nothing in it.
        let offered = std::mem::take(&mut prepared.picture);
        // Whether it reached a shell is not asked here: a clipboard paste goes
        // to the pane that already holds the keyboard, so there is no focus for
        // it to move (owner's ruling 2026-09-17 changed the two *drop* roads and
        // left this one alone).
        self.deliver_paste(target, prepared, "write clipboard paste to PTY")?;
        if offered.is_empty() {
            return Ok(());
        }
        self.save_clipboard_picture(target, offered)
    }

    /// **The same paste, with the paths already in hand** — what a drop onto a
    /// pane leaves behind (GitHub issue #1 ②).
    ///
    /// The only difference between a file *copied* in Explorer or Finder and one
    /// *dropped* on this window is how the list of paths reached this process,
    /// and that is the one thing neither the shell nor the reader can see. So it
    /// is also the only thing that differs here: the paths go through
    /// [`prepare_dropped_paste`] into the very function the clipboard's own
    /// road uses, which is what makes the quoting, the `paste_paths_as` profile
    /// key, the joining of several files into one command line and the refusal
    /// notices one implementation rather than two that drift.
    ///
    /// **Focus is not moved here**, and the caller is what moves it (owner's
    /// ruling 2026-09-17). The rule used to be that a drop leaves the keyboard
    /// alone, on the footing that a pointer gesture does not take it away from
    /// the pane the reader was typing in; the owner met the other half of that
    /// on the machine — the keystroke *after* a drop is `Enter` or the rest of
    /// the argument, and with the keyboard left behind it goes into a different
    /// shell, which is the wrong-pane hazard this road spent three review rounds
    /// closing for the path itself. So the two drop roads move it and the
    /// clipboard road does not, because a clipboard paste already went to the
    /// pane holding the keyboard. That decision belongs to each caller, not
    /// here: this function is the one door all three share.
    ///
    /// **Answers whether a shell actually received bytes**, which is what a
    /// caller that moves focus has to know. `false` for a target that is no
    /// longer live, for a path no shell could spell (the refusal card has been
    /// raised by then) and for a seat whose session has gone — three different
    /// reasons and one meaning: nothing was written, so nothing is focused and
    /// nothing is raised.
    ///
    /// **Two roads arrive here and not one** (§7.61). The second is the file
    /// Folio writes for a picture on the clipboard, which by the time it has a
    /// name is a path in hand and nothing else — the same sentence this
    /// function's first paragraph makes about a drop, said about a third way of
    /// coming by a path. `context` is all they do not share: it names the write
    /// for a reader of the error, and a picture that could not reach a shell is
    /// not a drop that could not.
    pub(in crate::runtime) fn paste_paths_into(
        &mut self,
        target: PasteTarget,
        paths: Vec<PathBuf>,
        context: &'static str,
    ) -> Result<bool> {
        let Some(index) = self.live_paste_target(target) else {
            return Ok(false);
        };
        let leaf = &self.window.tabs[index].sessions[&target.seat];
        let recipient = leaf.paste_recipient.clone();
        let leading_space = input_line_needs_a_space_first(&leaf.session);
        let prepared = prepare_dropped_paste(paths, &recipient, leading_space);
        self.deliver_paste(target, prepared, context)
    }

    /// **Name the shell in one of this window's seats**, as a paste's
    /// destination (review X-1).
    ///
    /// `None` when there is no shell in that seat, which is the same answer
    /// every paste road already gave for that case. Built at the moment of the
    /// gesture, so that a road which spends it later spends an address rather
    /// than a guess.
    pub(crate) fn paste_target(&self, seat: SeatId) -> Option<PasteTarget> {
        let tab = self.window.tabs.get(self.window.active_tab)?;
        let leaf = tab.sessions.get(&seat)?;
        Some(PasteTarget {
            tab: tab.id,
            seat,
            incarnation: leaf.incarnation,
        })
    }

    /// **Is the shell this paste was promised to still the shell on top?**
    /// (review X-1) — the tab's index if so, and `None` if the paste has
    /// nowhere left to land.
    ///
    /// Three questions and all three are load-bearing. The **tab** is found by
    /// its id, never by the position it had, because tabs move. The **seat** has
    /// to still be in it. And the **incarnation** has to match, because a seat
    /// whose shell was restarted is a hole with a different program in it, and
    /// typing a path into it is typing into something the reader never addressed.
    ///
    /// **And that tab has to be the one on top**, which is the conservative arm
    /// and is stated rather than implied. The bookkeeping a delivered paste owes
    /// — the attention answer, the typing note, the frame — is written against
    /// the active tab throughout this file, so a paste into a background tab
    /// would be bytes sent with none of it done. A reader who pressed `Ctrl+V`
    /// and left for another tab before the encode finished gets nothing, which
    /// is the honest half of that: the file is still on disk, the newest twenty
    /// are kept, and no shell they were not looking at was typed into.
    pub(in crate::runtime) fn live_paste_target(&self, target: PasteTarget) -> Option<usize> {
        let index = self.window.active_tab;
        let tab = self.window.tabs.get(index)?;
        let standing = tab.sessions.get(&target.seat).map(|leaf| leaf.incarnation);
        paste_target_is_live(tab.id, standing, target).then_some(index)
    }

    /// **What a prepared paste does to one named pane**, whichever road
    /// prepared it.
    ///
    /// The tail [`Self::paste_from_clipboard_into`] always had, given a name on
    /// the day a second road arrived at it. `context` is the one thing the two
    /// roads do not share: it names the write for a reader of the error, and a
    /// drop that could not reach a shell is not a clipboard that could not.
    ///
    /// **And the one place a paste may be asked about** (0.4.4 ticket 02). All four doors arrive
    /// here, so [`stage_paste`] — and through it [`crate::paste_road`] — is asked exactly once per
    /// paste, after the address is known to be live and before a byte is written. A paste it holds
    /// raises the card and sends nothing; the card's answer spends it through
    /// [`Self::answer_paste_card`].
    fn deliver_paste(
        &mut self,
        target: PasteTarget,
        prepared: PreparedClipboardPaste,
        context: &'static str,
    ) -> Result<bool> {
        // **The address is checked before anything is said**, which is why this
        // stands above the notice rather than beside the write (review X-1): a
        // card about a path that could not be spelled for a shell that is no
        // longer there is a card about nothing.
        let Some(active) = self.live_paste_target(target) else {
            return Ok(false);
        };
        if let Some(notice) = prepared.notice {
            self.toast(
                toast::ToastKind::Error,
                toast::ToastAnchor::Window,
                None,
                notice,
            )?;
        }
        let Some(text) = prepared.text else {
            return Ok(false);
        };
        let ask = self.app.settings_store.loaded().multiline_paste_ask;
        match stage_paste(
            &mut self.window.tabs[active],
            target,
            text,
            prepared.clipboard_text,
            ask,
            bt_platform::host_platform(),
            context,
        ) {
            StagedPaste::Send(text) => self.send_paste(target, PasteBody::Text(&text), context),
            StagedPaste::InputLine(bytes) => {
                self.send_paste(target, PasteBody::InputLine(&bytes), context)
            }
            StagedPaste::Held => {
                self.window.paste_card_hover = None;
                if self.refresh_overlay() {
                    self.present_chrome_change()?;
                }
                Ok(false)
            }
        }
    }

    /// **The one writer every paste ends in** — today's tail of [`Self::deliver_paste`], and the
    /// road the paste card's answer takes too (0.4.4 ticket 02), and a PowerShell prompt's
    /// input-line bytes (ticket 03). Whatever the body, it is one write.
    ///
    /// **The address is asked again here**, because the card's answer lands on a later turn:
    /// a tab no longer on top, a seat gone or a shell restarted since the paste was held all send
    /// nothing (review X-1's rule for every delayed paste). On the synchronous road it is the
    /// same question asked twice in one turn, which costs nothing.
    fn send_paste(
        &mut self,
        target: PasteTarget,
        body: PasteBody<'_>,
        context: &'static str,
    ) -> Result<bool> {
        let Some(active) = self.live_paste_target(target) else {
            return Ok(false);
        };
        let seat = target.seat;
        let Some(LeafSession {
            pty,
            session,
            projection,
            ..
        }) = self.window.tabs[active].sessions.get_mut(&seat)
        else {
            return Ok(false);
        };
        // **The answer travels back out of the paste** (review 2026-09-17 P2-a):
        // `write_pty_input` swallows a refusal, which is right for a keystroke
        // and wrong for a caller that is about to move the reader's keyboard on
        // the strength of it. Everything below this line is unchanged — the
        // bookkeeping a paste owes is owed for the gesture rather than for the
        // ring's mood — and only what this function *answers* now depends on it.
        let landed = paste_body(session, projection, body, |bytes| {
            offer_pty_input(pty.as_ref(), bytes, context)
        })?;
        // A paste is one gesture landing in one named pane, so it answers whatever that pane was
        // asking — and it is the pane the clipboard went into, not the one holding the keyboard
        // (`attention` plan §10.3.2 row 3).
        self.answer_attention(seat, UserInputKind::Paste);
        // Review row R1-24 — a paste is the reader putting bytes in, exactly as
        // a keystroke is.
        self.note_user_typing(seat);
        // **The keyboard's clock is the keyboard's pane's.** A paste into the
        // focused shell is a keystroke and is measured as one; a paste into the
        // pane a menu was raised over is not, and stamping `pending_keyboard_at`
        // for it would put a pointer gesture into the input-latency the caret's
        // own pane is judged by. The other pane still has to reach the glass,
        // which is exactly what `repaint_pane_change` is for.
        if seat != self.focused_leaf {
            self.repaint_pane_change(seat)?;
            return Ok(landed.queued());
        }
        self.pending_keyboard_at = Some(Instant::now());
        self.publish_frame(FrameTrigger {
            occurred_at: self.pending_keyboard_at.unwrap_or_else(Instant::now),
            source: FrameSource::Keyboard,
        })?;
        Ok(landed.queued())
    }

    /// **The pane the paste card is asking about**, or `None` while no paste waits — which is
    /// the whole of "is the card up" (0.4.4 ticket 02).
    ///
    /// Read off the leaves of the tab on top rather than off a flag of its own, so the card and
    /// the fact it projects cannot disagree: when the shell goes, the question goes with it.
    pub(crate) fn paste_card_seat(&self) -> Option<SeatId> {
        let tab = self.window.tabs.get(self.window.active_tab)?;
        pending_paste_in(tab).map(|(seat, _)| seat)
    }

    /// **The word `Enter` activates on the card that is up**, or `None` while no paste waits —
    /// read off the pending paste, like the card, so the key and the ring cannot disagree.
    pub(crate) fn paste_card_focus(&self) -> Option<PasteAnswer> {
        let tab = self.window.tabs.get(self.window.active_tab)?;
        pending_paste_in(tab).map(|(_, pending)| pending.focus())
    }

    /// **A key on the card** ([`paste_card_step`]): a focus move is spent on the pending paste
    /// and repainted, and an answer is spent.
    pub(in crate::runtime) fn press_paste_card_key(&mut self, key: PasteCardKey) -> Result<()> {
        let active = self.window.active_tab;
        let Some(pending) = self.window.tabs[active]
            .sessions
            .values_mut()
            .find_map(|leaf| leaf.pending_paste.as_mut())
        else {
            return Ok(());
        };
        match paste_card_step(pending, key) {
            Some(answer) => self.answer_paste_card(answer),
            None => {
                if self.refresh_overlay() {
                    self.present_chrome_change()?;
                }
                Ok(())
            }
        }
    }

    /// **Spend the card's answer** — `Enter`, `Tab`, `Esc`, or a press on it.
    ///
    /// The paste is taken off its leaf first, whatever the answer, so an answer can never be
    /// spent twice and the card is gone before anything is written. `Cancel` sends nothing and
    /// touches nothing, the clipboard included. The other two go through [`Self::send_paste`],
    /// which re-checks the address before a byte leaves.
    pub(in crate::runtime) fn answer_paste_card(&mut self, answer: PasteAnswer) -> Result<()> {
        let active = self.window.active_tab;
        let Some(pending) = take_pending_paste(&mut self.window.tabs[active]) else {
            return Ok(());
        };
        self.window.paste_card_hover = None;
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        if let Some(text) = paste_answer_text(&pending, answer) {
            self.send_paste(pending.target, PasteBody::Text(&text), pending.context)?;
        }
        Ok(())
    }

    /// A press on the card, answered by what it landed on. The face and the scrim answer nothing.
    pub(in crate::runtime) fn press_paste_card(
        &mut self,
        target: restore::PasteCardTarget,
    ) -> Result<()> {
        let answer = match target {
            restore::PasteCardTarget::Panel => return Ok(()),
            restore::PasteCardTarget::Close => PasteAnswer::Cancel,
            restore::PasteCardTarget::Join => PasteAnswer::Join,
            restore::PasteCardTarget::Run => PasteAnswer::RunLineByLine,
        };
        self.answer_paste_card(answer)
    }

    /// The paste card, measured against a real font, or nothing while no paste waits.
    ///
    /// `<shell>` is the profile's own title — the name the tab already reads — through
    /// [`profile_banner_name`], the one door that names a pane's profile to the reader.
    pub(in crate::runtime) fn paste_card_layout(&mut self) -> Option<restore::PasteCardLayout> {
        let tab = self.window.tabs.get(self.window.active_tab)?;
        let (seat, pending) = pending_paste_in(tab)?;
        let shell = profile_banner_name(&tab.sessions.get(&seat)?.profile);
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (width, height) = (width as f32, height as f32);
        let scale = self.window.renderer.scale_factor() as f32;
        let lines = pending.lines;
        let word = |answer| match answer {
            PasteAnswer::Join => restore::PasteCardTarget::Join,
            PasteAnswer::RunLineByLine | PasteAnswer::Cancel => restore::PasteCardTarget::Run,
        };
        let default = word(pending.default_answer());
        // The ring stands where the keyboard moved the focus, and only once it has.
        let ring = pending.moved_focus.map(word);
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let content = restore::paste_card_content(
            lines,
            &shell,
            default,
            width,
            scale,
            &mut |text, size, weight| {
                renderer.measure_chrome_label(gpu, text, size, weight, 0.0, false)
            },
        );
        Some(restore::paste_card_layout(&content, width, height, scale).with_ring(ring))
    }
}
