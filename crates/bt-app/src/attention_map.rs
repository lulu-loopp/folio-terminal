//! **The adapters, and they are data.**
//!
//! `docs/plans/attention/plan.md` §11.10.3 is the ruling this file is the execution of: eight agent
//! CLIs surveyed, three shapes found, and the largest of the three — an upstream event reaching us
//! over the pane's own pipe — needs **no code per family**. What a family costs is a block of rows
//! below and a configuration template beside it. There is no `match` on a family name anywhere in
//! this crate, and the test at the bottom that adds a family and asserts nothing else changed is
//! the pin on that.
//!
//! # The four declared columns (§12.1 R1)
//!
//! Every row states its `kind`, whether the family gives it a stable **identifier**, and — for a
//! wait — which **tier** it belongs to, or — for a clear — which **class** it is. Three of those
//! four keep being got wrong by inference rather than by declaration, and each has a written
//! consequence:
//!
//! * **`id`** is `None` unless the upstream document has been quoted verbatim. Today that is every
//!   row (§12.1.6), so every kind runs on the level path, where sameness is decided by the ledger's
//!   own watermark rather than by a key we hoped was stable. `None` is not a gap — it is the answer
//!   that carries a written-down behaviour.
//! * **`tier`** is why the catalogue below is a *menu* rather than a configuration. The zero-delay
//!   event and the six-second notification describe the **same** request; installing both is how
//!   one request becomes two credentials. [`installed_rows`] picks one per kind, and which one is a
//!   question for the program itself rather than for a version number we guessed at.
//! * **`clear-class`** is [`ClearClass`], and its three values are the answer to "how badly could
//!   this remove the wrong thing" rather than to "is it a receipt".
//!
//! # The other lane (§11.6)
//!
//! [`OSC_ROWS`] is the second table, and it is about the programs that need no adapter at all: a
//! build script, a shell function, anything holding its own tty writes a few bytes and is heard.
//! Its rows declare two columns rather than four, because the questions the other two answer cannot
//! be asked of a wire that carries no identity — and the one thing they do declare is the level,
//! which is the only genuine decision on that lane. `bt-term` reads the bytes and stops; **what a
//! sequence is worth is data here, not a branch there.**
//!
//! # What is not here, and why
//!
//! **The end of a turn.** `Stop`, codex's `agent-turn-complete`, pi's `agent_settled` — none of
//! them is a wait, and the plan spent four recordings proving that a turn ending is not the same
//! sentence as *it is standing there waiting for you*. They are in [`TURN_END`], a second and
//! deliberately separate table, because the moment they share a table with the waits somebody will
//! one day give one of them a `kind` and the whole block will have regressed to what it was.
//! `Stop` and `StopFailure` appear in **both** tables and that is not a contradiction: as clears
//! they end every wait this pane was holding, and as announcements they reach the desktop. Neither
//! of those is "the pane is waiting for you".
//!
//! **pi has no wait rows at all**, and that is a finding rather than an omission (§12.3). The one
//! event its survey turned up fires "only once a run fully settles", which is the end of a turn.
//!
//! **copilot CLI is not here yet, and this is the open account.** §11.10.3 files it as a family of
//! this shape, on two notification subtypes; §12.3 then withdrew a third for naming the same thing
//! Claude Code's `idle_prompt` names, which a ruling had already refused. What its two remaining
//! subtypes and its `agent_completed` / `shell_completed` clears say **verbatim** has not been
//! fetched under §10.4's rule, and this file's own discipline is that a row is written from a
//! quotation or not at all. When that quotation is taken, copilot is a block of rows here and
//! nothing else changes — which is the claim [`a_family_is_rows_and_nothing_else`] exists to keep
//! honest.

use bt_term::{AttentionRequest, BellSource, NotificationSource, TerminalNotification};

use crate::attention::{
    ClearClass, ClearReason, ClearScope, ClearSelector, Credential, Event, IdSource, MappedAction,
    MappingRow, Mode, Tier, Transport, Via, WaitKind, WaitSlot, kind_mode,
};

/// Anthropic's Claude Code.
pub(crate) const CLAUDE_CODE: &str = "claude-code";
/// OpenAI's codex CLI.
pub(crate) const CODEX: &str = "codex";
/// pi, which reaches us as an announcement and never as a wait.
pub(crate) const PI: &str = "pi";

