//! **An agent pane's turns, as its blue dot reads them** (0.4.10 plan row 75,
//! T-UNREAD-DOT-ON-EVERY-BACKGROUND-TAB).
//!
//! The tab's blue dot says one thing: *something here finished while you were not looking, and
//! nothing has started since* (DESIGN §7.1.5b, 未读·完成). For a shell, "finished" is the shell's
//! own command boundary: a command ran (`OSC 133;C`), ended (`D`), and what it printed has not
//! been painted. **For an agent that boundary does not exist.** Claude Code writes no `OSC 133`
//! (`docs/plans/attention/evidence-2026-08-25.md` §3.2), so a pane whose program *is* the agent —
//! every pane row 195 resumes, every agent profile's tab — is never "working", and every byte it
//! draws (a spinner frame, a status line, a repaint) read as "finished, unread". That is the
//! attention plan's defect D4, second branch, and since 0.4.9 it was the owner's every background
//! tab. An agent typed into an integrated shell had the opposite answer for the same reason — D4's
//! first branch: the shell's `C` stood for the agent's whole life, so the pane never wore a dot.
//!
//! So an agent pane's "finished" is read from the agent's own turns, which its hooks report
//! ([`crate::attention_map::TurnStep`]):
//!
//! * **a turn's end** is the finish, at the output revision the pane had reached when it was heard
//!   — so a turn whose last words were painted before you left owes you nothing, and one that went
//!   on writing after you left does;
//! * **a turn beginning, or the agent's work reporting in from inside one**, is work in flight,
//!   which outranks a finished claim (D35) until that turn ends in its turn;
//! * **looking** is painting, exactly as for a shell: the pane's seen revision is the one both
//!   rules are measured against ([`crate::SessionFacts`]).
//!
//! # Which panes are read this way, and for how long
//!
//! An account is **bound** at the first turn arrival heard from the pane, to what the agent is
//! there: the pane's own child, when that is a vendor's CLI ([`agent_resume::Child::Agent`]), or
//! the marked command the shell is running, when the agent was typed into a shell. A pane born as
//! a resume is bound from birth — its record exists only because that agent's `Stop` was heard
//! before the restart. A command's account ends with the command, at the shell's next mark
//! ([`TurnAccount::settle_turn_command`]); a child's ends with the child, because the pane that
//! outlives it is a new [`crate::ShellSession`]. Unbound, the pane is read by the shell's rule,
//! bytes and all: a fresh agent tab before its first prompt (the owner's ruling R4, 2026-11-06),
//! and a shell that marks nothing, which has no command for an account to belong to.
//!
//! **One agent, one family** (review N1), the rule the pane's other account already keeps
//! ([`agent_resume::AgentAccount::hear`]). An agent runs other agents — Claude Code's Bash tool
//! running `codex exec` — and the nested run's hooks name the same pane, because they inherit its
//! capability. So a pane whose child is a vendor's CLI hears only that vendor's family, and a typed
//! command's account hears only the family it was bound by, the first heard from inside it; a
//! nested run of another vendor ends nothing. A turn end its row says another client ran (codex's
//! `notify` from `codex exec`) is no turn of the pane's either: the wire drops it before it is a
//! step ([`crate::attention_wire::Message::another_client`]). A nested run of the **same** vendor
//! through a hook that names no client (`claude -p` inside Claude Code, `codex exec` inside codex
//! through codex's `Stop` hook) is not told apart — the limit the agent account has too.
//!
//! **Arrival order is connection order** (review N2). Every hook is its own process, run
//! asynchronously, and the endpoint hears them in the order they connect, not the order the agent
//! fired them; no payload field orders them. A step overtaken by its own turn's end — a
//! `PostToolUse` whose process connects after the `Stop` — leaves the turn running, and that end
//! lights nothing until the next one. The `hear` lines show whether it happens.
//!
//! **No clock.** A turn's end followed at once by a turn the agent started itself lights the dot
//! for the moment between them, which is true; whether to hold it back, and on what evidence, is
//! a later row's question (the owner's ruling R1, 2026-11-06). Every arrival writes a `hear` line
//! to `BT_ATTENTION_TRACE`, which is what that row will be decided from.

