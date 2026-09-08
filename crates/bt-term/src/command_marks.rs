//! The per-session ledger of shell commands: one record per `A/B/C/D` cycle the shell reported.
//!
//! This is the data the command-marks rail stands on (DESIGN §7.1.5c: "刻度=OSC 133 命令边界+转录
//! 逻辑行锚(G3)，错误红=退出码非 0"). Before it there was nowhere to read a per-command anything:
//! the session kept `failure_exit_code`, a single scalar every command overwrote, so a rail asked
//! to colour its third tick red had literally no source of truth to consult.
//!
//! **What it is not.** It is not a second semantic-region table. `SemanticInputRegion` and
//! `SemanticOutputRegion` exist to gate decorations — "does this decoration touch what the user
//! typed", "was this line printed by a command" — and are split by that question's polarity. This
//! ledger answers a third, unrelated question: *which commands has this session run, in order, and
//! how did each end*. It shares the regions' anchors rather than duplicating them (see
//! [`CommandMark::start`]) precisely so there is still exactly one mechanism keeping a coordinate
//! alive across reflow, migration and eviction.
//!
//! **Honesty, restated as a property of this file.** Nothing here infers. A shell that emits no
//! OSC 133 — a PowerShell whose profile never installed the integration, a WSL distribution that
//! logs into zsh — produces an empty ledger, and the rail then draws nothing and reports no error
//! (inventory C13). There is no prompt-shaped-line heuristic, no "the last line before output was
//! probably the command", no exit-code guess from the text. A terminal that guesses which of your
//! commands failed is worse than one that admits it was never told.
//!
//! A shell that emits *some* of them is served to exactly the depth it spoke to, and that is the
//! same rule rather than an exception to it: `cmd.exe` reports its prompt boundaries and nothing
//! else, so its records carry a prompt and an end and no exit code, no command text and no
//! duration — see [`CommandMarkLedger::note_prompt`].

use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

use bt_doc::AnchorId;

/// Identity of one command in this session's ledger. Monotonic, never reused, and stable across
/// every migration a mark's anchors go through — so a painter may cache by it and a rail tick may
/// hold onto it between frames.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CommandMarkId(pub u64);