/// **The catalogue.** Every row every supported family could contribute, tier included.
///
/// A menu and not a configuration: see [`installed_rows`] for the difference, which is the whole of
/// R2 and the reason one request cannot arrive as two credentials.
pub(crate) const ROWS: &[MappingRow] = &[
    // -- Claude Code, waits (plan §10.4.1's first list) ----------------------
    //
    // Official: "Runs when Claude Code is about to ask you for permission." The document also says
    // why this tier exists at all — the Notification fallback "reaches you only after the prompt
    // has waited about six seconds".
    MappingRow {
        family: CLAUDE_CODE,
        event: "PermissionRequest",
        kind: WaitKind::Permission,
        // The `PermissionRequest` payload's request identifier and `PostToolUse`'s tool identifier
        // have not both been quoted verbatim, so this kind stays on the level path (§12.1.5's
        // verification item, whose three landings are already written down).
        id: IdSource::None,
        action: MappedAction::Wait {
            tier: Tier::Primary,
        },
    },
    // "Claude needs you to approve a tool use and the prompt has waited about six seconds."
    MappingRow {
        family: CLAUDE_CODE,
        event: "Notification.permission_prompt",
        kind: WaitKind::Permission,
        id: IdSource::None,
        action: MappedAction::Wait {
            tier: Tier::Fallback,
        },
    },
    // "Runs when an MCP server requests user input mid-task."
    MappingRow {
        family: CLAUDE_CODE,
        event: "Elicitation",
        kind: WaitKind::Elicitation,
        id: IdSource::None,
        action: MappedAction::Wait {
            tier: Tier::Primary,
        },
    },
    // "An MCP server opens an elicitation form and you haven't typed for about six seconds."
    MappingRow {
        family: CLAUDE_CODE,
        event: "Notification.elicitation_dialog",
        kind: WaitKind::Elicitation,
        id: IdSource::None,
        action: MappedAction::Wait {
            tier: Tier::Fallback,
        },
    },
    // "An MCP server asks you to open a browser URL and you haven't typed for about six seconds."
    MappingRow {
        family: CLAUDE_CODE,
        event: "Notification.elicitation_url_dialog",
        kind: WaitKind::Elicitation,
        id: IdSource::None,
        action: MappedAction::Wait {
            tier: Tier::Fallback,
        },
    },
    // "A background session starts waiting on your input." One layer only, so it is `Primary` by
    // being the only thing there rather than by beating a fallback.
    MappingRow {
        family: CLAUDE_CODE,
        event: "Notification.agent_needs_input",
        kind: WaitKind::Agent,
        id: IdSource::None,
        action: MappedAction::Wait {
            tier: Tier::Primary,
        },
    },
    // "…Claude Code waits for you to press `Enter` instead of continuing." Literally a wait for one
    // keystroke, and structurally single: a session has one quota.
    MappingRow {
        family: CLAUDE_CODE,
        event: "Notification.quota_auto_resume_stale",
        kind: WaitKind::Quota,
        id: IdSource::None,
        action: MappedAction::Wait {
            tier: Tier::Primary,
        },
    },
    // -- Claude Code, clears (plan §11.4.2's pairing table) ------------------
    //
    // "Runs when the user submits a prompt, before Claude processes it." You have replied, so
    // nothing in this pane is waiting on you — and this is the one event that re-arms the
    // announcement of a turn's end, because it *is* the start of the next one.
    MappingRow {
        family: CLAUDE_CODE,
        event: "UserPromptSubmit",
        // Unread: an `All` scope selects every kind. The column is filled because a row declares
        // four things or none, and leaving one blank invites a reader to believe it means
        // something.
        kind: WaitKind::Permission,
        id: IdSource::None,
        action: MappedAction::Clear {
            class: ClearClass::Boundary,
            scope: ClearScope::All,
            reason: ClearReason::Hook,
            begins_turn: true,
        },
    },
    // "Runs when the main Claude Code agent has finished responding." A boundary, never a wait —
    // this is the exact mapping four recordings were made to refuse.
    MappingRow {
        family: CLAUDE_CODE,
        event: "Stop",
        kind: WaitKind::Permission,
        id: IdSource::None,
        action: MappedAction::Clear {
            class: ClearClass::Boundary,
            scope: ClearScope::All,
            reason: ClearReason::Hook,
            begins_turn: false,
        },
    },
    // "Runs instead of Stop when the turn ends due to an API error."
    MappingRow {
        family: CLAUDE_CODE,
        event: "StopFailure",
        kind: WaitKind::Permission,
        id: IdSource::None,
        action: MappedAction::Clear {
            class: ClearClass::Boundary,
            scope: ClearScope::All,
            reason: ClearReason::Hook,
            begins_turn: false,
        },
    },
    MappingRow {
        family: CLAUDE_CODE,
        event: "SessionEnd",
        kind: WaitKind::Permission,
        id: IdSource::None,
        action: MappedAction::Clear {
            class: ClearClass::Boundary,
            scope: ClearScope::All,
            reason: ClearReason::SessionEnd,
            begins_turn: false,
        },
    },
    // The receipt for "you approved it, and the tool has now run". A receipt that can only name a
    // kind, so it may retire only credentials the user has already answered: a late echo of the
    // last tool call must not erase the request standing right now.
    MappingRow {
        family: CLAUDE_CODE,
        event: "PostToolUse",
        kind: WaitKind::Permission,
        id: IdSource::None,
        action: MappedAction::Clear {
            class: ClearClass::Receipt,
            scope: ClearScope::ThisKind,
            reason: ClearReason::Hook,
            begins_turn: false,
        },
    },
    // "Runs after a user responds to an MCP elicitation."
    MappingRow {
        family: CLAUDE_CODE,
        event: "ElicitationResult",
        kind: WaitKind::Elicitation,
        id: IdSource::None,
        action: MappedAction::Clear {
            class: ClearClass::Receipt,
            scope: ClearScope::ThisKind,
            reason: ClearReason::Hook,
            begins_turn: false,
        },
    },
    // "An MCP elicitation form is submitted or dismissed."
    MappingRow {
        family: CLAUDE_CODE,
        event: "Notification.elicitation_complete",
        kind: WaitKind::Elicitation,
        id: IdSource::None,
        action: MappedAction::Clear {
            class: ClearClass::Receipt,
            scope: ClearScope::ThisKind,
            reason: ClearReason::Hook,
            begins_turn: false,
        },
    },
    // "An MCP elicitation response is sent back to the server."
    MappingRow {
        family: CLAUDE_CODE,
        event: "Notification.elicitation_response",
        kind: WaitKind::Elicitation,
        id: IdSource::None,
        action: MappedAction::Clear {
            class: ClearClass::Receipt,
            scope: ClearScope::ThisKind,
            reason: ClearReason::Hook,
            begins_turn: false,
        },
    },
    // "A background session finishes or fails." It means what a boundary means — *this one is not
    // waiting on you any more* — but background sessions run several at a time, so "clear the kind"
    // and "clear that one" are **not** the same act and it cannot be a `BoundaryKind`. The cost is
    // written down in the plan: an agent that ends by itself while you never answered leaves a
    // level for the TTL to sweep. That is a stale entry, not a swallowed request.
    MappingRow {
        family: CLAUDE_CODE,
        event: "Notification.agent_completed",
        kind: WaitKind::Agent,
        id: IdSource::None,
        action: MappedAction::Clear {
            class: ClearClass::Receipt,
            scope: ClearScope::ThisKind,
            reason: ClearReason::Hook,
            begins_turn: false,
        },
    },
    // **The one pair that qualifies as a `BoundaryKind`** (§13.1.3). Both conditions hold: the
    // event *is* the exit ("the wait for this kind is over", not "the last thing finished"), and a
    // session has exactly one quota, so there is no concurrent quota wait it could retire by
    // mistake. Without this class these two would be gated by the watermark and could never fire —
    // they arrive precisely when nobody answered.
    MappingRow {
        family: CLAUDE_CODE,
        event: "Notification.quota_auto_resume_fired",
        kind: WaitKind::Quota,
        id: IdSource::None,
        action: MappedAction::Clear {
            class: ClearClass::BoundaryKind,
            scope: ClearScope::ThisKind,
            reason: ClearReason::AutoResume,
            begins_turn: false,
        },
    },
    MappingRow {
        family: CLAUDE_CODE,
        event: "Notification.quota_auto_resume_disabled",
        kind: WaitKind::Quota,
        id: IdSource::None,
        action: MappedAction::Clear {
            class: ClearClass::BoundaryKind,
            scope: ClearScope::ThisKind,
            reason: ClearReason::AutoResume,
            begins_turn: false,
        },
    },
    // -- codex: the second family, and it is rows and a template ------------
    //
    // The one event of its ten whose meaning is the same as Claude Code's `PermissionRequest`: a
    // synchronous decision gate with a `decision.behavior` in its output schema. Which is also why
    // the hook must be fire-and-forget — a signal hook that blocked would hold up **every** approval
    // this CLI ever asks for.
    //
    // **Born without an identifier and it will stay that way**: the survey's verbatim input schema
    // carries `session_id`, `turn_id`, `tool_name` and `tool_input` and nothing else. `turn_id` is
    // explicitly not usable as one — a turn can contain several approvals, so keying on it would
    // read the second real request as a restatement of the first and swallow it.
    MappingRow {
        family: CODEX,
        event: "permission-request",
        kind: WaitKind::Permission,
        id: IdSource::None,
        action: MappedAction::Wait {
            tier: Tier::Primary,
        },
    },
    MappingRow {
        family: CODEX,
        event: "user-prompt-submit",
        kind: WaitKind::Permission,
        id: IdSource::None,
        action: MappedAction::Clear {
            class: ClearClass::Boundary,
            scope: ClearScope::All,
            reason: ClearReason::Hook,
            begins_turn: true,
        },
    },
    MappingRow {
        family: CODEX,
        event: "stop",
        kind: WaitKind::Permission,
        id: IdSource::None,
        action: MappedAction::Clear {
            class: ClearClass::Boundary,
            scope: ClearScope::All,
            reason: ClearReason::Hook,
            begins_turn: false,
        },
    },
];