use bt_term::{CommandMark, CommandMarkId};

use crate::agent_resume::{self, Child};
use crate::attention_map::TurnStep;

/// **One pane's account of its agent's turns** — on [`crate::ShellSession`], with the leaf's
/// lifetime.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TurnAccount {
    /// What the agent is in this pane, once a turn arrival has been heard from it; `None` while
    /// the pane is read by the shell's rule.
    bound: Option<TurnBinding>,
    /// The output revision the pane had reached when its latest turn ended; `None` before any
    /// turn has ended.
    ended_at: Option<u64>,
    /// A turn has begun, or reported in, since the latest end.
    running: bool,
}

/// What an account belongs to, and therefore what ends it and which family it hears.
#[derive(Clone, Debug, Eq, PartialEq)]
enum TurnBinding {
    /// The pane's own child is the agent; the account ends with the leaf, and hears the child's
    /// own vendor's family ([`Child::Agent`]).
    Child,
    /// The agent is this marked command of the pane's shell; the account ends with it, and hears
    /// the family it was bound by.
    Command {
        command: CommandMarkId,
        family: String,
    },
}

/// **What the claim reads of a bound account** — [`crate::SessionFacts::turn`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AgentTurn {
    /// The output revision at the latest turn's end, if a turn has ended. Unseen while the pane's
    /// seen revision is below it.
    pub(crate) ended_at: Option<u64>,
    /// A turn is under way.
    pub(crate) running: bool,
}

impl TurnAccount {
    /// **The account a pane is born with, beside the agent account it was born with.** A pane
    /// born as its agent's resume is bound from birth: its record was written by that agent's own
    /// `Stop`, so its hooks are known to speak, and the bytes a resume prints while it reloads its
    /// conversation are not a turn that ended. Every other pane is born unbound.
    #[must_use]
    pub(crate) fn born_beside(agent: &agent_resume::AgentAccount) -> Self {
        Self {
            bound: agent.is_resumed().then_some(TurnBinding::Child),
            ..Self::default()
        }
    }

    /// **A turn arrival from this pane's hook**, spoken as `family`. `child` is what the pane's
    /// program makes it ([`agent_resume::AgentAccount::child`]); `output_revision` is how much the
    /// pane has said.
    ///
    /// An unbound account binds here, or stays unbound: a hook from a shell that is running no
    /// marked command has no command to belong to, and the pane keeps the shell's rule. An arrival
    /// of a family the pane's agent is not — a nested run of another vendor — is no step of its
    /// (review N1), bound or not.
    pub(crate) fn hear_turn_step(
        &mut self,
        step: TurnStep,
        family: &str,
        child: Child<'_>,
        output_revision: u64,
    ) {
        match child {
            Child::Agent(own) => {
                if own.family != family {
                    return;
                }
                self.bound.get_or_insert(TurnBinding::Child);
            }
            Child::Shell(marks) => match &self.bound {
                Some(TurnBinding::Command { family: first, .. }) => {
                    if first != family {
                        return;
                    }
                }
                _ => match agent_resume::running_command(marks) {
                    Some(command) => {
                        self.bound = Some(TurnBinding::Command {
                            command,
                            family: family.to_owned(),
                        });
                    }
                    None => return,
                },
            },
        }
        match step {
            TurnStep::Ended => {
                self.ended_at = Some(output_revision);
                self.running = false;
            }
            TurnStep::Began | TurnStep::Within => self.running = true,
        }
    }

