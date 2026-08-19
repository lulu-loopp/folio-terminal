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
//! OSC 133 — `cmd.exe`, or a PowerShell whose profile never installed the integration — produces an
//! empty ledger, and the rail then draws nothing and reports no error (inventory C13). There is no
//! prompt-shaped-line heuristic, no "the last line before output was probably the command", no
//! exit-code guess from the text. A terminal that guesses which of your commands failed is worse
//! than one that admits it was never told.

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
    fn is_blank_draft(&self) -> bool {
        self.executed.is_none() && self.finished.is_none() && self.command_text.is_empty()
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
}

impl CommandMarkLedger {
    /// Oldest first. This ordering *is* the rail's ordinal stack ("oldest at the top — position
    /// carries order, not scroll geometry").
    pub fn marks(&self) -> &[CommandMark] {
        &self.marks
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

    /// `A`. Remember where the prompt started; the next `B` claims it.
    ///
    /// No revision bump: nothing observable through [`Self::marks`] has changed yet. A prompt with
    /// no command after it is not a command.
    pub fn note_prompt(&mut self, prompt: AnchorId) {
        self.pending_prompt = Some(prompt);
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
    /// mark was told, not what the shell meant: see [`CommandMark::is_blank_draft`].
    pub fn open_command(&mut self, start: AnchorId) -> CommandMarkId {
        let prompt = self.pending_prompt.take();
        if let Some(mark) = self.marks.last_mut().filter(|mark| mark.is_blank_draft()) {
            // A `B` with no `A` before it leaves the prompt this mark already had: the repaint
            // reported no new one, so the old one is still the best coordinate we were given.
            mark.prompt = prompt.or(mark.prompt);
            mark.start = start;
            let id = mark.id;
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
        self.marks.retain(|mark| !doomed.contains(&mark.id));
        if self.marks.len() == before {
            return;
        }
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