/// **The other lane**: events that say a turn ended.
///
/// They mint no episode, take no place in the queue and never raise `grounds` — red line 14 — so
/// they are a table of their own rather than a third variant of [`MappedAction`]. What they get is
/// the event door of §11.7: one decision per turn, re-armed by your next prompt.
///
/// `Via` is the field that keeps this table worth having. Four sources say the same sentence, and
/// telling them apart in the trace is how "did the adapter actually install?" becomes a question
/// with an answer.
pub(crate) const TURN_END: &[(&str, &str, Via)] = &[
    (CLAUDE_CODE, "Stop", Via::Stop),
    (CLAUDE_CODE, "StopFailure", Via::StopFailure),
    (CODEX, "stop", Via::Stop),
    // codex's `notify` program, whose only observed `type` is `agent-turn-complete` — and which the
    // survey found **silent** while a real approval box was on screen. It says "I have finished
    // talking", which is exactly the sentence this block refuses to promote.
    (CODEX, "agent-turn-complete", Via::Notify),
    // pi's whole contribution. "Fires only once a run fully settles."
    (PI, "agent_settled", Via::AgentSettled),
];

/// The row a `<family>:<event>` names, if this build knows one.
#[must_use]
pub(crate) fn row(
    rows: &'static [MappingRow],
    family: &str,
    event: &str,
) -> Option<&'static MappingRow> {
    rows.iter()
        .find(|row| row.family == family && row.event == event)
}

