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
//! **No clock.** A turn's end followed at once by a turn the agent started itself lights the dot
//! for the moment between them, which is true; whether to hold it back, and on what evidence, is
//! a later row's question (the owner's ruling R1, 2026-11-06). Every arrival writes a `hear` line
//! to `BT_ATTENTION_TRACE`, which is what that row will be decided from.

use bt_term::{CommandMark, CommandMarkId};

use crate::agent_resume::{self, Child};
use crate::attention_map::TurnStep;

/// **One pane's account of its agent's turns** — on [`crate::ShellSession`], with the leaf's
/// lifetime.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct TurnAccount {
    /// What the agent is in this pane, once a turn arrival has been heard from it; `None` while
    /// the pane is read by the shell's rule.
    bound: Option<Binding>,
    /// The output revision the pane had reached when its latest turn ended; `None` before any
    /// turn has ended.
    ended_at: Option<u64>,
    /// A turn has begun, or reported in, since the latest end.
    running: bool,
}

/// What an account belongs to, and therefore what ends it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Binding {
    /// The pane's own child is the agent; the account ends with the leaf.
    Child,
    /// The agent is this marked command of the pane's shell; the account ends with it.
    Command(CommandMarkId),
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
    /// **A pane born as its agent's resume**, bound from birth: its record was written by that
    /// agent's own `Stop`, so its hooks are known to speak, and the bytes a resume prints while
    /// it reloads its conversation are not a turn that ended.
    #[must_use]
    pub(crate) fn of_a_resumed_agent() -> Self {
        Self {
            bound: Some(Binding::Child),
            ..Self::default()
        }
    }

    /// **A turn arrival from this pane's hook.** `child` is what the pane's program makes it
    /// ([`agent_resume::AgentAccount::child`]); `output_revision` is how much the pane has said.
    ///
    /// An unbound account binds here, or stays unbound: a hook from a shell that is running no
    /// marked command has no command to belong to, and the pane keeps the shell's rule.
    pub(crate) fn hear_turn_step(
        &mut self,
        step: TurnStep,
        child: Child<'_>,
        output_revision: u64,
    ) {
        if self.bound.is_none() {
            self.bound = match child {
                Child::Agent(_) => Some(Binding::Child),
                Child::Shell(marks) => agent_resume::running_command(marks).map(Binding::Command),
            };
        }
        if self.bound.is_none() {
            return;
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
        if let Some(Binding::Command(command)) = self.bound
            && !agent_resume::command_goes_on(command, marks)
        {
            *self = Self::default();
        }
    }

    /// What the claim reads, or `None` while the pane is read by the shell's rule.
    #[must_use]
    pub(crate) fn agent_turn(&self) -> Option<AgentTurn> {
        self.bound.map(|_| AgentTurn {
            ended_at: self.ended_at,
            running: self.running,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_resume::VENDORS;

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
        account.hear_turn_step(TurnStep::Ended, Child::Agent(&VENDORS[0]), 7);
        assert_eq!(
            account.agent_turn(),
            Some(AgentTurn {
                ended_at: Some(7),
                running: false
            })
        );
        account.hear_turn_step(TurnStep::Within, Child::Agent(&VENDORS[0]), 9);
        assert_eq!(
            account.agent_turn(),
            Some(AgentTurn {
                ended_at: Some(7),
                running: true
            })
        );
        account.hear_turn_step(TurnStep::Ended, Child::Agent(&VENDORS[0]), 12);
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
        account.hear_turn_step(TurnStep::Ended, shell(session.command_marks()), 1);
        assert_eq!(account.agent_turn(), None, "no marked command, no binding");
        session
            .feed(b"\x1b]133;A\x07PS> \x1b]133;B\x07claude\r\n\x1b]133;C\x07")
            .expect("the marks parse");
        account.hear_turn_step(TurnStep::Began, shell(session.command_marks()), 2);
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

    /// **A resume is bound from birth, before its agent has said anything** (row 75): what it
    /// prints while it reloads its conversation is not a turn that ended.
    #[test]
    fn a_resume_is_bound_from_birth_with_nothing_finished() {
        assert_eq!(
            TurnAccount::of_a_resumed_agent().agent_turn(),
            Some(AgentTurn {
                ended_at: None,
                running: false
            })
        );
    }
}