/// One command, as the shell described it.
///
/// **The anchor fields are `AnchorId`, not `ContentAnchor`, and that is the whole design.** A
/// `ContentAnchor` is a coordinate — a grid point, a staging offset, a transcript offset — and it
/// is exactly the thing that stops being true the moment the grid reflows or the row scrolls out.
/// An `AnchorId` is a registration in [`bt_doc::HistoryDocument`]'s anchor registry, and the
/// document's own transactions rewrite every registered anchor in step with the content it names:
/// `capture_rows_transaction` moves Live to Staging, `finalize_transaction` moves Staging to
/// History, `delete_transaction` degrades what is deleted. Storing a snapshot instead would mean
/// re-deriving all of that here, which is the second mechanism this file exists to avoid.
/// Resolve one with `DualPlaneSession::command_mark_anchor` at the moment you need a coordinate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandMark {
    pub id: CommandMarkId,
    /// Where `A` put the prompt.
    ///
    /// `Option` because `B` without a preceding `A` is a real thing a real shell does — the marker
    /// handler names the case ("a shell that skips markers still tells us here that the command
    /// being typed is not output") and tolerates it. Manufacturing a prompt anchor for those marks
    /// would be a fallback dressed as data: the rail would draw a tick pointing at a prompt line
    /// nobody ever reported.
    pub prompt: Option<AnchorId>,
    /// Where `B` put the start of the typed command — **the same registration the command's
    /// `SemanticInputRegion` uses.**
    ///
    /// Sharing it is not a shortcut, it is the point. That anchor is already the one the resize
    /// path re-seats by content witness (`semantic_witness_rematch`) when the vendor reflows the
    /// live grid, so a mark whose command is still on screen survives a width change for the same
    /// reason the decoration gate does. A privately registered twin would ride the document's
    /// migrations but silently miss the reflow re-match, and would be wrong only on the one path
    /// nobody tests by hand.
    pub start: AnchorId,
    /// Where `C` said output begins. `None` while the command is still being typed, and for a
    /// command abandoned at the prompt (Ctrl+C) before it ever ran.
    pub executed: Option<AnchorId>,
    /// Where `D` said the command ended. `None` means **in flight** — the rail shows the tick, it
    /// simply has no ending to colour yet.
    pub finished: Option<AnchorId>,
    /// The command line itself, as the terminal saw it, trimmed. Empty when unknown — a `C` that
    /// arrived while the region's start had already scrolled off the grid leaves nothing to read,
    /// and an empty string is the honest answer there.
    pub command_text: String,
    /// The exit status `D` carried. `None` covers three different silences that a rail must treat
    /// alike: still running, ended without a `D`, and a `D` that carried no status parameter.
    pub exit_code: Option<i32>,
    /// When `C` was seen, and when `D` was — the two ends of [`Self::duration`].
    ///
    /// **`C` and not `B`**, which is the whole of the definition. `B` is when the shell began
    /// *reading* a line, and the gap between `B` and `C` is the user thinking, going for coffee,
    /// or leaving the pane open overnight; a card that reported it would call a one-second `ls`
    /// a nine-hour command. `C` is when the shell said "this is submitted, output starts here",
    /// so `C..D` is the only span in the `A/B/C/D` cycle that is the command *running*.
    ///
    /// [`Instant`] rather than a wall clock: this is an elapsed time and nothing else, and a
    /// monotonic reading is the one that cannot be moved by an NTP step or a daylight-saving
    /// boundary halfway through a build. It is therefore also not persistable, which is correct —
    /// a mark does not outlive its session.
    ///
    /// Both stay `None` for a command that never reached the marker that sets them: a `B` straight
    /// to `D` has no `executed_at`, and a command still running has no `finished_at`.
    pub executed_at: Option<Instant>,
    pub finished_at: Option<Instant>,
    /// **Whether the reader's own keyboard reached this pane while this command
    /// was being read** (review row R1-24).
    ///
    /// [`Self::command_text`] is whatever the terminal saw between the shell's
    /// `OSC 133;B` and the `C` after it — and a *program* can print those marks
    /// with any text it likes between them, so the text is a claim by whoever was
    /// writing to the screen rather than a record of what anybody typed. That is
    /// harmless while the text is only drawn on a card; it stops being harmless
    /// the moment the text is put back onto a prompt for the reader to press
    /// Enter on, which is what a restored summoned terminal does with it.
    ///
    /// This is the fact that separates the two: bytes reached the pty from the
    /// reader's keyboard, paste or IME between this mark opening and it ending.
    /// It is not a claim that the text *is* what they typed — a shell may echo
    /// anything — only that they were at this prompt. Nothing that is not offered
    /// back to the keyboard consults it.
    pub typed_by_user: bool,
}

impl CommandMark {
    /// Has this command not ended yet? (`B` seen, no `D`.)
    pub fn is_running(&self) -> bool {
        self.finished.is_none()
    }

    /// Did this command report a non-zero exit status? This is the `.cmdtick.fail` predicate — the
    /// one signal that earns permanent colour at rest.
    pub fn failed(&self) -> bool {
        self.exit_code.is_some_and(|code| code != 0)
    }

    /// Has this mark been told anything at all? No text read off the line, no `C` that submitted
    /// it, no `D` that ended it — a record that is nothing but the fact that a prompt was drawn.
    ///
    /// This is the whole of the redraw rule in [`CommandMarkLedger::open_command`], and the reason
    /// it is stated as "nothing recorded" rather than "not executed": a command abandoned at the
    /// prompt has not executed either, but it *does* carry the text that was typed, and that text
    /// is a fact about a real line the reader wrote. A repaint carries nothing, and an empty record
    /// was never a command.
    ///
    /// **The second reader is the rail's glance card** (user report 2026-09-07, §7.57 ⑧), and it is
    /// why the predicate is public and named for the state rather than for the reclaim. Opening a
    /// record at `A` gives every prompt a tick the moment it is drawn, and such a record has no `D`
    /// — so [`Self::is_running`], whose whole test is `finished.is_none()`, answers yes and the card
    /// said *running · command* over a prompt nobody had typed a character into. Both halves were
    /// true and the sentence they made was not. What this predicate says about such a record is
    /// what the card now says about it: the shell is standing at this prompt.
    #[must_use]
    pub fn is_at_the_prompt(&self) -> bool {
        self.executed.is_none() && self.finished.is_none() && self.command_text.is_empty()
    }