/// Whether a `<family>:<event>` is one of the turn-end announcements.
#[must_use]
pub(crate) fn turn_end(family: &str, event: &str) -> Option<Via> {
    TURN_END
        .iter()
        .find(|(row_family, row_event, _)| *row_family == family && *row_event == event)
        .map(|(_, _, via)| *via)
}

/// **R2, made a function**: the rows one machine actually has installed.
///
/// A kind that has a primary row keeps only its primary; a kind that has none keeps its fallbacks.
/// The catalogue holds both because *this build* supports both, and the machine in front of us
/// supports one — which is a question for the program itself, not for a version number. The
/// installer answers it by asking (see `attention_hooks`), and hands the answer here.
///
/// `has_primary` is the caller's answer for one `(family, kind)`. Answering `false` for a family
/// whose upstream does have the event is not a correctness failure, only a six-second one.
pub(crate) fn installed_rows(
    rows: &'static [MappingRow],
    family: &str,
    has_primary: impl Fn(WaitKind) -> bool,
) -> Vec<MappingRow> {
    rows.iter()
        .filter(|row| row.family == family)
        .filter(|row| match row.action {
            MappedAction::Wait { tier } => match tier {
                Tier::Primary => has_primary(row.kind),
                Tier::Fallback => !has_primary(row.kind),
            },
            MappedAction::Clear { .. } => true,
        })
        .copied()
        .collect()
}

/// What one arrived message does to the ledger.
///
/// The whole of the per-family logic, and there is none: a row declares its verb, its kind, its
/// class and where its identifier lives, and this reads the declaration. `id` is whatever the
/// payload gave for the row's declared path — `None` when the row declares none, which today is
/// every row.
#[must_use]
pub(crate) fn event_for(installed: &[MappingRow], row: &MappingRow, id: Option<&str>) -> Event {
    let mode = kind_mode(installed, row.family, row.kind);
    match row.action {
        MappedAction::Wait { .. } => Event::StrongWait(row.slot(mode, id)),
        MappedAction::Clear {
            class,
            scope,
            reason,
            begins_turn,
        } => Event::StrongClear {
            selector: selector(row, mode, scope, id),
            class,
            reason,
            begins_turn,
        },
    }
}

fn selector(row: &MappingRow, mode: Mode, scope: ClearScope, id: Option<&str>) -> ClearSelector {
    match scope {
        ClearScope::All => ClearSelector::All,
        ClearScope::ThisKind => match row.slot(mode, id) {
            WaitSlot::Keyed { kind, key } => ClearSelector::Key { kind, key },
            WaitSlot::Level(kind) => ClearSelector::Kind(kind),
        },
    }
}

/// The transport every row of [`ROWS`] and [`TURN_END`] arrives over.
///
/// A constant rather than a column, because it is a property of *those* tables: they are hook
/// events and a hook event reaches us one way. The other lane is [`OSC_ROWS`] below, and it is a
/// second table rather than a column here for exactly the reason this comment predicted before it
/// existed — a sequence a program writes down its own tty and a message an upstream hook posts
/// through a pipe have no field in common but the level they are worth.
pub(crate) const PIPE_TRANSPORT: Transport = Transport::Pipe;

// ---------------------------------------------------------------------------
// The other lane: what a program writes down its own tty
// ---------------------------------------------------------------------------

/// **One arrival on the OSC lane, spelled the way `bt-term` reports it.**
///
/// Deliberately the two upstream types themselves rather than a third spelling of them: this is
/// the seam where *the bytes* meet *the ledger*, and a private copy of "which sequence was it"
/// would be a copy that could fall behind the parser without anything failing to compile.
///
/// **The parser has no opinion about any of this.** `bt-term` says `Yes`, or `Osc777`, and stops
/// there; what a `Yes` is *worth* is the row below, which is data — so a family whose level is
/// argued over is a row somebody edits, not a `match` somebody adds a branch to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OscArrival {
    /// `OSC 1337;RequestAttention=`, iTerm2's sequence and its spelling.
    Request(AttentionRequest),
    /// A desktop notification: `OSC 9`, `OSC 777;notify`, or kitty's `OSC 99`.
    Notification(NotificationSource),
}

/// One row of the OSC lane: an arrival, and the level it is worth.
///
/// Two columns and not four. The pipe lane's other two — an identifier and a tier — answer
/// questions this lane cannot ask: nothing on the wire says *which* request a sequence is about
/// (§11.4's association key is an endpoint's, and red line 13 keeps it there), and there is no
/// second layer of the same event to displace. A row here declares the one thing that is genuinely
/// a decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OscRow {
    pub arrival: OscArrival,
    pub credential: Credential,
}

