//! `clipboard` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::PtyTarget;
use crate::TermViewPorts;
use crate::clipboard_lane::{
    Ask, ClipboardRequest, ClipboardTerminal, Declined, Held, OwedCopy, OwedPaste, PASTES_OWED,
    PasteHold, PasteInto, Reply,
};
use crate::lane::Terminal;
use crate::runtime::keyboard::ime_owner;
use crate::window_seam::prelude::*;
use crate::{
    ClipboardWriteEffect, Drag, DropLanding, PasteAnswer, PasteBody, PasteCardKey, PasteOffer,
    PasteTarget, PreparedClipboardPaste, PreviewSurface, Runtime, ShellSession, StagedPaste,
    TextFieldSeat, UserInputKind, i18n, menubar, offer_pty_input, profile_banner_name, restore,
    seats, text_field, toast,
};
use crate::{
    GlassHere, ImeOwner, PasteFacts, PasteRoad, PendingPaste, TabId, TabState, input, quit,
    shell_literal,
};
use anyhow::Result;
use bt_layout::SeatId;
use bt_pty::PSREADLINE_PASTE_INPUT;
use bt_render::{FrameSource, FrameTrigger};
use bt_term::DualPlaneSession;
use bt_viewport::{MathBlockAnchor, ViewportProjection};
use std::path::{Path, PathBuf};
use std::time::Instant;
use winit::event::Ime;