    /// Every anchor this mark holds, including the one it shares with its input region.
    fn anchors(&self) -> BTreeSet<AnchorId> {
        self.prompt
            .into_iter()
            .chain(std::iter::once(self.start))
            .chain(self.executed)
            .chain(self.finished)
            .collect()
    }

    /// The anchors this mark asked the document for on its own account.
    ///
    /// [`Self::start`] is deliberately absent: `B` hands the mark the registration its input
    /// region already made, so the region is what releases it. Everything else — the prompt `A`
    /// reported, and the coordinates `C` and `D` named — was registered for this mark and nothing
    /// else, and goes when the mark stops naming it.
    fn own_anchors(&self) -> Vec<AnchorId> {
        self.prompt
            .into_iter()
            .chain(self.executed)
            .chain(self.finished)
            .collect()
    }

    /// How long this command ran: `C` to `D`.
    ///
    /// `None` for anything that did not have both ends — still running, never executed, or a shell
    /// that skipped `C`. There is no elapsed-so-far reading for a running command here on purpose:
    /// that is a number that changes between two reads of the same mark, and a ledger that answered
    /// it would be answering with the clock rather than with what it was told.
    pub fn duration(&self) -> Option<Duration> {
        let (executed, finished) = (self.executed_at?, self.finished_at?);
        finished.checked_duration_since(executed)
    }
}

/// The session's ledger.
///
/// **Primary screen only, and there is deliberately no `screen` field to say so.** DESIGN §3.2
/// keeps the alternate screen in an isolated namespace — selection and search never cross into it,
/// its anchors are not even orderable against the document's — and a full-screen TUI that emits
/// `A/B/C/D` for its own internal redraws is describing its own canvas, not this session's command
/// history. So alternate-screen markers are dropped at the door by the caller and every mark in
/// here is a primary-screen mark by construction. A field with one legal value would only invite
/// someone to write the other one.
#[derive(Clone, Debug, Default)]
pub struct CommandMarkLedger {
    marks: Vec<CommandMark>,
    /// The command that has begun and not ended: `B` seen, `D` not yet. At most one, because the
    /// shell runs one command at a time and every marker that could end one clears it.
    open: Option<CommandMarkId>,
    /// The anchor registered at the last `A`, waiting for its `B` to claim it.
    pending_prompt: Option<AnchorId>,
    next_id: u64,
    revision: u64,
    /// Anchors this ledger asked for and no longer holds, waiting for the session to give them
    /// back to the document.
    ///
    /// The ledger cannot release them itself — it holds ids, not the registry — and the registry
    /// cannot release them on its own either, because a deleted line's anchors are degraded onto a
    /// neighbour rather than removed and afterwards nothing in the registry can tell a live holder
    /// from a departed one. So the ledger says what it dropped and
    /// `DualPlaneSession::release_retired_mark_anchors` hands it over.
    ///
    /// [`CommandMark::start`] is never in here: that registration belongs to the command's input
    /// region, which releases it when the region leaves.
    orphaned: Vec<AnchorId>,
}

impl CommandMarkLedger {
    /// Oldest first. This ordering *is* the rail's ordinal stack ("oldest at the top — position
    /// carries order, not scroll geometry").
    pub fn marks(&self) -> &[CommandMark] {
        &self.marks
    }

    /// Take the anchors this ledger has stopped holding, for the session to release.
    pub fn take_released_anchors(&mut self) -> Vec<AnchorId> {
        std::mem::take(&mut self.orphaned)
    }

    /// Note that `held` are no longer held by any mark, minus the ones still standing.
    fn orphan(&mut self, held: Vec<AnchorId>, kept: &BTreeSet<AnchorId>) {
        self.orphaned
            .extend(held.into_iter().filter(|anchor| !kept.contains(anchor)));
    }

    pub fn get(&self, id: CommandMarkId) -> Option<&CommandMark> {
        self.marks.iter().find(|mark| mark.id == id)
    }

