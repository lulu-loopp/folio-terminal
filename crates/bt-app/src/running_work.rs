//! **What is still running: the one reader for running work** (0.4.10 plan row 87, part 87e;
//! T-TRAY-87E-QUIT-ASKS; the owner's ruling R8 of 2026-11-14 and the coordinator's items 15
//! and 19 of the same day).
//!
//! A quit asks once, at its Ask step, while an agent works or a command runs
//! ([`crate::quit::Quit::begin_asking`]), and the application's power hold while it is put away
//! on macOS (part 87c) reads the same count. One reader, so that the card and everything else
//! that says "Folio is still working" can never disagree about it.
//!
//! **Read per pane, from the two facts the strip's own claim reads** (`ShellSession::session_facts`,
//! [`crate::SessionFacts`]): the shell's command state and the agent's turns.
//!
//! * **A pane read by its agent's turns** — its account is bound, because a hook of its agent
//!   has been heard ([`crate::agent_turns::TurnAccount::agent_turn`]) — **is an agent working
//!   while a turn is under way**, and nothing otherwise. Its shell's command state is not read:
//!   an agent typed into an integrated shell stands on the shell's `C` for its whole life, idle
//!   at its prompt included, so that reading would call every waiting agent a running command.
//! * **Every other pane is a command running while its session's own `working` flag is up**:
//!   the shell's `OSC 133` `C` seen on the primary screen and its `D` not yet
//!   ([`bt_term::SessionStatus::working`]). The flag stays up while a full-screen program holds
//!   the grid — `vim`, `less`, an `ssh` into a TUI end on quit as surely as a build does — which is
//!   why this is not the tab's breath (`TabState::fleet_working` drops the alternate screen,
//!   rightly for a breath and wrongly here). And it is not the last command mark
//!   ([`crate::agent_resume::running_command`]): a mark can be retired while its command runs
//!   (Clear scrollback, `ED 3`, the scrollback quota, a reflow), and the session's flag survives
//!   that.
//!
//! **A pane without shell integration is not running work** (item 15): no `C` ever arrives
//! there, so nothing says a command runs, and unknown is not running. The same holds for an agent
//! whose hooks have not been heard (a profile's agent before its first prompt, a vendor without
//! hooks): it is not counted until its account is bound.

use std::ops::Add;

use crate::agent_turns::AgentTurn;

/// **How many panes have an agent working, and how many a command running.** One pane is at most
/// one of the two.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RunningWork {
    /// Panes whose agent has a turn under way.
    pub(crate) agents: usize,
    /// Panes whose shell says a command is running, agents' panes not among them.
    pub(crate) commands: usize,
}

impl RunningWork {
    /// Nothing running.
    pub(crate) const NONE: Self = Self {
        agents: 0,
        commands: 0,
    };

    /// Whether anything is running.
    #[must_use]
    pub(crate) const fn any(self) -> bool {
        self.agents > 0 || self.commands > 0
    }
}

impl Add for RunningWork {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            agents: self.agents + other.agents,
            commands: self.commands + other.commands,
        }
    }
}