impl<B: Backend> Runtime<'_, B> {
    /// **`Copy` on a row whose execution policy the user can change**: put the command on the
    /// clipboard and say so only after the platform accepts the write.
    pub(crate) fn copy_policy_command(&mut self) -> Result<()> {
        let (text, effect) = policy_command_copy();
        self.submit_clipboard_write(text, "copy the execution policy command", effect)
            .map(drop)
    }

    /// **Put one request on the clipboard lane**, through this window's clipboard door.
    fn ask_the_clipboard(&mut self, ask: Ask) -> Result<ClipboardRequest, Declined> {
        self.window.window.clipboard(&self.app.clipboard_lane, ask)
    }

    /// **Submit one copy to the clipboard lane**, and apply its effect only once the write has
    /// landed ([`Self::settle_clipboard`]). Answers whether the lane took it.
    ///
    /// A newer copy replaces the window's last copy not yet answered, which ends `Superseded` — the
    /// clipboard ends holding the newer either way, and only the newer says it landed — **unless a
    /// paste was asked after that copy**, in any window: a paste reads the clipboard as it stood at
    /// its press, so the newer copy then queues behind the paste instead.
    ///
    /// A copy the lane refuses at its door is said once, through the window's door
    /// (`WindowHost::note`, `diagnostics.log` in the product), with `action`; one the clipboard
    /// refuses is said once by the lane's ledger. Neither says anything on screen, which is the
    /// owner's rule for a copy (2026-09-14 ②): only a copy that landed says it landed.
    pub(crate) fn submit_clipboard_write(
        &mut self,
        text: String,
        action: &'static str,
        effect: ClipboardWriteEffect,
    ) -> Result<bool> {
        let request = match self.ask_the_clipboard(Ask::Write(text)) {
            Ok(request) => request,
            Err(declined) => {
                self.window.window.note(&format!(
                    "the clipboard lane refused a copy at its door; {action} ignored: {}",
                    declined.reason()
                ));
                return Ok(false);
            }
        };
        let newer = request.id();
        let replaces = self.window.clipboard.copies.last().is_some_and(|older| {
            !self
                .app
                .clipboard_lane
                .a_paste_was_asked_after(older.request.id())
        });
        if replaces && let Some(older) = self.window.clipboard.copies.pop() {
            // An answer that came before the newer copy was asked is the older copy's own.
            let ended = self.app.clipboard_lane.supersede(older.request, newer);
            self.answer_copy(older.effect, ended)?;
        }
        self.window
            .clipboard
            .copies
            .push(OwedCopy { request, effect });
        Ok(true)
    }

    /// **What a copy's terminal does**: a write that landed applies its effect; a refusal was said
    /// by the lane's ledger, a replaced copy says nothing, and a lane whose worker ended said so.
    fn answer_copy(
        &mut self,
        effect: ClipboardWriteEffect,
        ended: ClipboardTerminal,
    ) -> Result<()> {
        match ended {
            Terminal::Done(_) => self.apply_clipboard_write_effect(effect),
            Terminal::Failed(_)
            | Terminal::Superseded { .. }
            | Terminal::Cancelled
            | Terminal::Joined { .. }
            | Terminal::WorkerGone => Ok(()),
        }
    }

    /// Put a rendered page's selection on the clipboard, through the door the
    /// terminal's own copy already uses. Answers whether the lane took it.
    pub(in crate::runtime) fn copy_preview_text_selection(
        &mut self,
        surface: PreviewSurface,
    ) -> Result<bool> {
        let Some(text) = self.preview_selected_text(surface) else {
            return Ok(false);
        };
        // The selection stays standing whatever the clipboard answers, so the
        // reader can try again.
        self.submit_clipboard_write(text, "copy a page's selection", ClipboardWriteEffect::None)
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
    ) -> Result<()> {
        // A block anchor names a place in a shell's transcript, so a tab with no
        // shell has no anchor anybody could have clicked and nothing to copy
        // (§7.1.6h) — the same `None` a stale anchor already answers with.
        let Some(index) = self.live_paste_target(target) else {
            return Ok(());
        };
        let Some(source) = self.window.tabs[index]
            .sessions
            .get(&target.seat)
            .and_then(|leaf| leaf.session.math_source(anchor))
            .map(str::to_owned)
        else {
            return Ok(());
        };
        // **Only a copy that landed says it landed** (owner ruling 2026-09-14 ②):
        // the tick is the write's effect, applied on the turn the clipboard lane
        // answers that it landed, and a tick on a clipboard the lane could not
        // reach would be the one acknowledgement in this product that confirms
        // nothing.
        self.submit_clipboard_write(
            source,
            "formula copy",
            ClipboardWriteEffect::MathCopied(anchor.clone()),
        )
        .map(drop)
    }

    pub(crate) fn apply_clipboard_write_effect(
        &mut self,
        effect: ClipboardWriteEffect,
    ) -> Result<()> {
        match effect {
            ClipboardWriteEffect::None => Ok(()),
            ClipboardWriteEffect::Toast { anchor, text } => {
                self.toast(toast::ToastKind::Ok, anchor, None, text)
            }
            ClipboardWriteEffect::ClearSelection { target, copied } => {
                let Some(index) = self.live_paste_target(target) else {
                    return Ok(());
                };
                let Some(leaf) = self.window.tabs[index].sessions.get_mut(&target.seat) else {
                    return Ok(());
                };
                if !clear_copied_selection(&mut leaf.session, &mut leaf.view.projection, &copied) {
                    return Ok(());
                }
                self.publish_interaction_frame()
            }
            ClipboardWriteEffect::MathCopied(anchor) => {
                self.window.math_copied = Some((anchor, Instant::now()));
                self.publish_interaction_frame()
            }
        }
    }

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
    /// Nothing else to do: [`super::terminal::should_copy_on_select_release`] reads the loaded
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
        self.submit_clipboard_write(text, "copy a files row's path", ClipboardWriteEffect::None)
            .map(drop)
    }

    /// `Ctrl+C`: the focused pane's selection onto the clipboard, and the selection taken down
    /// once the copy has landed ([`ClipboardWriteEffect::ClearSelection`]).
    pub(in crate::runtime) fn copy_selection(&mut self) -> Result<()> {
        let active = self.window.active_tab;
        let focused = self.window.tabs[active].focused_leaf;
        let Some(leaf) = self.window.tabs[active].sessions.get(&focused) else {
            return Ok(());
        };
        let Some(text) = selection_to_copy(&leaf.session, false) else {
            return Ok(());
        };
        // The pane the selection was made in, named now: its copy lands on a later turn.
        let target = PasteTarget {
            tab: self.window.tabs[active].id,
            seat: focused,
            incarnation: leaf.incarnation,
        };
        self.submit_clipboard_write(
            text.clone(),
            "copy",
            ClipboardWriteEffect::ClearSelection {
                target,
                copied: text,
            },
        )
        .map(drop)
    }

    /// Copy-on-select, from the pane the gesture began in.
    ///
    /// The text is that pane's, whatever pane the pointer let go over: it is the
    /// selection just made that the hand means, and there is only one pane it was
    /// ever made in.
    pub(in crate::runtime) fn copy_selection_on_release(&mut self, seat: SeatId) -> Result<()> {
        let Some(text) = self
            .sessions
            .get(&seat)
            .and_then(|leaf| selection_to_copy(&leaf.session, true))
        else {
            return Ok(());
        };
        self.submit_clipboard_write(text, "copy on select", ClipboardWriteEffect::None)
            .map(drop)
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

    /// **One paste into the text field that holds the keyboard** (B-AUDIT-046
    /// RT-1) — the keystroke's (`Ctrl+V`, `Shift+Insert`, `Cmd+V`) and Edit ▸
    /// Paste's alike, and never into the terminal the field is standing on.
    ///
    /// Each field takes it through **its own insert door**: the one a committed
    /// composition goes through, because a paste into a one-line field is
    /// exactly that — a run of text arriving at the caret, replacing the
    /// selection — and each of those doors already does what its field does
    /// after an insert (re-ask the search, re-filter the palette, write the
    /// setting through). One line of printable text for the five one-line
    /// fields (`text_field::one_line`, as [`Self::apply_clipboard_text_to_field`] cuts it);
    /// the settings dialog drops the line breaks instead, its own long-standing
    /// rule for a pasted path.
    ///
    /// **Asked of the clipboard lane, and landed on the turn it answers** —
    /// into the field, if that field still holds the keyboard then; a field that
    /// let it go withdraws the paste. A failure is silent on
    /// [`Self::paste_into_preview`]'s own terms: a clipboard another process is
    /// holding open is a condition that clears itself, and a box that raised a
    /// card about it would be interrupting a reader mid-query to report an event
    /// they can simply repeat; the lane's ledger says it once in the log.
    pub(crate) fn paste_into_field(&mut self, field: TextFieldSeat) -> Result<()> {
        self.ask_for_a_paste(PasteInto::Field(field), Ask::Text)
    }

    fn apply_clipboard_text_to_field(&mut self, field: TextFieldSeat, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        if field == TextFieldSeat::Settings {
            return self.paste_into_settings_field(text);
        }
        let line = Ime::Commit(text_field::one_line(text));
        match field {
            TextFieldSeat::FindBar => self.search_ime(line),
            TextFieldSeat::GraphSearch => self.graph_search_ime(&line),
            TextFieldSeat::GitPrompt => self.git_prompt_ime(&line),
            TextFieldSeat::Palette => self.palette_ime(&line),
            TextFieldSeat::TabName => self.rename_ime(&line),
            TextFieldSeat::Settings => Ok(()),
        }
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
    ///
    /// **A paste goes where a keystroke would, or nowhere** (0.4.5 ticket 57).
    /// The keyboard's road reaches this door only when no surface above the
    /// clipboard rung holds the keyboard; the Edit menu's road defers to the same
    /// list; the terminal menu's road is a press, and the restore card lets a
    /// press past it to raise that menu. So the door asks the ladder's own
    /// question, once, before the clipboard is read — and with the restore card
    /// up, the menu's *Paste* writes nothing, as `Ctrl+V` writes nothing.
    ///
    /// **The clipboard is asked on its own lane** (`clipboard_lane`), and its
    /// answer lands on a later turn, where the gesture was made: the pane is named
    /// now, by tab, seat and incarnation (review X-1), and the person's input to
    /// it waits behind the paste until the answer is spent ([`PasteHold`]).
    pub(in crate::runtime) fn paste_from_clipboard_into(&mut self, seat: SeatId) -> Result<()> {
        if self.a_surface_above_the_clipboard_rung_holds_the_keyboard() {
            return Ok(());
        }
        let active = self.window.active_tab;
        let Some(leaf) = self.window.tabs[active].sessions.get(&seat) else {
            return Ok(());
        };
        // Named now, while the gesture is happening, because the answer is spent on a later
        // turn (review X-1).
        let target = PasteTarget {
            tab: self.window.tabs[active].id,
            seat,
            incarnation: leaf.incarnation,
        };
        self.ask_for_a_paste(PasteInto::Terminal(target), Ask::Payload)
    }

    /// **One paste asked of the clipboard lane**, aimed at `into`.
    ///
    /// An earlier paste's answer that has come is spent first. A paste still owed is never
    /// replaced by a later one: this one queues behind it — both read the clipboard, in order — and
    /// a terminal's pane holds this paste's place behind whatever it already holds ([`PasteHold`]),
    /// so `Ctrl+V`, `k`, `Ctrl+V` reaches the shell as the first paste, `k`, the second. A window
    /// owes at most [`PASTES_OWED`]; a paste past that desk, or one the lane refuses at its door,
    /// is said once in the log and, for a terminal, once on screen.
    pub(in crate::runtime) fn ask_for_a_paste(&mut self, into: PasteInto, ask: Ask) -> Result<()> {
        let now = Instant::now();
        self.settle_clipboard(now)?;
        if self.window.clipboard.pastes.len() >= PASTES_OWED {
            self.window.window.note(&format!(
                "the clipboard lane: a paste was refused on the window: {PASTES_OWED} pastes are \
                 already owed"
            ));
            return self.say_the_clipboard_busy(into);
        }
        let request = match self.ask_the_clipboard(ask) {
            Ok(request) => request,
            Err(declined) => {
                self.window.window.note(&format!(
                    "the clipboard lane refused a paste at its door: {}",
                    declined.reason()
                ));
                return self.say_the_clipboard_busy(into);
            }
        };
        let id = request.id();
        self.window.clipboard.pastes.push(OwedPaste {
            id,
            request: Some(request),
            ended: None,
            into,
            asked_at: now,
        });
        if let PasteInto::Terminal(target) = into
            && let Some(leaf) = self.paste_leaf_mut(target)
        {
            leaf.paste_hold
                .get_or_insert_with(PasteHold::default)
                .queue_paste(id);
        }
        Ok(())
    }

    /// **Take what the clipboard lane has answered this window, withdraw what it no longer
    /// wants, and spend what can be spent** — on the turn its wake brings
    /// (`AppEvent::ClipboardAnswered`), on every turn of the window's (so a lost wake loses no
    /// answer), and before a new paste is asked.
    ///
    /// A copy that landed applies its effect. Every paste's terminal is taken as it comes. A paste
    /// still owed is withdrawn — ended `Cancelled`, its terminal to the lane's sink, a line in
    /// `diagnostics.log` saying why — when its pane closed, was restarted or left the front, when
    /// its field or page let the keyboard go, when it has been owed for
    /// [`crate::clipboard_lane::PASTE_DEADLINE`], or when what its pane holds has passed the cap;
    /// the last two also say once on screen that the clipboard was busy. A field's or a page's
    /// paste is spent as soon as it has ended; a terminal's when it reaches the front of its pane's
    /// hold ([`Self::advance_the_hold`]). `now` is the turn's clock, read once by the caller.
    pub(crate) fn settle_clipboard(&mut self, now: Instant) -> Result<()> {
        // The copies end in the order they were asked (one worker, first in first out); each that
        // has is answered, in that order.
        let mut at = 0;
        while at < self.window.clipboard.copies.len() {
            let Some(ended) = self.window.clipboard.copies[at].request.take() else {
                at += 1;
                continue;
            };
            let owed = self.window.clipboard.copies.remove(at);
            self.answer_copy(owed.effect, ended)?;
        }
        for owed in &mut self.window.clipboard.pastes {
            if owed.ended.is_none()
                && let Some(ended) = owed.request.as_mut().and_then(ClipboardRequest::take)
            {
                owed.ended = Some(ended);
                owed.request = None;
            }
        }
        let mut busy = None;
        for at in 0..self.window.clipboard.pastes.len() {
            let owed = &self.window.clipboard.pastes[at];
            if owed.ended.is_some() {
                continue;
            }
            let (into, deadline) = (owed.into, owed.deadline());
            let Some(why) = self.paste_withdrawn(into, deadline, now) else {
                continue;
            };
            let owed = &mut self.window.clipboard.pastes[at];
            let request = owed.request.take();
            owed.ended = Some(Terminal::Cancelled);
            if let Some(request) = request {
                self.app.clipboard_lane.withdraw(request);
            }
            self.window.window.note(&format!(
                "the clipboard lane: a paste was cancelled: {}",
                why.words()
            ));
            if why.is_the_clipboard_busy() && busy.is_none() {
                busy = Some(into);
            }
        }
        if let Some(into) = busy {
            self.say_the_clipboard_busy(into)?;
        }
        let mut at = 0;
        while at < self.window.clipboard.pastes.len() {
            let owed = &self.window.clipboard.pastes[at];
            if matches!(owed.into, PasteInto::Terminal(_)) || owed.ended.is_none() {
                at += 1;
                continue;
            }
            let owed = self.window.clipboard.pastes.remove(at);
            if let Some(ended) = owed.ended {
                self.spend_paste(owed.into, ended)?;
            }
        }
        let holding: Vec<PasteTarget> = self
            .window
            .tabs
            .iter()
            .flat_map(|tab| {
                tab.sessions
                    .iter()
                    .filter(|(_, leaf)| leaf.paste_hold.is_some())
                    .map(move |(seat, leaf)| PasteTarget {
                        tab: tab.id,
                        seat: *seat,
                        incarnation: leaf.incarnation,
                    })
            })
            .collect();
        for target in holding {
            self.advance_the_hold(target)?;
        }
        // A terminal paste that has ended and that no pane holds a place for any more — its pane
        // closed, or moved to another window — has nowhere to be spent.
        let held = self.places_held();
        self.window
            .clipboard
            .pastes
            .retain(|owed| owed.ended.is_none() || held.contains(&owed.id));
        Ok(())
    }

    /// The pastes whose places this window's panes hold.
    fn places_held(&self) -> Vec<crate::clipboard_lane::ClipboardId> {
        let mut held = Vec::new();
        for tab in &self.window.tabs {
            for leaf in tab.sessions.values() {
                if let Some(hold) = &leaf.paste_hold {
                    held.extend(hold.pastes());
                }
            }
        }
        held
    }

    /// **Why a paste still owed is withdrawn now**, if it is.
    fn paste_withdrawn(
        &mut self,
        into: PasteInto,
        deadline: Instant,
        now: Instant,
    ) -> Option<Withdrawn> {
        let gone = match into {
            PasteInto::Terminal(target) => self.live_paste_target(target).is_none(),
            PasteInto::Field(field) => {
                menubar::paste_seat(self.clipboard_focus()) != menubar::ClipboardSeat::Field(field)
            }
            PasteInto::Preview(surface) => self.preview_keyboard_surface() != Some(surface),
        };
        if gone {
            return Some(Withdrawn::TargetGone);
        }
        if now >= deadline {
            return Some(Withdrawn::Deadline);
        }
        let over = match into {
            PasteInto::Terminal(target) => self
                .paste_leaf_mut(target)
                .and_then(|leaf| leaf.paste_hold.as_ref())
                .is_some_and(PasteHold::over_the_cap),
            PasteInto::Field(_) | PasteInto::Preview(_) => false,
        };
        over.then_some(Withdrawn::HeldOverTheCap)
    }

    /// **One pane's hold, spent as far as its answers allow** — from the front: a write of the
    /// person's goes to the shell as the write it was; a paste whose answer is in hand is spent
    /// ([`Self::spend_paste`]) with the hold taken off the pane, so its bytes go straight to the
    /// shell; a paste not yet answered stops it, and so does the multi-line card while it asks
    /// (its answer goes on from here, [`Self::answer_paste_card`]). An empty hold is let go.
    fn advance_the_hold(&mut self, target: PasteTarget) -> Result<()> {
        loop {
            let Some(leaf) = self.paste_leaf_mut(target) else {
                return Ok(());
            };
            if leaf.pending_paste.is_some() {
                return Ok(());
            }
            let Some(hold) = leaf.paste_hold.take() else {
                return Ok(());
            };
            let Some(front) = hold.pop_front() else {
                return Ok(());
            };
            match front {
                Held::Typed(bytes) => {
                    let written = offer_pty_input(
                        leaf.input_target(),
                        &bytes,
                        "release the input held behind a paste to PTY",
                    );
                    leaf.paste_hold = Some(hold);
                    written?;
                }
                Held::Paste(id) => {
                    let at = self
                        .window
                        .clipboard
                        .pastes
                        .iter()
                        .position(|owed| owed.id == id);
                    match at {
                        Some(at) if self.window.clipboard.pastes[at].ended.is_none() => {
                            hold.push_front(Held::Paste(id));
                            if let Some(leaf) = self.paste_leaf_mut(target) {
                                leaf.paste_hold = Some(hold);
                            }
                            return Ok(());
                        }
                        Some(at) => {
                            let owed = self.window.clipboard.pastes.remove(at);
                            let spent = match owed.ended {
                                Some(ended) => self.spend_paste(owed.into, ended),
                                None => Ok(()),
                            };
                            if let Some(leaf) = self.paste_leaf_mut(target) {
                                leaf.paste_hold = Some(hold);
                            }
                            spent?;
                        }
                        // No paste of this window's owns the place (the pane came here from
                        // another window while it was owed there): it holds nothing back.
                        None => {
                            if let Some(leaf) = self.paste_leaf_mut(target) {
                                leaf.paste_hold = Some(hold);
                            }
                        }
                    }
                }
            }
        }
    }

    /// **What one paste's terminal does**, where the paste was aimed.
    ///
    /// For a terminal (spent from the front of its pane's hold): a read the clipboard answered is
    /// pasted; a read the clipboard refused, or one lost with the lane's worker, says the notice
    /// every failed paste says; a paste withdrawn writes nothing. For a field or the quick edit:
    /// the text lands only if that field or page still holds the keyboard.
    fn spend_paste(&mut self, into: PasteInto, ended: ClipboardTerminal) -> Result<()> {
        match into {
            PasteInto::Terminal(target) => match ended {
                Terminal::Done(Reply::Payload(payload)) => {
                    self.apply_clipboard_payload(target, payload)
                }
                Terminal::Failed(_) | Terminal::WorkerGone => {
                    if self.live_paste_target(target).is_some() {
                        self.toast(
                            toast::ToastKind::Error,
                            toast::ToastAnchor::Window,
                            None,
                            i18n::Text::PasteClipboardRead.text().to_owned(),
                        )
                    } else {
                        Ok(())
                    }
                }
                Terminal::Done(_)
                | Terminal::Superseded { .. }
                | Terminal::Cancelled
                | Terminal::Joined { .. } => Ok(()),
            },
            PasteInto::Field(field) => match ended {
                Terminal::Done(Reply::Text(text))
                    if menubar::paste_seat(self.clipboard_focus())
                        == menubar::ClipboardSeat::Field(field) =>
                {
                    self.apply_clipboard_text_to_field(field, &text)
                }
                _ => Ok(()),
            },
            PasteInto::Preview(surface) => match ended {
                Terminal::Done(Reply::Text(text))
                    if self.preview_keyboard_surface() == Some(surface) =>
                {
                    self.apply_clipboard_text_to_preview(&text)
                }
                _ => Ok(()),
            },
        }
    }

    /// **The pane a paste was aimed at, wherever it stands in this window now** — by tab id, seat
    /// and incarnation, in any tab: its held input is its own whether or not its tab is on top.
    fn paste_leaf_mut(&mut self, target: PasteTarget) -> Option<&mut ShellSession> {
        self.window
            .tabs
            .iter_mut()
            .find(|tab| tab.id == target.tab)?
            .sessions
            .get_mut(&target.seat)
            .filter(|leaf| leaf.incarnation == target.incarnation)
    }

    /// **"The clipboard was busy", said once on screen** — for a terminal paste only: a field and
    /// the quick edit say nothing about a clipboard they could not read, on their own terms.
    fn say_the_clipboard_busy(&mut self, into: PasteInto) -> Result<()> {
        let PasteInto::Terminal(target) = into else {
            return Ok(());
        };
        if self.live_paste_target(target).is_none() {
            return Ok(());
        }
        self.toast(
            toast::ToastKind::Error,
            toast::ToastAnchor::Window,
            None,
            i18n::Text::PasteClipboardBusy.text().to_owned(),
        )
    }

    fn apply_clipboard_payload(
        &mut self,
        target: PasteTarget,
        payload: bt_platform::ClipboardPayload,
    ) -> Result<()> {
        let Some(active) = self.live_paste_target(target) else {
            return Ok(());
        };
        let Some(leaf) = self.window.tabs[active].sessions.get(&target.seat) else {
            return Ok(());
        };
        let recipient = leaf.launch.paste_recipient.clone();
        let leading_space = input_line_needs_a_space_first(&leaf.session);
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
        let recipient = leaf.launch.paste_recipient.clone();
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
    /// **And the one place a paste may be asked about** (0.4.4 ticket 02). Both roads arrive here
    /// — the clipboard's ([`Self::apply_clipboard_payload`], from the paste chord, the Edit menu and
    /// the terminal's menu) and paths in hand ([`Self::paste_paths_into`], from a drop, a dragged
    /// path and a saved clipboard picture) — so [`stage_paste`] — and through it
    /// [`paste_road`] — is asked exactly once per paste, after the address is known to be
    /// live and before a byte is written. A paste it holds
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
        let Some(ShellSession {
            life,
            session,
            view: crate::TermView { projection, .. },
            paste_hold,
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
        // **Behind a paste this pane is still owed**, as every input of the person's is: the
        // paste the clipboard lane answers takes the hold off before it writes, so only what was
        // offered after it — a drop, a picture's path — waits here (`clipboard_lane`).
        let target = match paste_hold {
            Some(hold) => PtyTarget::BehindAPaste(hold),
            None => PtyTarget::of(life.pty(), life.birth()),
        };
        let landed = paste_body(session, projection, body, |bytes| {
            offer_pty_input(target, bytes, context)
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
    ///
    /// **What the pane held behind the paste goes after it** (`clipboard_lane`): taken off the
    /// pane while the answer is written, and then spent from where the card stopped it, in its
    /// order — at once, for `Cancel`.
    pub(in crate::runtime) fn answer_paste_card(&mut self, answer: PasteAnswer) -> Result<()> {
        let active = self.window.active_tab;
        let Some(pending) = take_pending_paste(&mut self.window.tabs[active]) else {
            return Ok(());
        };
        let held = self
            .paste_leaf_mut(pending.target)
            .and_then(|leaf| leaf.paste_hold.take());
        self.window.paste_card_hover = None;
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        // An error here is the window's end (`App::fail`), and a hold off its pane goes with it.
        if let Some(text) = paste_answer_text(&pending, answer) {
            self.send_paste(pending.target, PasteBody::Text(&text), pending.context)?;
        }
        if let Some(held) = held
            && let Some(leaf) = self.paste_leaf_mut(pending.target)
        {
            leaf.paste_hold = Some(held);
        }
        self.advance_the_hold(pending.target)
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
        let shell = profile_banner_name(&tab.sessions.get(&seat)?.launch.profile);
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

    /// **What the window looks like to the Edit menu's clipboard rows**
    /// (T-MAC-EDIT-CLIPBOARD).
    ///
    /// The two questions [`Runtime::keyboard_input`] asks before it lets a
    /// `Cmd+C` reach the terminal, asked again in the same words — which is what
    /// `every_surface_the_menu_defers_to_stands_above_the_clipboard_rung` holds
    /// the two functions to.
    pub(crate) fn clipboard_focus(&mut self) -> menubar::ClipboardFocus {
        let rung = ime_owner(self.keyboard_owner());
        clipboard_focus_of(
            rung,
            self.a_surface_above_the_clipboard_rung_holds_the_keyboard(),
            self.settings_field_holds_the_keyboard(),
            self.preview_edit_focus().is_some(),
        )
    }

    /// **Whether a text field of the settings dialog holds the keyboard** — the
    /// one fact [`text_field_holding`] needs beyond the rung (B-AUDIT-046 RT-1).
    ///
    /// The conditions under which the ladder reaches
    /// [`Self::settings_field_key`] and it answers, asked in the ladder's own
    /// words: no card stands above the dialog (each of those has its rung above
    /// the dialog's), no chord is being recorded (the recorder takes every key),
    /// no picker is open over the field, and the focus is a text field.
    fn settings_field_holds_the_keyboard(&self) -> bool {
        let card_above = self.app.quit.as_ref().is_some_and(quit::Quit::is_asking)
            || self.window.dirty_gate.is_open()
            || self.window.first_run.is_open()
            || self.paste_card_seat().is_some()
            || self.update_card_is_up()
            || self.uninstall_card_is_up();
        !card_above
            && self.window.settings.is_open()
            && self.window.settings.recording_row().is_none()
            && self.window.settings.menu().is_none()
            && self.window.settings.row_menu().is_none()
            && self
                .window
                .settings
                .focus()
                .is_some_and(|target| self.window.settings.text_field(target).is_some())
    }
}

/// **Which text field holds the keyboard**, read off the rung the ladder
/// resolved ([`ime_owner`]) — or `None` when what holds it is not a text field
/// (B-AUDIT-046 RT-1).
///
/// The rungs *are* the enumeration: every rung that is a one-line field answers
/// with itself, and the match is total, so a rung added to [`ImeOwner`] has to
/// say here whether a paste goes into it. The one field that is not a rung of
/// its own is the settings dialog's: the dialog is a modal, and whether its
/// focus is a text field is the one extra fact this is handed
/// ([`Runtime::settings_field_holds_the_keyboard`]).
///
/// The preview's document is not here: it takes a paste through its own door
/// (`ClipboardSeat::PreviewDocument`), because it keeps the file's line breaks
/// where a one-line field keeps the first line.
#[must_use]
pub(crate) fn text_field_holding(rung: ImeOwner, settings_field: bool) -> Option<TextFieldSeat> {
    match rung {
        ImeOwner::Rename => Some(TextFieldSeat::TabName),
        ImeOwner::GitPrompt => Some(TextFieldSeat::GitPrompt),
        ImeOwner::Palette => Some(TextFieldSeat::Palette),
        ImeOwner::GraphSearch => Some(TextFieldSeat::GraphSearch),
        ImeOwner::Search => Some(TextFieldSeat::FindBar),
        ImeOwner::Modal => settings_field.then_some(TextFieldSeat::Settings),
        ImeOwner::FilesTree | ImeOwner::Preview | ImeOwner::Shell => None,
    }
}

/// **A paste whose bytes are already spelled**, with everything else a paste owes: the
/// selection goes, the view returns to the bottom, and the bytes go out in one write
/// ([`paste_text`]'s tail, and the whole of a PowerShell input-line paste — 0.4.4 ticket 03).
fn paste_spelled<T>(
    session: &mut DualPlaneSession,
    projection: &mut ViewportProjection,
    bytes: &[u8],
    write: impl FnOnce(&[u8]) -> Result<T>,
) -> Result<T> {
    session.port_clear_selection();
    projection.set_selection(None);
    projection.scroll_to_bottom();
    write(bytes)
}

/// **The one rule for whether a paste asks** (owner's rulings 2026-09-22, 0.4.4 ticket 02).
///
/// `setting on ∧ payload was text ∧ !bracketed ∧ more than one line`, with ticket 03's PowerShell
/// road standing in front of the question. The cheap facts are read first, so a bracketed paste
/// or a path is never scanned, and a single line is scanned once and allocates nothing.
pub(crate) fn paste_road(text: &str, facts: PasteFacts) -> PasteRoad {
    if !facts.clipboard_text || facts.bracketed {
        return PasteRoad::AsTyped;
    }
    let lines = input::pasted_line_count(text);
    if lines <= 1 {
        return PasteRoad::AsTyped;
    }
    if facts.powershell_prompt_open && facts.through_conpty {
        return PasteRoad::InputLine;
    }
    if facts.ask {
        PasteRoad::Ask { lines }
    } else {
        PasteRoad::AsTyped
    }
}

/// **What a multi-line paste into a PowerShell prompt writes** (0.4.4 ticket 03) — one write,
/// whichever of the spike's two proven roads the text needs.
///
/// * **C1, the clipboard road** — the one byte [`PSREADLINE_PASTE_INPUT`] (Ctrl+V), and
///   PSReadLine reads the clipboard itself and inserts the block without accepting it. Taken
///   when [`input::psreadline_pastes_it_unchanged`]: what lands is then what Folio's cleanup
///   would have sent. Only [`paste_road`] sends a paste here, and it does so only for the
///   clipboard's own text, so the shell re-reads the very text Folio read — never a path Folio
///   spelled or a picture it saved. **A known limit, accepted** (2026-09-23 spike, A4): a third
///   program that rewrites the clipboard in the ~20 ms between this write and the shell's read
///   gets its text pasted instead; Folio does not read the clipboard a second time to check.
/// * **C2, Folio's own bytes** — [`input::input_line_bytes`]: today's sanitized text with every
///   break as a Shift+Enter record. Taken for text Folio has to change (a control character it
///   drops, a lone `\r` PSReadLine would delete), because C1 would bypass that cleanup.
pub(crate) fn powershell_input_line(text: &str) -> std::borrow::Cow<'static, [u8]> {
    if input::psreadline_pastes_it_unchanged(text) {
        std::borrow::Cow::Borrowed(PSREADLINE_PASTE_INPUT)
    } else {
        std::borrow::Cow::Owned(input::input_line_bytes(text))
    }
}

/// **The mark a line of this shell ends with when its command goes on to the next line** (0.4.4
/// ticket 45): cmd's `^`, PowerShell's backtick, a POSIX shell's and fish's backslash. Nushell
/// has none (a command spans lines inside brackets, not after a mark), and an agent's prompt is
/// not a shell.
fn line_continuation_mark(grammar: shell_literal::ShellGrammar) -> Option<char> {
    match grammar {
        shell_literal::ShellGrammar::Cmd => Some('^'),
        shell_literal::ShellGrammar::PowerShell => Some('`'),
        shell_literal::ShellGrammar::Posix
        | shell_literal::ShellGrammar::Fish
        | shell_literal::ShellGrammar::Csh => Some('\\'),
        shell_literal::ShellGrammar::Nushell | shell_literal::ShellGrammar::Agent => None,
    }
}

/// **What the clipboard verbs read of the keyboard**, for the rung that holds it
/// (`Runtime::clipboard_focus`): whether a surface above the clipboard rung
/// swallows every key, the one-line field on the rung, whether the quick edit
/// has the keyboard, and whether the rung is the shell's.
#[must_use]
pub(crate) fn clipboard_focus_of(
    rung: ImeOwner,
    swallowing: bool,
    settings_field: bool,
    preview_edit: bool,
) -> menubar::ClipboardFocus {
    menubar::ClipboardFocus {
        swallowing,
        field: text_field_holding(rung, settings_field),
        preview_edit,
        shell: rung == ImeOwner::Shell,
    }
}

/// **Whether the release may keep the offer the box made** — the whole of the
/// text write's safety, as one comparison of two readings taken at two moments.
///
/// `promised` is what the last survey put on screen; `at_release` is the same
/// question asked again at the point the platform says the hand is, against the
/// window as it stands now. Anything other than "these are the same offer" is a
/// refusal, and a refusal is the dashed outline the drop table already draws —
/// **never a redirection**. A text write that lands somewhere other than where
/// the box said is the one failure this gesture cannot afford, so the rule is
/// equality and not a nearest match.
///
/// **Equality over the whole offer is what makes this one rule and not four.**
/// The landing differing is the hand having moved or the tree having reflowed
/// under it; the tab or the incarnation differing is the pane having been
/// replaced under a still hand. `at_release` being `None` is the aim no longer
/// naming a live shell at all. And a missing `promised` refuses as loudly as a
/// mismatch: it is a release whose box never promised a paste in the first
/// place.
pub(crate) fn paste_offer_survives(
    promised: Option<PasteOffer>,
    at_release: Option<PasteOffer>,
) -> bool {
    promised.is_some() && promised == at_release
}

/// **A text write happens only when the glass is certainly ours** (review
/// 2026-09-17 round 2).
///
/// The opposite default to [`super::mouse::glass_allows_a_drop`], and the asymmetry is the
/// ruling rather than an inconsistency: bytes on a command line are not visible
/// as a mistake and cannot be undone by repeating the gesture, so the answer
/// this window does not have is the answer it refuses on.
///
/// **What it is defending against.** The pointer is captured for the whole of a
/// drag, so a release is delivered to the window that started it wherever the
/// hand actually is — and the survey reads this window's own geometry, which
/// goes on describing a pane that an always-on-top window is sitting in front
/// of. Hover a visible part of a terminal's middle, move into the part another
/// window covers, let go: every identity is unchanged, the plan fits, and
/// without this the path is typed into a terminal nobody could see. Neither the
/// broker's aim nor its pointer closes it — both are refreshed by *delivered*
/// motion, which is the reading the release already distrusts.
pub(crate) const fn glass_allows_a_text_write(glass: GlassHere) -> bool {
    matches!(glass, GlassHere::Ours)
}

/// **What a paste does with what the clipboard answered** — the clipboard lane's `Done` for a
/// terminal paste. A read that failed never reaches here: it is its request's `Failed`, said once by
/// the lane's ledger, and its notice is the window's (`Runtime::spend_paste`).
pub(crate) fn prepare_clipboard_paste(
    payload: bt_platform::ClipboardPayload,
    recipient: &shell_literal::Recipient,
    leading_space: bool,
) -> PreparedClipboardPaste {
    use bt_platform::ClipboardPayload;
    match payload {
        ClipboardPayload::Files(paths) => {
            let insertion = shell_literal::paths_text(&paths, recipient, leading_space);
            PreparedClipboardPaste {
                text: (!insertion.text.is_empty()).then_some(insertion.text),
                notice: shell_literal::refusal_notice(&insertion.refused),
                ..PreparedClipboardPaste::default()
            }
        }
        ClipboardPayload::Text(text) => PreparedClipboardPaste {
            text: Some(text),
            clipboard_text: true,
            ..PreparedClipboardPaste::default()
        },
        // **The third rung, and it is third because the clipboard says so**
        // (§7.61). A source that puts a picture *and* its own text on the board —
        // every browser does — is a text paste, and a source that puts a file
        // beside a thumbnail of it is a path paste. This arm is what is left:
        // a picture and nothing else.
        ClipboardPayload::Picture(offered) => PreparedClipboardPaste {
            picture: offered,
            ..PreparedClipboardPaste::default()
        },
        ClipboardPayload::Nothing => PreparedClipboardPaste::default(),
        ClipboardPayload::Refused(bt_platform::UnsupportedKind::Promise) => {
            PreparedClipboardPaste {
                notice: Some(i18n::Text::PasteClipboardPromise.text().to_owned()),
                ..PreparedClipboardPaste::default()
            }
        }
    }
}

/// Deliver one string to a shell the way a paste is delivered.
///
/// Shared by terminal clipboard routing and K144 rather than copied because the files
/// tree's `Insert path into terminal` (K144) is a paste in every respect that
/// matters to the shell and to the view — the selection goes, the view returns
/// to the bottom, the bytes are bracketed if the shell asked for bracketing, and
/// they go down the one door onto the child's input.
/// The only thing that differs is where the string came from, which is the one
/// thing this function does not ask.
///
/// **One write, whatever the clipboard held.** This used to hand the bytes over in 16 KiB
/// chunks, and the reason it gave was true at the time: the write was synchronous, so chunking
/// it was how ConPTY's backpressure reached us a piece at a time instead of as one unbounded
/// call. The wait now lives on `bt_pty`'s writer thread, which makes chunking here strictly
/// worse — a paste split into sixty pieces is sixty chances for the child to stop reading
/// *between* them, and `bt_pty::InputRing` only promises a whole write, never a partial one. As
/// one write it either reaches the shell entire or is refused entire, which is the only pair of
/// outcomes a pasted command line can survive.
pub(crate) fn paste_text<T>(
    session: &mut DualPlaneSession,
    projection: &mut ViewportProjection,
    text: &str,
    write: impl FnOnce(&[u8]) -> Result<T>,
) -> Result<T> {
    let bytes = input::paste_bytes(text, session.bracketed_paste_mode());
    paste_spelled(session, projection, &bytes, write)
}

/// **The rule [`Runtime::live_paste_target`] turns on**, as a function of the
/// two facts it reads and nothing else (review X-1).
///
/// A named function rather than a run of `if`s inside the method, on the wheel
/// ruling's own footing: this is a claim about a decision, and a decision that
/// cannot be named cannot be pinned. Everything the method needs a window for —
/// the tab list, the session map — happens before this is called, so the part
/// that is a rule is testable and the part that is a lookup is not asked to be.
///
/// `standing` is the incarnation of whatever shell is in that seat now, and
/// `None` is "no shell there at all" — a pane that was closed, or a seat that
/// was never a terminal. Both answer the same way, because both mean the
/// address names nothing.
pub(crate) const fn paste_target_is_live(
    on_top: TabId,
    standing: Option<u64>,
    target: PasteTarget,
) -> bool {
    on_top.0 == target.tab.0
        && match standing {
            Some(incarnation) => incarnation == target.incarnation,
            None => false,
        }
}

/// **Decide, and hold the paste if the answer is "ask"** — `Runtime::deliver_paste`'s decision,
/// with the window taken out so the rule and its effect can be run on a real tab.
///
/// **A second multi-line paste replaces the pending one** (design note §2): a question about text
/// nobody answered is not a question anyone asked, so every other leaf's pending paste in this tab
/// is dropped as the new one is held. The card is modal, so in practice there is never a first.
pub(crate) fn stage_paste(
    tab: &mut TabState,
    target: PasteTarget,
    text: String,
    clipboard_text: bool,
    ask: bool,
    host: bt_platform::HostPlatform,
    context: &'static str,
) -> StagedPaste {
    let Some(leaf) = tab.sessions.get(&target.seat) else {
        return StagedPaste::Send(text);
    };
    let mark = line_continuation_mark(leaf.launch.paste_recipient.encoder.grammar);
    match paste_road(&text, PasteFacts::of(leaf, clipboard_text, ask, host)) {
        PasteRoad::AsTyped => StagedPaste::Send(text),
        PasteRoad::InputLine => StagedPaste::InputLine(powershell_input_line(&text)),
        PasteRoad::Ask { lines } => {
            for leaf in tab.sessions.values_mut() {
                leaf.pending_paste = None;
            }
            if let Some(leaf) = tab.sessions.get_mut(&target.seat) {
                let continued = mark.filter(|mark| input::continued_by(&text, *mark));
                leaf.pending_paste = Some(PendingPaste {
                    target,
                    text,
                    lines,
                    continued,
                    moved_focus: None,
                    context,
                });
            }
            StagedPaste::Held
        }
    }
}

/// The pane in this tab holding a paste that waits for an answer, and the paste.
pub(crate) fn pending_paste_in(tab: &TabState) -> Option<(SeatId, &PendingPaste)> {
    tab.sessions
        .iter()
        .find_map(|(seat, leaf)| leaf.pending_paste.as_ref().map(|pending| (*seat, pending)))
}

/// Take the waiting paste out of this tab, which is what every answer does first — `Cancel`
/// included, and a spent answer cannot be spent twice.
pub(crate) fn take_pending_paste(tab: &mut TabState) -> Option<PendingPaste> {
    tab.sessions
        .values_mut()
        .find_map(|leaf| leaf.pending_paste.take())
}

/// **What a spent answer writes**, or nothing for `Cancel`.
pub(crate) fn paste_answer_text(pending: &PendingPaste, answer: PasteAnswer) -> Option<String> {
    match answer {
        PasteAnswer::RunLineByLine => Some(pending.text.clone()),
        PasteAnswer::Join => Some(match pending.continued {
            Some(mark) => input::join_continued_lines(&pending.text, mark),
            None => input::join_lines(&pending.text),
        }),
        PasteAnswer::Cancel => None,
    }
}

/// **One key on the card, spent on the pending paste** — the focus moves on the paste itself, and
/// an answer comes back for the caller to spend. The whole of the card's keyboard, so a test runs
/// the very step the window runs.
pub(crate) fn paste_card_step(
    pending: &mut PendingPaste,
    key: PasteCardKey,
) -> Option<PasteAnswer> {
    match key {
        PasteCardKey::Answer(answer) => Some(answer),
        PasteCardKey::Focus(to) => {
            pending.moved_focus = Some(to);
            None
        }
    }
}

/// Whether the cell in front of the cursor is something the path would be
/// welded onto.
///
/// **The screen is the only honest witness.** The mock-up could ask its model
/// what the input line held; a real shell keeps that string inside itself, and
/// nothing this window can ask will get it. What this window *does* own is the
/// grid the shell has drawn on, and the character immediately left of the cursor
/// is the last one the input line put there — which is precisely the thing the
/// rule is about. At column zero there is nothing in front to weld to, and an
/// unwritten cell reads as a blank, so both answer "no space needed" without a
/// special case.
pub(crate) fn input_line_needs_a_space_first(session: &bt_term::DualPlaneSession) -> bool {
    let cursor = session.terminal().cursor();
    let Some(column) = (cursor.column as usize).checked_sub(1) else {
        return false;
    };
    session
        .terminal()
        .visible_row(cursor.row)
        .and_then(|row| row.cells.get(column).map(|cell| cell.text.clone()))
        .is_some_and(|text| !text.chars().all(char::is_whitespace) && !text.is_empty())
}

/// **Every condition a dragged path must clear before a byte is written**, in
/// one place and as a function of four plain facts.
///
/// **`modal` is the fifth** (review 2026-09-17 P1-b), and it is here rather than
/// only on the external road because the two roads must answer one question the
/// same way. A press cannot *start* a row drag under a modal — `chrome_mouse_input`
/// swallows every press for the quit card, the dirty gate and the first-run card
/// and returns above the drag routing — but a card that came up *during* a drag
/// leaves a gesture in flight over a window that is now waiting for a keystroke,
/// and the keystroke after a drop is `Enter`.
///
/// Written as one function so that the answer can be read, and tested, without
/// a window — and so that no caller can satisfy four of the five and reach the
/// write. [`Runtime::paste_offer_kept`] is what gathers the facts; this is what
/// they mean, and it hands back the **address** rather than a `bool` so that a
/// caller cannot take the verdict from here and the destination from somewhere
/// older.
pub(crate) fn paste_offer_is_kept(
    glass: GlassHere,
    promised: Option<PasteOffer>,
    at_release: Option<PasteOffer>,
    plan_fits: bool,
    modal: bool,
) -> Option<PasteTarget> {
    let at_release = at_release?;
    (glass_allows_a_text_write(glass)
        && plan_fits
        && !modal
        && paste_offer_survives(promised, Some(at_release)))
    .then_some(at_release.target)
}

/// **The text a copy of this pane's selection puts on the clipboard**, or `None` when there is no
/// selection — or, for copy-on-select (`ignore_empty`), when it is empty. `Ctrl+C` writes an
/// empty selection as the empty text it is.
pub(crate) fn selection_to_copy(session: &DualPlaneSession, ignore_empty: bool) -> Option<String> {
    session
        .selection_text()
        .filter(|text| !(ignore_empty && text.is_empty()))
}

/// **`Ctrl+C`'s selection, taken down once its copy has landed** — only while the selection is
/// still the text that was copied: a selection made again between the press and the clipboard's
/// answer is the person's newer one, and stays. Answers whether it was taken down.
pub(crate) fn clear_copied_selection(
    session: &mut DualPlaneSession,
    projection: &mut ViewportProjection,
    copied: &str,
) -> bool {
    if session.selection_text().as_deref() != Some(copied) {
        return false;
    }
    session.port_clear_selection();
    projection.set_selection(None);
    true
}

/// **A dropped file's path is a copied file's path** (GitHub issue #1 ②).
///
/// One line, and the line is the whole point: a drop hands this process the same
/// thing a copy does — a list of paths — so it is turned into the same payload
/// and spelled by the same code. Nothing here decides anything. The day somebody
/// wants a drop quoted differently from a copy, this function is where that
/// decision would have to be written down, and
/// `a_dropped_path_is_spelled_like_a_copied_one` is what would go red first.
pub(crate) fn prepare_dropped_paste(
    paths: Vec<PathBuf>,
    recipient: &shell_literal::Recipient,
    leading_space: bool,
) -> PreparedClipboardPaste {
    prepare_clipboard_paste(
        bt_platform::ClipboardPayload::Files(paths),
        recipient,
        leading_space,
    )
}

/// [`paste_text`] or [`paste_spelled`], as the body says.
pub(crate) fn paste_body<T>(
    session: &mut DualPlaneSession,
    projection: &mut ViewportProjection,
    body: PasteBody<'_>,
    write: impl FnOnce(&[u8]) -> Result<T>,
) -> Result<T> {
    match body {
        PasteBody::Text(text) => paste_text(session, projection, text, write),
        PasteBody::InputLine(bytes) => paste_spelled(session, projection, bytes, write),
    }
}

/// **Why a paste still owed is withdrawn** ([`Runtime::settle_clipboard`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Withdrawn {
    /// Its pane closed, was restarted or left the front; its field or page let the keyboard go.
    TargetGone,
    /// It has been owed for [`crate::clipboard_lane::PASTE_DEADLINE`].
    Deadline,
    /// What its pane holds behind it has passed `bt_pty::PTY_INPUT_RING_BYTES`.
    HeldOverTheCap,
}

impl Withdrawn {
    /// The reason, in the words `diagnostics.log` says.
    fn words(self) -> &'static str {
        match self {
            Self::TargetGone => "what it was aimed at has gone or let the keyboard go",
            Self::Deadline => "the clipboard did not answer within a second",
            Self::HeldOverTheCap => "the input typed behind it passed what a pane holds",
        }
    }

    /// Whether the person is told on screen that the clipboard was busy: when the clipboard is
    /// what kept the paste, not the person's own moving on.
    fn is_the_clipboard_busy(self) -> bool {
        matches!(self, Self::Deadline | Self::HeldOverTheCap)
    }
}

/// **What `Copy` on the execution-policy row puts on the clipboard, and what the window says once
/// it has landed**: exactly [`crate::shell_integration::POLICY_COMMAND`], and the toast that names
/// it in full.
fn policy_command_copy() -> (String, ClipboardWriteEffect) {
    (
        crate::shell_integration::POLICY_COMMAND.to_owned(),
        ClipboardWriteEffect::Toast {
            anchor: toast::ToastAnchor::Window,
            text: crate::i18n::graph_copied(crate::shell_integration::POLICY_COMMAND),
        },
    )
}

#[cfg(test)]
mod policy_command_tests {
    use super::policy_command_copy;
    use crate::ClipboardWriteEffect;
    use crate::shell_integration::POLICY_COMMAND;

    /// PIN — **`Copy` puts exactly the one command on the clipboard, and says it in full**
    /// (T-INTEGRATION-INJECT-4 round 4), once the write has landed — the effect a landed write
    /// applies (`Runtime::answer_copy`); a refused write applies none. Never the real clipboard.
    ///
    /// RED (mutation: copy `"Set-ExecutionPolicy RemoteSigned"`, the LocalMachine scope that
    /// needs elevation, instead of the const).
    #[test]
    fn copy_puts_exactly_the_policy_command_on_the_clipboard() {
        let (written, effect) = policy_command_copy();
        assert_eq!(
            written,
            "Set-ExecutionPolicy -Scope CurrentUser RemoteSigned"
        );
        assert_eq!(written, POLICY_COMMAND);
        let ClipboardWriteEffect::Toast { text: said, .. } = effect else {
            panic!("the landed copy says what it copied");
        };
        assert_eq!(said, crate::i18n::graph_copied(POLICY_COMMAND));
        assert!(said.contains(POLICY_COMMAND));
    }
}

#[cfg(test)]
#[path = "clipboard_tests.rs"]
mod tests;