/// **Every arrival this lane can carry, and what each is worth** (`attention` plan §11.6).
///
/// The table is total over both upstream enums and the pin below says so: an arrival with no row
/// is a sequence the ledger has no level for, and *silently having no level* is how `OSC 9` spent
/// this plan's first three drafts — recognised by the parser, raising a toast, and absent from
/// every discussion of what counts as evidence because it had no name in the table.
///
/// **No row here is [`Credential::Strong`], and none ever can be.** "A program is blocked on your
/// input" is a sentence no OSC sequence in existence can say — iTerm2's `RequestAttention` says a
/// program *wants* you, which is a weaker thing, and promoting it would be the exact substitution
/// this whole block was opened to undo (the bell, read as "it is waiting for you"). The strong tier
/// has one producer and it is the pane's own endpoint.
pub(crate) const OSC_ROWS: &[OscRow] = &[
    // The weak tier, and the only two members it will ever have: a standing request, and the
    // program taking it back. `yes`/`no` being a *pair* is the whole reason this sequence was
    // chosen over the four alternatives — a sentence that can be unsaid.
    OscRow {
        arrival: OscArrival::Request(AttentionRequest::Yes),
        credential: Credential::Weak,
    },
    OscRow {
        arrival: OscArrival::Request(AttentionRequest::No),
        credential: Credential::Weak,
    },
    // iTerm2's own third value, and by its own definition a one-shot. It takes the bell's path
    // inside `bt-term` and never reaches the ledger; the row exists so that "it is an event" is
    // written where the other two are, rather than inferred from an absence.
    OscRow {
        arrival: OscArrival::Request(AttentionRequest::Once),
        credential: Credential::Announced,
    },
    // The three notification sequences. Each is a *message* — it has words and no "off" — which is
    // what the event level means, and it is why a survey found codex reaching us over `OSC 9` and
    // pi over `OSC 777` without either of them ever being able to say "I am waiting".
    OscRow {
        arrival: OscArrival::Notification(NotificationSource::Osc9),
        credential: Credential::Announced,
    },
    OscRow {
        arrival: OscArrival::Notification(NotificationSource::Osc777),
        credential: Credential::Announced,
    },
    OscRow {
        arrival: OscArrival::Notification(NotificationSource::Osc99),
        credential: Credential::Announced,
    },
];

/// **Which sequence said it** (`attention` plan §13.2.2).
///
/// A function of the parser's own answer rather than a column on [`OSC_ROWS`], because it is not a
/// decision: `Via` is the *name* of the sequence and `NotificationSource` is the parser's name for
/// the same sequence. What §13.2.2 buys with it is that a turn end reported over `OSC 777` stops
/// being recorded as a bare bell — and "did the adapter actually install?" becomes a question the
/// trace answers, which is the whole reason the survey wanted the field.
#[must_use]
pub(crate) fn osc_via(source: NotificationSource) -> Via {
    match source {
        NotificationSource::Osc9 => Via::Osc9,
        NotificationSource::Osc777 => Via::Osc777,
        NotificationSource::Osc99 => Via::Osc99,
    }
}

/// **How a latched bell got here** (`attention` plan §13.2.2).
///
/// One latch, two producers, and this is the row that keeps them apart in the trace. A literal
/// `0x07` is the only thing that may be recorded as `bel` — `RequestAttention=once` takes the
/// bell's *path* by an explicit ruling (§10.8.4: one sentence, one implementation), and taking a
/// path is not the same as being the thing that made it.
#[must_use]
pub(crate) fn bell_provenance(rang: BellSource) -> (Transport, Via) {
    match rang {
        BellSource::Bel => (Transport::Bel, Via::Bel),
        BellSource::AttentionOnce => (OSC_TRANSPORT, Via::Osc1337),
    }
}

/// The transport every row of [`OSC_ROWS`] arrives over — the pipe lane's [`PIPE_TRANSPORT`], for
/// its reason exactly.
///
/// **`bel` is deliberately not reachable from here.** Only a literal `0x07` is a bell (§13.2.2),
/// and a sequence recorded as one would make the one fact the survey needs — whether a family is
/// reaching us over OSC or has been downgraded to a bare bell — unanswerable.
pub(crate) const OSC_TRANSPORT: Transport = Transport::Osc;

/// What one arrival on the OSC lane is worth, or `None` for one this build has no level for.
#[must_use]
pub(crate) fn osc_credential(arrival: OscArrival) -> Option<Credential> {
    OSC_ROWS
        .iter()
        .find(|row| row.arrival == arrival)
        .map(|row| row.credential)
}

