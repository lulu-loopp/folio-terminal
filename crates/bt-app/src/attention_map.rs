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

use crate::attention::{
    ClearClass, ClearReason, ClearScope, ClearSelector, Event, IdSource, MappedAction, MappingRow,
    Mode, Tier, Transport, Via, WaitKind, WaitSlot, kind_mode,
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

/// The transport every row in this file arrives over.
///
/// A constant rather than a column, because it is a property of *this* table: these are hook
/// events and a hook event reaches us one way. The weak tier's rows, if there were a table of them,
/// would be `Osc` — and the day one exists it will be a different table, not a column here.
pub(crate) const TRANSPORT: Transport = Transport::Pipe;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::duplicated_tier;

    fn claude_installed(primary: bool) -> Vec<MappingRow> {
        installed_rows(ROWS, CLAUDE_CODE, |_| primary)
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