    /// **A typed agent's account ends with its command**: once the shell has marked that command
    /// finished, or marked a newer one, the pane is the shell's again. A mark retired under a
    /// running command is not its end ([`agent_resume::command_goes_on`]). Asked by the reaper on
    /// every turn, beside [`agent_resume::AgentAccount::settle_command`].
    pub(crate) fn settle_turn_command(&mut self, marks: &[CommandMark]) {
        if let Some(TurnBinding::Command { command, .. }) = &self.bound
            && !agent_resume::command_goes_on(*command, marks)
        {
            *self = Self::default();
        }
    }

    /// What the claim reads, or `None` while the pane is read by the shell's rule.
    #[must_use]
    pub(crate) fn agent_turn(&self) -> Option<AgentTurn> {
        self.bound.as_ref().map(|_| AgentTurn {
            ended_at: self.ended_at,
            running: self.running,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_resume::{AgentAccount, Resume, SeedAgent, VENDORS};
    use crate::attention_map::{CLAUDE_CODE, CODEX};

    fn shell(marks: &[CommandMark]) -> Child<'_> {
        Child::Shell(marks)
    }

    /// RED (row 75) — **an account binds to an agent child at its first turn arrival, and a turn
    /// that began after an end is work in flight until it ends in its turn.** The end is held at
    /// the revision it was heard at.
    ///
    /// MUTATION: `TurnStep::Began | TurnStep::Within => {}` (the running flag never set) and the
    /// second assertion goes red.
    #[test]
    fn a_turn_after_an_end_runs_until_it_ends() {
        let mut account = TurnAccount::default();
        assert_eq!(account.agent_turn(), None, "unbound: the shell's rule");
        account.hear_turn_step(
            TurnStep::Ended,
            VENDORS[0].family,
            Child::Agent(&VENDORS[0]),
            7,
        );
        assert_eq!(
            account.agent_turn(),
            Some(AgentTurn {
                ended_at: Some(7),
                running: false
            })
        );
        account.hear_turn_step(
            TurnStep::Within,
            VENDORS[0].family,
            Child::Agent(&VENDORS[0]),
            9,
        );
        assert_eq!(
            account.agent_turn(),
            Some(AgentTurn {
                ended_at: Some(7),
                running: true
            })
        );
        account.hear_turn_step(
            TurnStep::Ended,
            VENDORS[0].family,
            Child::Agent(&VENDORS[0]),
            12,
        );
        assert_eq!(
            account.agent_turn(),
            Some(AgentTurn {
                ended_at: Some(12),
                running: false
            })
        );
    }

    /// RED (row 75) — **a hook from a shell that runs no marked command binds nothing**, so the
    /// pane keeps the shell's rule; one heard while a marked command runs belongs to that command
    /// and ends with it — not with a mark retired under it.
    ///
    /// MUTATION: drop the `!command_goes_on` condition in `settle_turn_command` and the account
    /// ends while its command is still running: the second assertion goes red.
    #[test]
    fn a_typed_agents_account_belongs_to_its_command() {
        let one = |n| std::num::NonZeroU32::new(n).expect("a size");
        let mut session = bt_term::DualPlaneSession::new(one(40), one(4));
        let mut account = TurnAccount::default();
        account.hear_turn_step(
            TurnStep::Ended,
            CLAUDE_CODE,
            shell(session.command_marks()),
            1,
        );
        assert_eq!(account.agent_turn(), None, "no marked command, no binding");
        session
            .feed(b"\x1b]133;A\x07PS> \x1b]133;B\x07claude\r\n\x1b]133;C\x07")
            .expect("the marks parse");
        account.hear_turn_step(
            TurnStep::Began,
            CLAUDE_CODE,
            shell(session.command_marks()),
            2,
        );
        account.settle_turn_command(session.command_marks());
        assert!(account.agent_turn().is_some(), "bound while it runs");
        session
            .feed(b"\x1b]133;D;0\x07\x1b]133;A\x07PS> ")
            .expect("the marks parse");
        account.settle_turn_command(session.command_marks());
        assert_eq!(
            account.agent_turn(),
            None,
            "the shell's next prompt ends it"
        );
    }

    /// RED (row 75, review N1) — **a nested run of another vendor is no step of the pane's
    /// agent**: Claude Code's Bash tool running `codex exec` speaks from the same pane, and its
    /// turn's end neither binds a fresh pane nor ends the turn under way — for a pane whose child
    /// is the agent and for a typed command, which hears the family it was bound by.
    ///
    /// MUTATION: drop the `own.family != family` refusal in `hear_turn_step` and the first
    /// assertion goes red.
    #[test]
    fn a_nested_agent_of_another_family_is_no_step_of_the_panes() {
        let claude = VENDORS
            .iter()
            .find(|vendor| vendor.family == CLAUDE_CODE)
            .expect("claude is a vendor");
        let mut account = TurnAccount::default();
        account.hear_turn_step(TurnStep::Ended, CODEX, Child::Agent(claude), 3);
        assert_eq!(account.agent_turn(), None, "another vendor binds nothing");
        account.hear_turn_step(TurnStep::Began, CLAUDE_CODE, Child::Agent(claude), 4);
        account.hear_turn_step(TurnStep::Ended, CODEX, Child::Agent(claude), 9);
        assert_eq!(
            account.agent_turn(),
            Some(AgentTurn {
                ended_at: None,
                running: true
            }),
            "the nested run's end is not the pane's"
        );

        let one = |n| std::num::NonZeroU32::new(n).expect("a size");
        let mut session = bt_term::DualPlaneSession::new(one(40), one(4));
        session
            .feed(b"\x1b]133;A\x07PS> \x1b]133;B\x07claude\r\n\x1b]133;C\x07")
            .expect("the marks parse");
        let mut typed = TurnAccount::default();
        typed.hear_turn_step(
            TurnStep::Began,
            CLAUDE_CODE,
            shell(session.command_marks()),
            1,
        );
        typed.hear_turn_step(TurnStep::Ended, CODEX, shell(session.command_marks()), 5);
        assert_eq!(
            typed.agent_turn(),
            Some(AgentTurn {
                ended_at: None,
                running: true
            }),
            "a typed command hears the family it was bound by"
        );
        typed.hear_turn_step(
            TurnStep::Ended,
            CLAUDE_CODE,
            shell(session.command_marks()),
            6,
        );
        assert_eq!(
            typed.agent_turn(),
            Some(AgentTurn {
                ended_at: Some(6),
                running: false
            })
        );
    }

    /// RED (row 75) — **a resume is bound from birth, before its agent has said anything**: what
    /// it prints while it reloads its conversation is not a turn that ended. Any other birth is
    /// unbound, and read by the shell's rule until its agent's first turn arrival (ruling R4).
    ///
    /// MUTATION: `bound: None` in `born_beside` and the first assertion goes red.
    #[test]
    fn a_resume_is_bound_from_birth_with_nothing_finished() {
        let seed = SeedAgent::Resume(Resume {
            record: bt_persist::AgentRecordV1 {
                vendor: crate::attention_map::CLAUDE_CODE.to_owned(),
                resume_id: "0b9c7e1a-4f3d-4c2b-9a8e-5d6f7a8b9c0d".to_owned(),
                cwd: r"C:\work\resume".to_owned(),
                home: None,
            },
            hand_back: None,
        });
        assert_eq!(
            TurnAccount::born_beside(&AgentAccount::born(&seed, true)).agent_turn(),
            Some(AgentTurn {
                ended_at: None,
                running: false
            })
        );
        assert_eq!(
            TurnAccount::born_beside(&AgentAccount::born(&seed, false)).agent_turn(),
            None,
            "a birth that is not a resume"
        );
        assert_eq!(
            TurnAccount::born_beside(&AgentAccount::default()).agent_turn(),
            None
        );
    }
}