/// **What one pane is running**: `working` is its session's command flag
/// ([`bt_term::SessionStatus::working`]) and `turn` its agent's turns, `Some` once its account is
/// bound ([`crate::agent_turns::TurnAccount::agent_turn`]). See the module's doc for the rule.
#[must_use]
pub(crate) fn pane_work(working: bool, turn: Option<AgentTurn>) -> RunningWork {
    match turn {
        Some(turn) => RunningWork {
            agents: usize::from(turn.running),
            commands: 0,
        },
        None => RunningWork {
            agents: 0,
            commands: usize::from(working),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_resume::{AgentAccount, Child, VENDORS};
    use crate::agent_turns::TurnAccount;
    use crate::attention_map::{CLAUDE_CODE, TurnStep};

    /// A session fed `bytes`, as the drain feeds a pane.
    fn session_fed(bytes: &[u8]) -> bt_term::DualPlaneSession {
        let one = |n| std::num::NonZeroU32::new(n).expect("a size");
        let mut session = bt_term::DualPlaneSession::new(one(80), one(24));
        session.feed(bytes).expect("the bytes parse");
        session
    }

    const A_COMMAND_STARTS: &[u8] =
        b"\x1b]133;A\x07PS> \x1b]133;B\x07cargo build\r\n\x1b]133;C\x07";

    /// RED (T-TRAY-87E-QUIT-ASKS, R2) — **a pane is read by its agent's turns once its account is
    /// bound, and by its shell's command flag otherwise**: a shell between `C` and `D` is a command
    /// running, on the alternate screen too; a shell with no integration is nothing; a bound
    /// agent mid-turn is an agent working; the same agent after its turn ended is nothing, though
    /// its typed command's `C` is still up; an agent child whose hooks were never heard is
    /// nothing.
    ///
    /// PLANT 87e-P32: in `pane_work`, read `working` in the `Some(turn)` arm too (a bound agent's
    /// pane counted as a command while its shell's `C` stands) — the idle agent's case goes red.
    #[test]
    fn a_pane_is_read_by_its_agents_turns_or_else_by_its_shells_command() {
        let command = RunningWork {
            agents: 0,
            commands: 1,
        };
        let agent = RunningWork {
            agents: 1,
            commands: 0,
        };

        let running = session_fed(A_COMMAND_STARTS);
        assert_eq!(
            pane_work(running.status().working, None),
            command,
            "a shell between C and D runs a command"
        );

        let full_screen = session_fed(&[A_COMMAND_STARTS, b"\x1b[?1049h"].concat());
        assert!(full_screen.status().alternate_screen);
        assert_eq!(
            pane_work(full_screen.status().working, None),
            command,
            "and so it does while the command holds the screen"
        );

        let ended = session_fed(&[A_COMMAND_STARTS, b"\x1b]133;D;0\x07"].concat());
        assert_eq!(
            pane_work(ended.status().working, None),
            RunningWork::NONE,
            "D ends it"
        );

        let no_integration = session_fed(b"PS> cargo build\r\n   Compiling bt-app\r\n");
        assert_eq!(
            pane_work(no_integration.status().working, None),
            RunningWork::NONE,
            "a pane without shell integration is not running work"
        );

        // An agent typed into the integrated shell: its command's C stands for its whole life.
        let mut typed = TurnAccount::default();
        typed.hear_turn_step(
            TurnStep::Began,
            CLAUDE_CODE,
            Child::Shell(running.command_marks()),
            1,
        );
        assert_eq!(
            pane_work(running.status().working, typed.agent_turn()),
            agent,
            "a bound agent mid-turn is an agent working"
        );
        typed.hear_turn_step(
            TurnStep::Ended,
            CLAUDE_CODE,
            Child::Shell(running.command_marks()),
            2,
        );
        assert!(running.status().working, "its command's C is still up");
        assert_eq!(
            pane_work(running.status().working, typed.agent_turn()),
            RunningWork::NONE,
            "and the same agent waiting at its prompt is nothing"
        );

        // A pane whose program is the agent, no hook heard yet: unbound, no integration.
        let claude = VENDORS
            .iter()
            .find(|vendor| vendor.family == CLAUDE_CODE)
            .expect("Claude Code is a vendor");
        let unheard = TurnAccount::born_beside(&AgentAccount::default());
        assert_eq!(unheard.agent_turn(), None);
        let mut heard = unheard.clone();
        heard.hear_turn_step(TurnStep::Began, CLAUDE_CODE, Child::Agent(claude), 1);
        assert_eq!(
            pane_work(false, unheard.agent_turn()),
            RunningWork::NONE,
            "an agent whose hooks were never heard is not counted"
        );
        assert_eq!(
            pane_work(false, heard.agent_turn()),
            agent,
            "and is, once its turn is heard"
        );
    }

    /// RED — **the counts add pane by pane**, and `any` is either of them.
    #[test]
    fn running_work_adds_pane_by_pane() {
        let total = pane_work(true, None) + pane_work(true, None) + pane_work(false, None);
        assert_eq!(
            total,
            RunningWork {
                agents: 0,
                commands: 2
            }
        );
        assert!(total.any());
        assert!(!RunningWork::NONE.any());
        assert!(
            RunningWork {
                agents: 1,
                commands: 0
            }
            .any()
        );
    }
}