    /// Bumped by every change to the ledger's contents and by nothing else — not by a frame, not by
    /// output arriving, not by a scroll. A painter that caches its tick geometry can compare this
    /// and skip the whole rebuild, which is the only reason it exists.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// `A`. Open a mark at the prompt — and remember the anchor, so that the `B` which may follow
    /// claims this record rather than opening a second one.
    ///
    /// **A prompt is a place worth going back to even from a shell that never says where its input
    /// starts** (2026-09-07). `A` and `B` used to be one thing here — the record began at `B`,
    /// because every shell this terminal served sent both — and a shell with only `A` therefore
    /// left a ledger that was empty however many commands it had run. `cmd.exe` is that shell:
    /// `PROMPT` is expanded once, just before a line is read, so `A` and `D` are the two markers it
    /// can carry and `B` and `C` are the two it cannot
    /// (`bt_app::profiles::Integration::CmdPrompt`). The rail's tick answers "which command", and
    /// the prompt row *is* the answer to that; what `B` adds is a finer coordinate, not the fact.
    ///
    /// So the record starts here, with the prompt's own anchor standing in for
    /// [`CommandMark::start`], and a `B` that arrives afterwards finds a blank draft and takes it
    /// over — the same reclaim a redrawn prompt already relied on, reached by the same route. For
    /// a shell that sends both, the ledger is byte for byte what it was: one record per command,
    /// `start` at `B`, `prompt` at `A`.
    pub fn note_prompt(&mut self, prompt: AnchorId) {
        self.pending_prompt = Some(prompt);
        self.open_command(prompt);
    }

    /// Is there a command in flight? Callers use this to avoid registering an anchor for a marker
    /// that has no mark to attach it to.
    pub fn has_open_command(&self) -> bool {
        self.open.is_some()
    }

    /// `B`. Open a mark — or reclaim the blank one a redrawn prompt left behind. It is visible in
    /// [`Self::marks`] immediately, with `finished: None`.
    ///
    /// **A prompt drawn again for a line that has not run yet is the same command.** A line editor
    /// repainting its input — PSReadLine's `InvokePrompt`, which this terminal asks for by chord
    /// after every resize, and the equivalent every other editor has — re-runs the prompt function,
    /// so `A` and `B` arrive again for a line the reader is still typing. The shell is not
    /// describing a second command there, it is describing the same one at its new coordinates: the
    /// only `C` and `D` this line will ever produce will name the record the *last* repaint opened,
    /// so a record per `B` is a record that can never be completed. Dragging a window while typing
    /// used to leave one such blank card in the rail per repaint.
    ///
    /// Reclaiming is therefore not a filter over what the ledger shows — a filter would have to
    /// keep deciding, frame after frame, whether a record is still worth hiding, and the empty
    /// records would still be there to be counted, stepped through and jumped to. There is one
    /// record because there was one command.
    ///
    /// The reclaimed mark keeps its `id` — a rail tick or a painter's cache holding it across the
    /// repaint is still holding the same command — and takes the redrawn prompt's own coordinates,
    /// which are the ones that describe where the line now is. The old ones named cells the
    /// repaint erased.
    ///
    /// Nothing here asks *why* the prompt was drawn again, because OSC 133 does not say and no
    /// shell can be relied on to. What separates a repaint from an abandoned command is what the
    /// mark was told, not what the shell meant: see [`CommandMark::is_at_the_prompt`].
    pub fn open_command(&mut self, start: AnchorId) -> CommandMarkId {
        let prompt = self.pending_prompt.take();
        if let Some(mark) = self.marks.last_mut().filter(|mark| mark.is_at_the_prompt()) {
            // A `B` with no `A` before it leaves the prompt this mark already had: the repaint
            // reported no new one, so the old one is still the best coordinate we were given.
            let dropped = mark.own_anchors();
            mark.prompt = prompt.or(mark.prompt);
            mark.start = start;
            let kept = mark.anchors();
            let id = mark.id;
            self.orphan(dropped, &kept);
            self.open = Some(id);
            self.revision += 1;
            return id;
        }
        self.next_id += 1;
        let id = CommandMarkId(self.next_id);
        self.marks.push(CommandMark {
            id,
            prompt,
            start,
            executed: None,
            finished: None,
            command_text: String::new(),
            exit_code: None,
            executed_at: None,
            finished_at: None,
            typed_by_user: false,
        });
        self.open = Some(id);
        self.revision += 1;
        id
    }