/// **The program's own words**, for the one place they may be borrowed (§11.6 rule 2).
///
/// The body, unless the whole message was the title: `OSC 777;notify;<title>` with no body is a
/// message and its words are that title. Nothing is composed here and nothing can be — a
/// notification carrying neither field is never built, because `bt-term` refuses to call one a
/// message, so this answers `None` only for the empty-bodied `OSC 9` that cannot exist.
#[must_use]
pub(crate) fn announced_words(notification: &TerminalNotification) -> Option<&str> {
    if notification.body.is_empty() {
        notification.title.as_deref()
    } else {
        Some(&notification.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::duplicated_tier;

    fn claude_installed(primary: bool) -> Vec<MappingRow> {
        installed_rows(ROWS, CLAUDE_CODE, |_| primary)
    }

    /// PIN (`attention` plan §13.2.2, gate ⑱②) — **only a literal `0x07` is recorded as a bell,
    /// and every sequence names itself.**
    ///
    /// The whole reason `src=` and `via=` were split into two fields. The survey's second ask
    /// upstream is "stop letting codex fall back to a bare bell", and the way anybody finds out
    /// whether that landed on a given machine is by reading this pair out of the trace. A build
    /// that wrote `src=bel` for a fact that arrived as `OSC 777` would make the answer say the
    /// opposite of the truth on exactly the machines where the adapter *is* working.
    ///
    /// MUTATIONS: answer `Transport::Bel` for the `once` arm and gate ⑱② goes red on the one
    /// sequence that shares the bell's *path*; answer one `Via` for all three notification
    /// sequences and four upstream families become indistinguishable in the file whose job is
    /// telling them apart.
    #[test]
    fn every_arrival_names_its_own_transport_and_only_a_bare_bell_is_a_bell() {
        assert_eq!(
            bell_provenance(BellSource::Bel),
            (Transport::Bel, Via::Bel),
            "the one thing that is a bell"
        );
        assert_eq!(
            bell_provenance(BellSource::AttentionOnce),
            (Transport::Osc, Via::Osc1337),
            "it takes the bell's path by a ruling about implementations, and that is not the \
             same as being one"
        );
        let named = [
            (NotificationSource::Osc9, Via::Osc9),
            (NotificationSource::Osc777, Via::Osc777),
            (NotificationSource::Osc99, Via::Osc99),
        ];
        for (source, via) in named {
            assert_eq!(osc_via(source), via);
            assert_eq!(
                OSC_TRANSPORT,
                Transport::Osc,
                "no sequence on this lane may be recorded as a bell"
            );
        }
        // Four values, four names: `via=` is what tells one upstream family from another when they
        // all say the same sentence.
        let mut spelled: Vec<String> = named
            .iter()
            .map(|(_, via)| via.to_string())
            .chain([Via::Osc1337.to_string(), Via::Bel.to_string()])
            .collect();
        spelled.sort_unstable();
        let count = spelled.len();
        spelled.dedup();
        assert_eq!(
            spelled.len(),
            count,
            "two arrivals spelled the same: {spelled:?}"
        );
    }

    /// **R2's red form.** Installing both layers of one kind is the defect, and the catalogue is
    /// deliberately capable of it — so the assertion that matters is that the *installed* set never
    /// is, and that the checker would have caught it if it were.
    #[test]
    fn one_kind_gets_one_tier_and_the_catalogue_alone_would_not() {
        let both = ROWS
            .iter()
            .filter(|row| row.family == CLAUDE_CODE)
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(
            duplicated_tier(&both),
            Some((CLAUDE_CODE, WaitKind::Permission)),
            "the catalogue holds both layers of `permission`, and a checker that could not see \
             that is a checker that would pass the configuration this rule exists to forbid"
        );
        for primary in [true, false] {
            assert_eq!(
                duplicated_tier(&claude_installed(primary)),
                None,
                "an installed configuration must never carry two layers of one kind"
            );
        }
    }

    /// A machine with the zero-delay events installs those and not the six-second ones.
    #[test]
    fn the_primary_layer_displaces_the_fallback_and_nothing_else() {
        let modern = claude_installed(true);
        let older = claude_installed(false);
        let names = |rows: &[MappingRow]| {
            rows.iter()
                .filter(|row| row.is_wait())
                .map(|row| row.event)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(&modern),
            [
                "PermissionRequest",
                "Elicitation",
                "Notification.agent_needs_input",
                "Notification.quota_auto_resume_stale",
            ]
        );
        assert_eq!(
            names(&older),
            [
                "Notification.permission_prompt",
                "Notification.elicitation_dialog",
                "Notification.elicitation_url_dialog",
            ],
            "a kind whose only layer is primary has no fallback to fall back to, and must simply \
             not be installed on a build that lacks it"
        );
        // The clears are the same either way: they are not tiered, and a machine that gets its
        // waits late still has to get out of them.
        let clears = |rows: &[MappingRow]| {
            rows.iter()
                .filter(|row| !row.is_wait())
                .map(|row| row.event)
                .collect::<Vec<_>>()
        };
        assert_eq!(clears(&modern), clears(&older));
    }

    /// **Every kind runs on the level path today, and the table says so out loud** (§12.1.6).
    #[test]
    fn no_kind_claims_an_identifier_it_has_not_been_shown() {
        for (family, kind) in [
            (CLAUDE_CODE, WaitKind::Permission),
            (CLAUDE_CODE, WaitKind::Elicitation),
            (CLAUDE_CODE, WaitKind::Agent),
            (CLAUDE_CODE, WaitKind::Quota),
            (CODEX, WaitKind::Permission),
        ] {
            let installed = installed_rows(ROWS, family, |_| true);
            assert_eq!(
                kind_mode(&installed, family, kind),
                Mode::Level,
                "{family}/{kind} claims an identifier; a path may only be written from a verbatim \
                 quotation of the upstream payload"
            );
        }
    }

    /// **The turn's end is never a wait**, in either table, for any family.
    #[test]
    fn nothing_that_ends_a_turn_is_mapped_to_a_wait() {
        for (family, event, _) in TURN_END {
            let mapped = row(ROWS, family, event);
            assert!(
                mapped.is_none_or(|row| !row.is_wait()),
                "{family}:{event} ends a turn and must not also raise a wait"
            );
        }
        // And the two spellings a survey would be tempted by are absent entirely.
        for tempting in [
            "Notification.idle_prompt",
            "Notification.auth_success",
            "SubagentStop",
            "PreToolUse",
        ] {
            assert!(
                row(ROWS, CLAUDE_CODE, tempting).is_none(),
                "{tempting} is on the closed list of things that never map to a wait"
            );
        }
    }

    /// The quota pair is the only `BoundaryKind`, and it must be one.
    #[test]
    fn only_a_wait_that_the_program_itself_ends_gets_past_the_watermark() {
        let boundary_kinds = ROWS
            .iter()
            .filter(|row| {
                matches!(
                    row.action,
                    MappedAction::Clear {
                        class: ClearClass::BoundaryKind,
                        ..
                    }
                )
            })
            .map(|row| row.event)
            .collect::<Vec<_>>();
        assert_eq!(
            boundary_kinds,
            [
                "Notification.quota_auto_resume_fired",
                "Notification.quota_auto_resume_disabled",
            ],
            "a source qualifies only if it *is* the exit and the kind cannot run two at once; \
             `agent_completed` is the near miss and it is a `Receipt`"
        );
    }

    /// A clear that can only name a kind selects a kind; one that can name a key selects the key.
    #[test]
    fn a_receipt_aims_as_narrowly_as_its_evidence_allows() {
        let installed = claude_installed(true);
        let post_tool_use = row(ROWS, CLAUDE_CODE, "PostToolUse").expect("row");
        assert_eq!(
            event_for(&installed, post_tool_use, Some("toolu_01")),
            Event::StrongClear {
                selector: ClearSelector::Kind(WaitKind::Permission),
                class: ClearClass::Receipt,
                reason: ClearReason::Hook,
                begins_turn: false,
            },
            "an identifier offered by a payload must not be used by a kind that has not declared \
             one: half a kind on keys is the mixture that produces both failure directions at once"
        );
        // The same row with a declared path aims at the key — the upgrade §12.1.5 writes down,
        // exercised here so that taking the quotation is a data change and not a code change.
        let keyed = MappingRow {
            id: IdSource::Path("tool_use_id"),
            ..*post_tool_use
        };
        let keyed_wait = MappingRow {
            id: IdSource::Path("tool_use_id"),
            ..*row(ROWS, CLAUDE_CODE, "PermissionRequest").expect("row")
        };
        let upgraded = vec![keyed, keyed_wait];
        assert_eq!(
            event_for(&upgraded, &keyed, Some("toolu_01")),
            Event::StrongClear {
                selector: ClearSelector::Key {
                    kind: WaitKind::Permission,
                    key: "toolu_01".to_owned(),
                },
                class: ClearClass::Receipt,
                reason: ClearReason::Hook,
                begins_turn: false,
            }
        );
    }

    /// Only a reply re-arms the announcement of a turn's end. `Stop` is the turn's end, not the
    /// start of the next one.
    #[test]
    fn the_only_event_that_begins_a_turn_is_the_one_where_you_speak() {
        let begins = ROWS
            .iter()
            .filter(|row| {
                matches!(
                    row.action,
                    MappedAction::Clear {
                        begins_turn: true,
                        ..
                    }
                )
            })
            .map(|row| (row.family, row.event))
            .collect::<Vec<_>>();
        assert_eq!(
            begins,
            [
                (CLAUDE_CODE, "UserPromptSubmit"),
                (CODEX, "user-prompt-submit"),
            ]
        );
    }

    /// **A family is rows and nothing else.**
    ///
    /// codex is the claim's first witness: it arrives through the same verb, the same endpoint and
    /// the same ledger as Claude Code, and the only thing this crate learned in order to support it
    /// is the block of rows above. The assertion is on the *shape* of that support — a wait, a
    /// boundary that ends a turn, a boundary that begins one — which is what a new family would
    /// have to be expressible as.
    #[test]
    fn a_family_is_rows_and_nothing_else() {
        let installed = installed_rows(ROWS, CODEX, |_| true);
        assert_eq!(installed.len(), 3, "codex is three rows");
        let request = row(ROWS, CODEX, "permission-request").expect("row");
        assert_eq!(
            event_for(&installed, request, None),
            Event::StrongWait(WaitSlot::Level(WaitKind::Permission)),
            "codex is born without an identifier and rides the watermark"
        );
        assert_eq!(turn_end(CODEX, "stop"), Some(Via::Stop));
        assert_eq!(turn_end(CODEX, "agent-turn-complete"), Some(Via::Notify));
        assert_eq!(
            row(ROWS, CODEX, "agent-turn-complete"),
            None,
            "the announcement lane and the queue lane do not share rows"
        );
        // pi is the other end of the same claim: a family with no wait rows at all is a legal
        // answer, not an unfinished one.
        assert_eq!(installed_rows(ROWS, PI, |_| true), Vec::new());
        assert_eq!(turn_end(PI, "agent_settled"), Some(Via::AgentSettled));
    }

    // -- the OSC lane (§11.6) ------------------------------------------------

    /// Every value `bt-term` can report for `OSC 1337;RequestAttention=`.
    ///
    /// The `match` is the point of the function: adding a fourth value upstream stops this
    /// compiling, which is the only way "the table is total" stays true without anybody rereading
    /// it. A list alone would go quietly out of date, and the failure it hides — a sequence with no
    /// level — is exactly how `OSC 9` spent three drafts outside the vocabulary.
    fn every_request() -> [AttentionRequest; 3] {
        let all = [
            AttentionRequest::Yes,
            AttentionRequest::No,
            AttentionRequest::Once,
        ];
        for value in all {
            match value {
                AttentionRequest::Yes | AttentionRequest::No | AttentionRequest::Once => {}
            }
        }
        all
    }

    /// Every sequence a [`TerminalNotification`] can arrive over. See [`every_request`].
    fn every_source() -> [NotificationSource; 3] {
        let all = [
            NotificationSource::Osc9,
            NotificationSource::Osc777,
            NotificationSource::Osc99,
        ];
        for value in all {
            match value {
                NotificationSource::Osc9
                | NotificationSource::Osc777
                | NotificationSource::Osc99 => {}
            }
        }
        all
    }

    fn every_arrival() -> Vec<OscArrival> {
        every_request()
            .into_iter()
            .map(OscArrival::Request)
            .chain(every_source().into_iter().map(OscArrival::Notification))
            .collect()
    }

    /// **The OSC table is total, and no arrival is named twice.**
    #[test]
    fn every_arrival_the_wire_can_carry_names_exactly_one_row() {
        for arrival in every_arrival() {
            let named = OSC_ROWS.iter().filter(|row| row.arrival == arrival).count();
            assert_eq!(
                named, 1,
                "{arrival:?} is named {named} times; a sequence with no level is one that gets \
                 argued about from scratch every time it comes up, and one with two is a lookup \
                 whose answer depends on typing order"
            );
        }
        assert_eq!(OSC_ROWS.len(), every_arrival().len(), "and nothing else");
    }

    /// **No sequence a program writes down its own tty can say "I am blocked on you"** (red line
    /// 14's other half, and §11.6's three-level table).
    ///
    /// iTerm2's `RequestAttention=yes` says a program *wants* you. Reading that as *waiting for
    /// you* is the same substitution this whole block was opened to undo — it is what the bell was
    /// read as — and the fact that this one arrives over a documented sequence rather than as a
    /// `0x07` makes the substitution more tempting, not less wrong. The strong level has one
    /// producer and it is the pane's own endpoint.
    #[test]
    fn nothing_on_the_wire_is_the_strong_level() {
        let weak = OSC_ROWS
            .iter()
            .filter(|row| row.credential == Credential::Weak)
            .map(|row| row.arrival)
            .collect::<Vec<_>>();
        assert_eq!(
            weak,
            [
                OscArrival::Request(AttentionRequest::Yes),
                OscArrival::Request(AttentionRequest::No),
            ],
            "the weak level is the pair that can be unsaid, and only that pair"
        );
        for arrival in every_arrival() {
            assert_ne!(
                osc_credential(arrival),
                Some(Credential::Strong),
                "{arrival:?}"
            );
        }
    }

    /// **A notification is worth its words and nothing else** — the bytes, end to end.
    ///
    /// Fed the way a child writes them, read back the way the ledger will ask. Both sequences and
    /// both of `OSC 777`'s shapes, because the one that carries only a title is the case where the
    /// message is not in the field called `body`, and answering `None` there would drop the whole
    /// of what a program said.
    #[test]
    fn the_two_old_world_sequences_arrive_as_words_at_the_event_level() {
        for (bytes, source, words) in [
            (
                &b"\x1b]9;the build finished\x07"[..],
                NotificationSource::Osc9,
                "the build finished",
            ),
            (
                &b"\x1b]777;notify;Build;finished in 4s\x07"[..],
                NotificationSource::Osc777,
                "finished in 4s",
            ),
            (
                &b"\x1b]777;notify;the build finished\x07"[..],
                NotificationSource::Osc777,
                "the build finished",
            ),
        ] {
            let mut session = bt_term::DualPlaneSession::new(
                std::num::NonZeroU32::new(80).expect("a width"),
                std::num::NonZeroU32::new(8).expect("a height"),
            );
            session.feed(bytes).expect("the session accepts bytes");
            let arrived = session.take_notifications();
            let [notification] = arrived.as_slice() else {
                panic!("one message, not {}: {bytes:?}", arrived.len());
            };
            assert_eq!(notification.source, source, "{bytes:?}");
            assert_eq!(
                osc_credential(OscArrival::Notification(notification.source)),
                Some(Credential::Announced),
                "{bytes:?}"
            );
            assert_eq!(announced_words(notification), Some(words), "{bytes:?}");
        }
    }

    /// PIN (`attention` plan §11.6 rule 3) — **`OSC 9;4` is the progress ring, not an
    /// announcement.**
    ///
    /// ConEmu hung a numbered subcommand slot on `OSC 9` and iTerm2 hung a free-text notification
    /// on the same number, so one sequence carries two protocols and this build reads both. The
    /// failure this pin forbids is the cheap one: reading `9;4;3` as a message would put the text
    /// `4;3` in front of a person as something a program said, on every keep-alive tick of every
    /// progress bar.
    #[test]
    fn the_progress_arm_of_osc_nine_is_not_a_message() {
        let mut session = bt_term::DualPlaneSession::new(
            std::num::NonZeroU32::new(80).expect("a width"),
            std::num::NonZeroU32::new(8).expect("a height"),
        );
        session
            .feed(b"\x1b]9;4;3\x07")
            .expect("the session accepts bytes");
        assert_eq!(
            session.take_notifications(),
            Vec::new(),
            "the ring is not a sentence"
        );
        assert_eq!(
            session.status().progress,
            Some(bt_term::ProgressState::Indeterminate),
            "and it is still the ring"
        );
    }

    /// No `<family>:<event>` names two rows — a lookup that could answer twice is a lookup whose
    /// answer depends on the order somebody typed the table in.
    #[test]
    fn every_event_names_at_most_one_row() {
        for (index, row) in ROWS.iter().enumerate() {
            assert!(
                !ROWS[..index]
                    .iter()
                    .any(|earlier| earlier.family == row.family && earlier.event == row.event),
                "{}:{} appears twice",
                row.family,
                row.event
            );
        }
    }
}