    /// `C`. The command was submitted; output starts here, and the clock starts here.
    pub fn note_executed(&mut self, executed: AnchorId, at: Instant) {
        let Some(mark) = self.open_mark_mut() else {
            return;
        };
        mark.executed = Some(executed);
        mark.executed_at = Some(at);
        self.revision += 1;
    }

    /// The typed command line, read off the input region once it has closed.
    ///
    /// First writer wins. `C` is the authoritative close for a command's text, and every later
    /// refresh may only confirm it — the same attestation rule `refresh_semantic_input_witness`
    /// states for the witness this text comes from, for the same reason: after the close, a span
    /// carried through a reflow is permitted to attest to the same bytes, never to replace them.
    pub fn note_command_text(&mut self, text: String) {
        let Some(mark) = self.open_mark_mut() else {
            return;
        };
        if !mark.command_text.is_empty() || text.is_empty() {
            return;
        }
        mark.command_text = text;
        self.revision += 1;
    }

    /// **The reader's own bytes went into this pane** (review row R1-24).
    ///
    /// Booked against the mark that is open, which is the command the shell says
    /// it is reading right now — so a keystroke between `B` and `C` marks that
    /// command and a keystroke arriving while nothing is open marks nothing.
    /// There is no marker for it and there could not be: it is a fact about this
    /// process's own input, and the whole point is that a program writing to the
    /// screen cannot produce it.
    ///
    /// Idempotent, and cheap enough to call on every keystroke: one branch and a
    /// store, with no revision bump, because nothing on screen is drawn from it.
    pub fn note_user_input(&mut self) {
        let Some(mark) = self.open_mark_mut() else {
            return;
        };
        mark.typed_by_user = true;
    }

    /// `D`. The command ended, with whatever status it reported.
    pub fn note_finished(&mut self, finished: AnchorId, exit_code: Option<i32>, at: Instant) {
        let Some(mark) = self.open_mark_mut() else {
            return;
        };
        mark.finished = Some(finished);
        mark.exit_code = exit_code;
        mark.finished_at = Some(at);
        self.open = None;
        self.revision += 1;
    }

    /// A new prompt arrived while a command was still open — the command ended without ever saying
    /// `D` (Ctrl+C at the prompt, a shell that skips `D`, a `A` emitted mid-flight).
    ///
    /// The mark stays exactly as it is, `finished: None` and all: we were not told how it ended, so
    /// we do not say. Only the "which mark do further markers belong to" pointer is released, and
    /// since nothing readable changed, the revision does not move.
    pub fn release_open_command(&mut self) {
        self.open = None;
    }

    /// Drop the named marks. A no-op — which is the overwhelmingly common case, since most
    /// deletions touch no command's line — costs nothing and does not move the revision.
    ///
    /// The caller decides which are doomed, because only it can see the deletion in progress: by
    /// the time `HistoryDocument::delete_transaction` has run, a deleted line's anchors have been
    /// *degraded* onto a surviving successor rather than removed, and a mark asked afterwards
    /// whether it is still alive would confidently answer yes while pointing at somebody else's
    /// command.
    pub fn retire(&mut self, doomed: &BTreeSet<CommandMarkId>) {
        if doomed.is_empty() {
            return;
        }
        let before = self.marks.len();
        let dropped = self
            .marks
            .iter()
            .filter(|mark| doomed.contains(&mark.id))
            .flat_map(CommandMark::own_anchors)
            .collect::<Vec<_>>();
        self.marks.retain(|mark| !doomed.contains(&mark.id));
        if self.marks.len() == before {
            return;
        }
        let kept = self
            .marks
            .iter()
            .flat_map(CommandMark::anchors)
            .collect::<BTreeSet<_>>();
        self.orphan(dropped, &kept);
        if self.open.is_some_and(|open| doomed.contains(&open)) {
            self.open = None;
        }
        self.revision += 1;
    }

    fn open_mark_mut(&mut self) -> Option<&mut CommandMark> {
        let open = self.open?;
        self.marks.iter_mut().find(|mark| mark.id == open)
    }
}
