//! **The crate root: agents resumed after a restart** (0.4.9 plan row 195,
//! T-AGENT-RESUME-AFTER-RESTART). Tests of the road a recorded session takes — heard from a hook,
//! written with the layout, read back at a restore, composed into the vendor's resume, and ended
//! by the agent's own exit — through the crate root's own functions (`use super::*`), with their
//! shared fixtures from [`crate::test_support`].

use super::*;
use crate::agent_resume::{
    AgentAccount, CouldNotResume, Ended, Heard, Restore, SeedAgent, VENDORS,
};
use crate::test_support::{BOTH_NOTIFICATION_ROWS_ON, on_a_screen, ringing_tab, saved_tab};
use bt_persist::AgentRecordV1;
use bt_platform::HostPlatform;
use std::ffi::OsString;

/// A made-up session id of the vendors' shape. No vendor has ever filed a session under it.
const SESSION: &str = "0b9c7e1a-4f3d-4c2b-9a8e-5d6f7a8b9c0d";

/// The folder the made-up session is filed under. Only a name: nothing here asks the disk.
const FOLDER: &str = r"C:\work\resume";

/// A Claude Code record of [`SESSION`] in [`FOLDER`].
fn claude_record() -> AgentRecordV1 {
    AgentRecordV1 {
        vendor: attention_map::CLAUDE_CODE.to_owned(),
        resume_id: SESSION.to_owned(),
        cwd: FOLDER.to_owned(),
        home: None,
    }
}

/// A saved tab of one pane of `profile`, standing in `cwd`, with `last_command` and `agent`.
fn saved_agent_tab(profile: &str, last_command: &str, agent: Option<AgentRecordV1>) -> TabV1 {
    let mut tab = saved_tab(profile, r"C:\work\elsewhere", None, true);
    if let LayoutNodeV1::Leaf(LeafNodeV1::Term(leaf)) = &mut tab.root {
        leaf.last_command = last_command.to_owned();
        leaf.agent = agent;
    }
    tab
}

/// The one terminal seed a revived one-pane tab is made of.
fn revived_seed(tab: &TabV1, restore: Restore) -> LeafSeed {
    let (seats, _, leaves, _, _) = revive_plan_with(tab, restore);
    leaves
        .get(&seats.identity())
        .cloned()
        .expect("the saved pane comes back as a terminal")
}

/// This build's switch on, on Windows: what a launch of 0.4.9 asks for by default there.
const RESUMING: Restore = Restore {
    switch: true,
    platform: HostPlatform::Windows,
};

/// RED (row 195) — **a saved pane whose agent named its session is born through that vendor's
/// resume**: as the vendor's program, in the folder the session is filed under, with the resume's
/// arguments and none of the profile's, owing nothing to a prompt — and holding the record from
/// its first moment, so a second restart before its first turn ends still has it. A shell pane
/// where the agent was typed is born the same way and remembers the shell it hands back.
///
/// The road is the product's from the saved leaf to the command line the birth worker is handed:
/// `revive_plan_with` → `birth_here` → `shell_integration::shell_command` → `compose_resume` →
/// `AgentAccount::born`; only the spawn itself is not made.
///
/// MUTATION: `vendor.arguments(&record.resume_id)` in `compose_resume` → `Vec::new()` (the id
/// dropped), and the pane is born as the bare program: the arguments assertion goes red.
#[cfg(windows)]
#[test]
fn a_saved_agent_pane_with_a_recorded_session_is_born_through_its_vendors_resume() {
    let every_row =
        profiles::ProfilePrograms::with_only(&(0..profiles::count()).collect::<Vec<_>>());
    for (saved_as, hand_back) in [("claude", None), ("pwsh", Some("pwsh"))] {
        let tab = saved_agent_tab(saved_as, "claude", Some(claude_record()));
        let seed = revived_seed(&tab, RESUMING);
        assert_eq!(
            seed.profile, "claude",
            "{saved_as}: born as the vendor's program"
        );
        assert_eq!(
            seed.cwd.as_ref().map(profiles::SeedPlace::path),
            Some(Path::new(FOLDER)),
            "{saved_as}: in the folder the session is filed under"
        );
        assert_eq!(
            seed.prefill, None,
            "{saved_as}: nothing is typed at a resume"
        );
        let (born, started) = birth_here(&seed, "", &every_row).expect("every row is answered");
        assert_eq!(started, Started::AsAsked);
        let row = profiles::row_of(&born.profile).expect("the claude row is shipped");
        let mut command = shell_integration::shell_command(
            &row,
            &profiles::ShellProgram::Native,
            &[],
            shell_integration::Scripts::installed(),
            &bt_pty::SystemShellEnvironment,
        );
        let resumed = compose_resume(&mut command, &born, &started, &row);
        let expected = vec![OsString::from("--resume"), OsString::from(SESSION)];
        assert_eq!(
            command.arguments, expected,
            "{saved_as}: the vendor's resume, whole"
        );
        assert!(resumed, "{saved_as}: and the birth knows it is a resume");
        let account = AgentAccount::born(&born.agent, true);
        assert_eq!(account.record(), Some(&claude_record()), "{saved_as}");
        assert!(account.is_resumed());
        assert_eq!(
            account.saved_profile("claude"),
            hand_back.unwrap_or("claude"),
            "{saved_as}: the layout goes on writing the pane as what it was"
        );
    }
    // The configuration folder the session was recorded under is the one it is started with.
    let tab = saved_agent_tab(
        "claude",
        "",
        Some(AgentRecordV1 {
            home: Some(r"C:\work\second-config".to_owned()),
            ..claude_record()
        }),
    );
    let seed = revived_seed(&tab, RESUMING);
    let (born, started) = birth_here(&seed, "", &every_row).expect("every row is answered");
    let row = profiles::row_of(&born.profile).expect("the claude row is shipped");
    let mut command = shell_integration::shell_command(
        &row,
        &profiles::ShellProgram::Native,
        &[],
        shell_integration::Scripts::installed(),
        &bt_pty::SystemShellEnvironment,
    );
    compose_resume(&mut command, &born, &started, &row);
    assert!(command.environment.contains(&(
        OsString::from("CLAUDE_CONFIG_DIR"),
        OsString::from(r"C:\work\second-config")
    )));
}

/// RED (row 195) — **the switch off, a pane comes back as its shell**, exactly as before this
/// row: its own profile, its own folder, and its last typed line owed to its prompt, on Windows
/// and on a Mac alike (T-AGENT-RESUME-ON-MACOS). And on a platform this build does not resume on
/// (another Unix, which has no build), the same with the switch on.
///
/// MUTATION: drop `self.switch &&` from `Restore::resumes` and the switched-off pane is born as
/// the vendor's program.
#[test]
fn with_the_switch_off_a_recorded_pane_comes_back_as_its_shell() {
    for restore in [
        Restore {
            switch: false,
            platform: HostPlatform::Windows,
        },
        Restore {
            switch: false,
            platform: HostPlatform::MacOs,
        },
        Restore {
            switch: true,
            platform: HostPlatform::OtherUnix,
        },
        Restore::off(),
    ] {
        let tab = saved_agent_tab(
            profiles::fallback_profile_id(),
            "git status",
            Some(claude_record()),
        );
        let seed = revived_seed(&tab, restore);
        assert_eq!(seed.profile, profiles::fallback_profile_id(), "{restore:?}");
        assert_eq!(seed.agent, SeedAgent::None, "{restore:?}");
        assert_eq!(
            seed.cwd.as_ref().map(profiles::SeedPlace::path),
            Some(Path::new(r"C:\work\elsewhere")),
            "{restore:?}: the pane's own folder"
        );
        assert_eq!(seed.prefill.as_deref(), Some("git status"), "{restore:?}");
        assert_eq!(AgentAccount::born(&seed.agent, true).record(), None);
    }
    // And a record whose id is not a session is no resume, switch or no switch.
    let tab = saved_agent_tab(
        profiles::fallback_profile_id(),
        "",
        Some(AgentRecordV1 {
            resume_id: "--continue".to_owned(),
            ..claude_record()
        }),
    );
    assert_eq!(revived_seed(&tab, RESUMING).agent, SeedAgent::None);
}

/// RED (row 195) — **a resumed agent that exits before any hook has spoken from it did not come
/// back**: the pane is handed the shell it was, in the session's folder, and that shell's first
/// line says which session did not come back. The record is cleared, so the next restart does not
/// try again. No clock: Claude Code's refusal of an unknown id prints a sentence and exits 1
/// (measured 2026-11-02), which is this road's whole trigger.
///
/// MUTATION: in `AgentAccount::child_ended`, answer the unheard arm with `failed: None` and the
/// line is not said: the banner assertion goes red.
#[test]
fn a_resumed_agent_that_exits_before_a_hook_hands_back_its_shell_with_one_line() {
    let seed = SeedAgent::Resume(crate::agent_resume::Resume {
        record: claude_record(),
        hand_back: Some(profiles::fallback_profile_id().to_owned()),
    });
    let mut account = AgentAccount::born(&seed, true);
    let Ended::HandBack {
        profile,
        cwd,
        failed,
    } = account.child_ended()
    else {
        panic!("a resume that never ran hands back its shell");
    };
    assert_eq!(profile.as_deref(), Some(profiles::fallback_profile_id()));
    assert_eq!(cwd, PathBuf::from(FOLDER));
    assert_eq!(account.record(), None, "and the record is gone with it");
    let failed = failed.expect("a resume no hook spoke from did not come back");
    // The hand-back's shell, born the way `Runtime::hand_back_shell` asks for it.
    let shell_seed = LeafSeed {
        agent: SeedAgent::CouldNotResume(failed),
        ..restart_seed(
            profiles::fallback_profile_id(),
            Some(profiles::SeedPlace::Carried(cwd)),
        )
    };
    let every_row =
        profiles::ProfilePrograms::with_only(&(0..profiles::count()).collect::<Vec<_>>());
    let wake = PtyWakeSignal::new(crate::headless::LoopEvents::default().proxy());
    let pane = create_leaf_session(
        LeafView {
            text_scale: TextScale::ACTUAL,
            metrics: fixture_cell_metrics(1.0, 16.0),
        },
        bt_render::SeatViewport::whole(800, 600),
        LeafId {
            tab: TabId(1),
            seat: SeatId(1),
        },
        &wake,
        Some(b"PS> ".as_slice()),
        &shell_seed,
        &every_row,
        "",
        FormulaSwitches::from_settings(&bt_persist::SettingsV1::default()),
        NonZeroUsize::new(1_000).unwrap(),
        true,
        &bt_pty::SystemShellEnvironment,
    )
    .expect("a pane with no shell behind it is born");
    let first = pane.session.session.terminal().visible_text()[0].clone();
    assert!(
        first.contains("[Folio]") && first.contains("Claude Code") && first.contains("0b9c7e1a"),
        "the shell's first line names the session that did not come back: {first:?}"
    );
    assert_eq!(
        pane.session.agent.record(),
        None,
        "and the shell records nothing"
    );
    // A resume a hook spoke from ran: its end hands the shell back with nothing to say.
    let mut account = AgentAccount::born(&seed, true);
    account.hear(
        &VENDORS[0],
        &Heard {
            id: SESSION.to_owned(),
            cwd: FOLDER.to_owned(),
            home: None,
        },
        crate::agent_resume::Child::Agent(&VENDORS[0]),
    );
    assert!(matches!(
        account.child_ended(),
        Ended::HandBack { failed: None, .. }
    ));
    // And the line, as the hand-back's seed says it.
    assert!(
        CouldNotResume {
            title: "Codex",
            id: SESSION.to_owned(),
        }
        .text()
        .contains("Codex session 0b9c7e1a")
    );
}

/// RED (row 195) — **a record ends with the agent's own exit, and `SessionEnd` ends nothing**: a
/// pane whose program is the agent records the session its `Stop` names; Claude Code's
/// `SessionEnd`, which also fires while the system is ending the agent, leaves it standing; the
/// child's own end clears it. A shell pane that marks nothing records nothing at all.
///
/// MUTATION: in `AgentAccount::child_ended`, read the record with `self.record.clone()` instead of
/// taking it, and the record outlives the agent's exit: the last assertion goes red.
#[test]
fn a_record_is_cleared_by_the_agents_own_exit_and_not_by_session_end() {
    let mut tabs = vec![ringing_tab(1, 2)];
    let seats = tabs[0].seats.terminals();
    let (agent, shell) = (seats[0], seats[1]);
    let mut capabilities = Vec::new();
    for (seat, program) in [(agent, "claude.exe"), (shell, "pwsh.exe")] {
        let capability = attention_wire::mint_capability();
        let leaf = &mut tabs[0]
            .sessions
            .get_mut(&seat)
            .expect("the fixture's seat holds a shell")
            .session;
        leaf.attention.capability.clone_from(&capability);
        leaf.launch.program = Some(PathBuf::from(program));
        capabilities.push(capability);
    }
    let installed =
        attention_map::installed_rows(attention_map::ROWS, attention_map::CLAUDE_CODE, |_| true);
    let said = |capability: &str, event: &str, resume: Option<Heard>| attention_wire::Message {
        capability: capability.to_owned(),
        family: attention_map::CLAUDE_CODE.to_owned(),
        event: event.to_owned(),
        id: None,
        text: None,
        resume,
    };
    let heard = Heard {
        id: SESSION.to_owned(),
        cwd: FOLDER.to_owned(),
        home: None,
    };
    let mut places = attention::Places::default();
    let now = Instant::now();
    let mut deliver = |tabs: &mut [TabState], message: attention_wire::Message| {
        deliver_attention(
            tabs,
            1,
            on_a_screen(false),
            BOTH_NOTIFICATION_ROWS_ON,
            &mut places,
            &[message],
            &installed,
            now,
            None,
            &mut Vec::new(),
        )
    };
    let record =
        |tabs: &[TabState], seat: SeatId| tabs[0].sessions[&seat].session.agent.record().cloned();
    assert!(
        deliver(
            &mut tabs,
            said(&capabilities[0], "Stop", Some(heard.clone()))
        ),
        "a session named for the first time is owed to the layout"
    );
    assert_eq!(record(&tabs, agent), Some(claude_record()));
    assert!(
        !deliver(
            &mut tabs,
            said(&capabilities[0], "Stop", Some(heard.clone()))
        ),
        "the same session again changes nothing"
    );
    assert!(!deliver(
        &mut tabs,
        said(&capabilities[1], "Stop", Some(heard))
    ));
    assert_eq!(
        record(&tabs, shell),
        None,
        "a shell that marked no running command has no end to clear a typed agent's record at"
    );
    deliver(&mut tabs, said(&capabilities[0], "SessionEnd", None));
    assert_eq!(
        record(&tabs, agent),
        Some(claude_record()),
        "SessionEnd leaves the record standing"
    );
    let leaf = &mut tabs[0]
        .sessions
        .get_mut(&agent)
        .expect("the agent's pane")
        .session;
    assert_eq!(leaf.agent.child_ended(), Ended::Cleared);
    assert_eq!(leaf.agent.record(), None, "the agent's own exit ends it");
}

/// RED (row 195, review F2) — **a typed agent's record belongs to its command**: heard while the
/// shell's marked command runs, kept while that command goes on even when its mark is retired
/// under it (an `ED 3` from the agent's own redraw, Clear scrollback, eviction), heard again
/// there, and ended by the shell's next prompt — `settle_command`, which the reaper asks every
/// turn.
///
/// MUTATION: `settle_command` clearing whenever the shell is not running the recorded mark (the
/// mark retired reads as the command's end): the record is gone after the `ED 3`.
#[test]
fn a_typed_agents_record_ends_with_the_command_it_was_typed_as() {
    let mut tabs = [ringing_tab(1, 1)];
    let seat = tabs[0].seats.terminals()[0];
    let leaf = &mut tabs[0]
        .sessions
        .get_mut(&seat)
        .expect("the fixture's seat holds a shell")
        .session;
    // A prompt, a typed command, and its execution — the marks a shell with integration writes.
    leaf.session
        .feed(b"\x1b]133;A\x07PS> \x1b]133;B\x07claude\r\n\x1b]133;C\x07")
        .expect("the marks parse");
    let heard = Heard {
        id: SESSION.to_owned(),
        cwd: FOLDER.to_owned(),
        home: None,
    };
    let child =
        crate::agent_resume::child_of(Some(Path::new("pwsh.exe")), leaf.session.command_marks());
    assert!(leaf.agent.hear(&VENDORS[0], &heard, child));
    assert!(
        !leaf.agent.settle_command(leaf.session.command_marks()),
        "still running"
    );
    assert_eq!(leaf.agent.record(), Some(&claude_record()));
    // The agent writes a screenful and more, then clears the scrollback (`ED 3`): the command's
    // mark, now in the scrollback, is retired while the command runs.
    leaf.session
        .feed("agent output\r\n".repeat(200).as_bytes())
        .expect("the output parses");
    leaf.session.feed(b"\x1b[3J").expect("the ED 3 parses");
    assert!(
        leaf.session.command_marks().is_empty(),
        "the ED 3 retired the running command's mark"
    );
    assert!(
        !leaf.agent.settle_command(leaf.session.command_marks()),
        "a retired mark with no newer one is no command's end"
    );
    assert_eq!(leaf.agent.record(), Some(&claude_record()));
    let child =
        crate::agent_resume::child_of(Some(Path::new("pwsh.exe")), leaf.session.command_marks());
    let next = Heard {
        id: "1c2d3e4f-5a6b-4c7d-8e9f-0a1b2c3d4e5f".to_owned(),
        ..heard.clone()
    };
    assert!(
        leaf.agent.hear(&VENDORS[0], &next, child),
        "and the same command is heard on"
    );
    leaf.session
        .feed(b"\x1b]133;D;0\x07\x1b]133;A\x07PS> ")
        .expect("the marks parse");
    assert!(leaf.agent.settle_command(leaf.session.command_marks()));
    assert_eq!(leaf.agent.record(), None, "the shell's next prompt ends it");
}

/// **The record is written with the layout and read back as it was** (row 195): `term_leaf`
/// writes the pane's record, and a pane whose agent was resumed in it is written as the shell it
/// hands back.
#[test]
fn the_layout_writes_a_panes_record_and_the_shell_it_was() {
    let mut tabs = [ringing_tab(1, 1)];
    let seat = tabs[0].seats.terminals()[0];
    let seed = SeedAgent::Resume(crate::agent_resume::Resume {
        record: claude_record(),
        hand_back: Some("gitbash".to_owned()),
    });
    let leaf = &mut tabs[0]
        .sessions
        .get_mut(&seat)
        .expect("the fixture's seat holds a shell")
        .session;
    leaf.agent = AgentAccount::born(&seed, true);
    leaf.launch.profile = "claude".to_owned();
    let saved = tabs[0].term_leaf(seat, false);
    assert_eq!(saved.agent, Some(claude_record()));
    assert_eq!(saved.profile_id, "gitbash");
}

/// RED (row 195, review F1, F7) — **one run, one vendor, one folder**: a shell's command is bound
/// to the first vendor heard inside it, so the `codex` an agent typed into PowerShell starts is
/// refused while the command runs; the same vendor's new id (a `/clear`) is taken; and every turn
/// after the run's first keeps the folder the session was filed under, wherever the agent has
/// `cd`'d since. The shell's next command is a new run.
///
/// MUTATION: drop the refusal of another vendor in `AgentAccount::hear` and the pane records the
/// `codex exec` that the typed `claude` started — the first assertion after it goes red.
#[test]
fn a_typed_agents_record_is_its_first_vendors_and_keeps_its_first_folder() {
    let mut tabs = [ringing_tab(1, 1)];
    let seat = tabs[0].seats.terminals()[0];
    let leaf = &mut tabs[0]
        .sessions
        .get_mut(&seat)
        .expect("the fixture's seat holds a shell")
        .session;
    leaf.session
        .feed(b"\x1b]133;A\x07PS> \x1b]133;B\x07claude\r\n\x1b]133;C\x07")
        .expect("the marks parse");
    fn shell(marks: &[bt_term::CommandMark]) -> crate::agent_resume::Child<'_> {
        crate::agent_resume::child_of(Some(Path::new("pwsh.exe")), marks)
    }
    let heard = Heard {
        id: SESSION.to_owned(),
        cwd: FOLDER.to_owned(),
        home: None,
    };
    let (claude, codex) = (&VENDORS[0], &VENDORS[1]);
    assert!(
        leaf.agent
            .hear(claude, &heard, shell(leaf.session.command_marks()))
    );
    let exec = Heard {
        id: "2d3e4f5a-6b7c-4d8e-9f0a-1b2c3d4e5f6a".to_owned(),
        cwd: r"C:\work\resume\scratch".to_owned(),
        home: None,
    };
    assert!(
        !leaf
            .agent
            .hear(codex, &exec, shell(leaf.session.command_marks())),
        "another vendor's turn inside the command is refused"
    );
    assert_eq!(leaf.agent.record(), Some(&claude_record()));
    let cleared = Heard {
        id: "3e4f5a6b-7c8d-4e9f-8a1b-2c3d4e5f6a7b".to_owned(),
        cwd: r"C:\work\resume\crates\bt-app".to_owned(),
        home: Some(r"C:\work\elsewhere-config".to_owned()),
    };
    assert!(
        leaf.agent
            .hear(claude, &cleared, shell(leaf.session.command_marks())),
        "the same vendor's next session is taken"
    );
    assert_eq!(
        leaf.agent.record(),
        Some(&AgentRecordV1 {
            resume_id: cleared.id.clone(),
            ..claude_record()
        }),
        "with the run's first folder and configuration folder"
    );
    // The command ends; the next one is a run of its own, and may be another vendor's.
    leaf.session
        .feed(b"\x1b]133;D;0\x07\x1b]133;A\x07PS> \x1b]133;B\x07codex\r\n\x1b]133;C\x07")
        .expect("the marks parse");
    assert!(leaf.agent.settle_command(leaf.session.command_marks()));
    assert!(
        leaf.agent
            .hear(codex, &exec, shell(leaf.session.command_marks()))
    );
    assert_eq!(
        leaf.agent.record().map(|record| record.vendor.as_str()),
        Some(attention_map::CODEX)
    );
    // And a resumed pane keeps the folder its session was filed under at every turn.
    let mut account = AgentAccount::born(
        &SeedAgent::Resume(crate::agent_resume::Resume {
            record: claude_record(),
            hand_back: None,
        }),
        true,
    );
    assert!(account.hear(claude, &cleared, crate::agent_resume::Child::Agent(claude)));
    assert_eq!(
        account.record().map(|record| record.cwd.as_str()),
        Some(FOLDER)
    );
}

/// RED (row 195, review F3) — **a resume whose folder is gone is started nowhere**: the birth's
/// alternative for a gone folder starts nothing for a resume (a shell's starts where no folder
/// would have put it), the landing says so in `diagnostics.log` and leaves the pane where it was
/// put, and the account then owes the pane's shell, once, with the could-not-resume line and its
/// record cleared — the hand-back road every failed resume takes.
///
/// MUTATION: `birth_unless_gone` composing the alternative for a resume too — the resume would
/// then run in the profile's own folder — and the first assertion goes red.
#[test]
fn a_resume_whose_folder_is_gone_is_started_nowhere_and_hands_back_its_shell() {
    let gone = profiles::GoneFolder {
        folder: PathBuf::from(FOLDER),
        place: profiles::spawn_place(
            profiles::index_of_id(profiles::fallback_profile_id()),
            None,
            &bt_pty::SystemShellEnvironment,
        )
        .place,
    };
    let composed = || vec![OsString::from("-NoLogo")];
    assert_eq!(
        birth_unless_gone(Some(&gone), true, |_| composed()).map(|spec| spec.arguments),
        Some(None),
        "a resume starts nothing in the gone folder's stead"
    );
    assert_eq!(
        birth_unless_gone(Some(&gone), false, |_| composed()).map(|spec| spec.arguments),
        Some(Some(composed())),
        "a shell starts where no folder would have put it"
    );
    let mut notes = Vec::new();
    let decision = BirthDecision {
        started: Started::AsAsked,
        spawn_profile: "claude".to_owned(),
        spawn_place: Some(PathBuf::from(FOLDER)),
        at_shell_home: false,
        named: false,
        program: None,
        program_kind: profiles::ShellProgram::Native,
        unless_gone: Some(Box::new(gone)),
    }
    .with_resume_not_started(&mut |line| notes.push(line.to_owned()));
    assert_eq!(decision.spawn_place, Some(PathBuf::from(FOLDER)));
    assert!(
        notes.len() == 1 && notes[0].contains(FOLDER) && notes[0].contains("not resumed"),
        "{notes:?}"
    );
    let mut account = AgentAccount::born(
        &SeedAgent::Resume(crate::agent_resume::Resume {
            record: claude_record(),
            hand_back: Some("gitbash".to_owned()),
        }),
        true,
    );
    assert_eq!(
        account.take_not_started(),
        None,
        "a resume that started owes nothing"
    );
    account.not_started();
    let Some(Ended::HandBack {
        profile,
        cwd,
        failed,
    }) = account.take_not_started()
    else {
        panic!("a resume that was not started hands back its shell");
    };
    assert_eq!(profile.as_deref(), Some("gitbash"));
    assert_eq!(cwd, PathBuf::from(FOLDER));
    assert!(
        failed.is_some_and(|failed| failed.text().contains("Claude Code session 0b9c7e1a")),
        "with the line"
    );
    assert_eq!(account.record(), None, "and the record cleared");
    assert_eq!(account.take_not_started(), None, "once");
}

/// **A resume this machine cannot start comes back as the shell the pane hands back** (row 195,
/// review N2): a Git Bash pane whose typed `claude` was recorded, restored where no row but Git
/// Bash's is startable, is born as its Git Bash with the could-not-resume line — not as the
/// default profile.
#[cfg(windows)]
#[test]
fn a_resume_whose_program_is_missing_comes_back_as_the_shell_it_hands_back() {
    let only_gitbash = profiles::ProfilePrograms::with_only(&[profiles::index_of_id("gitbash")]);
    let seed = LeafSeed {
        agent: SeedAgent::Resume(crate::agent_resume::Resume {
            record: claude_record(),
            hand_back: Some("gitbash".to_owned()),
        }),
        ..restart_seed(
            "claude",
            Some(profiles::SeedPlace::Carried(PathBuf::from(FOLDER))),
        )
    };
    let (born, started) = birth_here(&seed, "", &only_gitbash).expect("every row is answered");
    assert_eq!(born.profile, "gitbash");
    assert_eq!(started, Started::AsAsked);
    assert!(
        could_not_resume_line(&born, &started, false)
            .is_some_and(|line| line.contains("Claude Code session 0b9c7e1a")),
        "and says which session did not come back"
    );
}

/// RED (row 195, review F5) — **a resumed shell pane's own profile is the shell it hands back**:
/// a Git Bash pane whose typed `claude` was resumed runs the `claude` profile, and its own profile
/// is the Git Bash the layout writes it as — the one accessor *Restart shell*, *Duplicate tab* and
/// a split read (`runtime::tabs::life_tests::a_resumed_shell_panes_restart_duplicate_and_split_start_its_shell`
/// drives the three).
///
/// MUTATION: `ShellSession::own_profile` answering the launch's profile, and the first assertion
/// goes red.
#[test]
fn a_resumed_shell_panes_own_profile_is_the_shell_it_hands_back() {
    let mut tabs = [ringing_tab(1, 1)];
    let seat = tabs[0].seats.terminals()[0];
    let leaf = &mut tabs[0]
        .sessions
        .get_mut(&seat)
        .expect("the fixture's seat holds a shell")
        .session;
    leaf.launch.profile = "claude".to_owned();
    leaf.agent = AgentAccount::born(
        &SeedAgent::Resume(crate::agent_resume::Resume {
            record: claude_record(),
            hand_back: Some("gitbash".to_owned()),
        }),
        true,
    );
    assert_eq!(
        leaf.own_profile(),
        "gitbash",
        "the pane's own profile is the shell it hands back"
    );
    leaf.agent = AgentAccount::default();
    assert_eq!(
        leaf.own_profile(),
        "claude",
        "and a pane with no resumed agent's is its launch's"
    );
}

/// A Codex record of [`SESSION`] in [`FOLDER`].
fn codex_record() -> AgentRecordV1 {
    AgentRecordV1 {
        vendor: attention_map::CODEX.to_owned(),
        ..claude_record()
    }
}

/// The shell environment a Mac whose account shell is `/bin/zsh` answers with: what
/// `profiles::shipped_for(SeedPlatform::MacOs, …)` reads to build that table on any machine.
struct ZshAccount;

impl bt_pty::ShellEnvironment for ZshAccount {
    fn var_os(&self, key: &str) -> Option<OsString> {
        (key == "SHELL").then(|| OsString::from("/bin/zsh"))
    }

    fn is_file(&self, _path: &Path) -> bool {
        true
    }
}

/// The macOS table's row `id`, built as a value on whichever machine runs the test (H1 rule 1).
fn mac_row(id: &str) -> profiles::Profile {
    profiles::shipped_for(profiles::SeedPlatform::MacOs, &ZshAccount)
        .into_iter()
        .find(|row| row.id == id)
        .unwrap_or_else(|| panic!("the macOS table ships {id}"))
}

/// A seed that resumes `record` in a pane of `profile`, handing that profile back.
fn resume_seed(profile: &str, record: AgentRecordV1) -> LeafSeed {
    LeafSeed {
        profile: profile.to_owned(),
        cwd: Some(profiles::SeedPlace::Carried(PathBuf::from(FOLDER))),
        agent: SeedAgent::Resume(crate::agent_resume::Resume {
            record,
            hand_back: Some(profile.to_owned()),
        }),
        ..LeafSeed::default()
    }
}

/// The scripts a shell's door names, as names only: nothing here asks the disk.
fn named_scripts() -> shell_integration::Scripts<'static> {
    shell_integration::Scripts {
        bash: Some(Path::new("/folio/folio.bash")),
        zdotdir: Some(Path::new("/folio/zdotdir")),
    }
}

/// RED (T-AGENT-RESUME-ON-MACOS) — **the road a resume takes is the platform's, and the vendor is
/// the record's**: on Windows a shell pane is born as the vendor's own profile, on a Mac as that
/// same shell (which `exec`s the vendor), and a pane whose own profile is the vendor's keeps it on
/// both. Asked of the platform as a value, so either machine checks the other (CONVENTIONS H1).
/// Another Unix has no build and resumes nothing.
///
/// MUTATION (plant `plant-15-mac-not-resumed.py`): `Capability::AgentResume` on Windows only, and
/// the Mac's shell pane comes back with no resume.
#[test]
fn the_resume_road_is_the_platforms() {
    use crate::agent_resume::{Resume, Road, revived};
    assert_eq!(Road::on(HostPlatform::Windows), Road::VendorProgram);
    assert_eq!(Road::on(HostPlatform::MacOs), Road::ThroughTheShell);
    for (platform, shell_pane_born_as) in [
        (HostPlatform::Windows, "claude"),
        (HostPlatform::MacOs, "zsh"),
    ] {
        let restore = Restore {
            switch: true,
            platform,
        };
        let (profile, folder, agent) = revived(
            "zsh".to_owned(),
            Some(PathBuf::from("/work/elsewhere")),
            Some(&claude_record()),
            restore,
        );
        assert_eq!(profile, shell_pane_born_as, "{platform:?}: a shell pane");
        assert_eq!(
            folder.as_deref(),
            Some(Path::new(FOLDER)),
            "{platform:?}: in the folder the session is filed under"
        );
        assert_eq!(
            agent,
            SeedAgent::Resume(Resume {
                record: claude_record(),
                hand_back: Some("zsh".to_owned()),
            }),
            "{platform:?}: resumed, handing the shell back"
        );
        let (profile, _, agent) =
            revived("claude".to_owned(), None, Some(&claude_record()), restore);
        assert_eq!(profile, "claude", "{platform:?}: the vendor's own pane");
        assert_eq!(
            agent,
            SeedAgent::Resume(Resume {
                record: claude_record(),
                hand_back: None,
            }),
            "{platform:?}: which hands nothing back"
        );
    }
    let (profile, _, agent) = revived(
        "zsh".to_owned(),
        None,
        Some(&claude_record()),
        Restore {
            switch: true,
            platform: HostPlatform::OtherUnix,
        },
    );
    assert_eq!((profile.as_str(), agent), ("zsh", SeedAgent::None));
}

/// RED (T-AGENT-RESUME-ON-MACOS) — **a shell pane's resume is its own shell, interactive,
/// `exec`ing the vendor's resume**: the shell's words stay as its door composed them (zsh's and
/// sh's `-l`, bash's `--init-file … -i`), `-i` is added once, and `-c` carries the one command,
/// `exec claude --resume <id>` / `exec codex resume <id>`. The configuration folder the session
/// was recorded under is set. The rows are the macOS table's, built as values, so this runs on
/// both machines.
///
/// MUTATIONS (plants `plant-15-shell-argv-replaced.py`, `plant-15-no-exec.py`,
/// `plant-15-login-only.py`): the shell's words replaced by the vendor's, the command without
/// `exec`, or the interactive flag dropped, and the argv assertion goes red.
#[test]
fn a_shell_panes_resume_keeps_its_shells_words_and_execs_the_vendor() {
    let exec =
        |vendor: &str, words: &str| OsString::from(format!("exec {vendor} {words} {SESSION}"));
    let cases = [
        (
            "zsh",
            claude_record(),
            vec![
                OsString::from("-l"),
                OsString::from("-i"),
                OsString::from("-c"),
                exec("claude", "--resume"),
            ],
        ),
        (
            "bash",
            codex_record(),
            vec![
                OsString::from("--init-file"),
                OsString::from("/folio/folio.bash"),
                OsString::from("-i"),
                OsString::from("-c"),
                exec("codex", "resume"),
            ],
        ),
        (
            "sh",
            claude_record(),
            vec![
                OsString::from("-l"),
                OsString::from("-i"),
                OsString::from("-c"),
                exec("claude", "--resume"),
            ],
        ),
    ];
    for (id, record, expected) in cases {
        let row = mac_row(id);
        let seed = resume_seed(id, record);
        let mut command = shell_integration::shell_command(
            &row,
            &profiles::shipped_kind(&row),
            &[],
            named_scripts(),
            &ZshAccount,
        );
        assert!(
            compose_resume(&mut command, &seed, &Started::AsAsked, &row),
            "{id}: the birth is a resume"
        );
        assert_eq!(command.arguments, expected, "{id}: the shell, told to exec");
    }
    let row = mac_row("zsh");
    let seed = resume_seed(
        "zsh",
        AgentRecordV1 {
            home: Some("/work/second-config".to_owned()),
            ..claude_record()
        },
    );
    let mut command = shell_integration::shell_command(
        &row,
        &profiles::shipped_kind(&row),
        &[],
        named_scripts(),
        &ZshAccount,
    );
    compose_resume(&mut command, &seed, &Started::AsAsked, &row);
    assert!(
        command.environment.contains(&(
            OsString::from("CLAUDE_CONFIG_DIR"),
            OsString::from("/work/second-config")
        )),
        "the session's configuration folder"
    );
}

/// RED (T-AGENT-RESUME-ON-MACOS) — **a shell whose spelling is not verified comes back as that
/// shell, saying which session did not come back**: a fish pane is handed no command, its seed
/// carries the could-not-resume line in the session's folder, and the account it is born with
/// holds no record, so the next start does not try again. zsh, bash and sh are not refused, and
/// neither is a pane whose own profile is the vendor's.
///
/// MUTATION (plant `plant-15-unknown-family-composed.py`): `shell_cannot_resume` refusing nothing,
/// and the fish pane is answered with no seed.
#[test]
fn a_shell_without_a_known_spelling_comes_back_with_the_line() {
    let mut fish = mac_row("zsh");
    fish.id = profiles::USER_SHELL_ID.to_owned();
    fish.program = profiles::ProgramSource::Path(PathBuf::from("/opt/homebrew/bin/fish"));
    let seed = resume_seed(&fish.id, claude_record());
    let shell = shell_cannot_resume(&seed, Some(&fish), &Started::AsAsked)
        .expect("a fish pane is handed no command");
    assert_eq!(shell.profile, fish.id, "its own shell");
    assert_eq!(shell.cwd, seed.cwd, "in the session's folder");
    let SeedAgent::CouldNotResume(could) = &shell.agent else {
        panic!("with the line: {:?}", shell.agent);
    };
    assert!(
        could.text().contains("Claude Code session 0b9c7e1a"),
        "{}",
        could.text()
    );
    assert_eq!(
        AgentAccount::born(&shell.agent, true).record(),
        None,
        "and no record"
    );
    let mut command = shell_integration::shell_command(
        &fish,
        &profiles::shipped_kind(&fish),
        &[],
        named_scripts(),
        &ZshAccount,
    );
    let before = command.arguments.clone();
    assert!(
        !compose_resume(&mut command, &seed, &Started::AsAsked, &fish),
        "and no resume is composed for it"
    );
    assert_eq!(command.arguments, before);
    for id in ["zsh", "bash", "sh"] {
        let row = mac_row(id);
        assert!(
            shell_cannot_resume(
                &resume_seed(id, claude_record()),
                Some(&row),
                &Started::AsAsked
            )
            .is_none(),
            "{id} is told the command"
        );
    }
    let mut claude = mac_row("zsh");
    claude.id = "claude".to_owned();
    claude.program = profiles::ProgramSource::Path(PathBuf::from("/usr/local/bin/claude"));
    assert!(
        shell_cannot_resume(
            &resume_seed("claude", claude_record()),
            Some(&claude),
            &Started::AsAsked
        )
        .is_none(),
        "the vendor's own pane is no shell"
    );
}

/// RED (T-AGENT-RESUME-ON-MACOS, ruling R4) — **a resumed pane is its vendor's agent whatever its
/// program is called**: on a Mac the pane's program is the shell that `exec`ed the vendor, and a
/// `/clear` there (a new id from the same vendor) is still recorded, while another vendor's turn
/// from inside it is refused (row 195's F1). A pane that is no resume is what its program says.
///
/// MUTATION (plant `plant-15-resumed-pane-heard-as-a-shell.py`): `AgentAccount::child` answering
/// what the program's name says, and the `/clear` is refused.
#[test]
fn a_resumed_pane_is_its_vendors_agent_whatever_its_program() {
    let claude = &VENDORS[0];
    let codex = &VENDORS[1];
    let zsh = Some(Path::new("/bin/zsh"));
    let mut account = AgentAccount::born(
        &SeedAgent::Resume(crate::agent_resume::Resume {
            record: claude_record(),
            hand_back: Some("zsh".to_owned()),
        }),
        true,
    );
    let cleared = Heard {
        id: "1c9c7e1a-4f3d-4c2b-9a8e-5d6f7a8b9c0d".to_owned(),
        cwd: FOLDER.to_owned(),
        home: None,
    };
    let child = account.child(zsh, &[]);
    assert!(
        matches!(child, crate::agent_resume::Child::Agent(vendor) if vendor.family == claude.family),
        "the resumed pane is Claude Code's: {child:?}"
    );
    assert!(
        account.hear(claude, &cleared, child),
        "a /clear in the resumed pane is recorded"
    );
    assert_eq!(
        account.record().map(|record| record.resume_id.as_str()),
        Some(cleared.id.as_str())
    );
    let child = account.child(zsh, &[]);
    assert!(
        !account.hear(codex, &cleared, child),
        "another vendor's turn from inside it is refused"
    );
    assert!(
        matches!(
            AgentAccount::default().child(zsh, &[]),
            crate::agent_resume::Child::Shell(_)
        ),
        "a pane that is no resume is its program's"
    );
}

/// RED (T-AGENT-RESUME-ON-MACOS) — **the macOS twin of
/// `a_saved_agent_pane_with_a_recorded_session_is_born_through_its_vendors_resume`**: a saved zsh
/// pane with a Claude Code record and a saved bash pane with a Codex record, through this Mac's own
/// table — `revive_plan_with` → `birth_here` → `shell_command` → `compose_resume` →
/// `AgentAccount::born` — are born as their own shells in the session's folder, owing nothing to
/// a prompt, told to `exec` the vendor's resume, and remember the shell they hand back.
///
/// MUTATION (plants `plant-15-mac-not-resumed.py`, `plant-15-shell-argv-replaced.py`): the Mac
/// not resumed, and the pane comes back as its shell with its prefill; the shell's words replaced,
/// and the argv assertion goes red.
#[cfg(target_os = "macos")]
#[test]
fn a_saved_shell_pane_with_a_recorded_session_is_born_as_its_shell_execing_the_vendors_resume() {
    const ON_THIS_MAC: Restore = Restore {
        switch: true,
        platform: HostPlatform::MacOs,
    };
    let every_row =
        profiles::ProfilePrograms::with_only(&(0..profiles::count()).collect::<Vec<_>>());
    for (saved_as, record, command_word) in [
        (
            "zsh",
            claude_record(),
            format!("exec claude --resume {SESSION}"),
        ),
        (
            "bash",
            codex_record(),
            format!("exec codex resume {SESSION}"),
        ),
    ] {
        let tab = saved_agent_tab(saved_as, "claude", Some(record.clone()));
        let seed = revived_seed(&tab, ON_THIS_MAC);
        assert_eq!(seed.profile, saved_as, "{saved_as}: born as its own shell");
        assert_eq!(
            seed.cwd.as_ref().map(profiles::SeedPlace::path),
            Some(Path::new(FOLDER)),
            "{saved_as}: in the folder the session is filed under"
        );
        assert_eq!(
            seed.prefill, None,
            "{saved_as}: nothing is typed at a resume"
        );
        let (born, started) = birth_here(&seed, "", &every_row).expect("every row is answered");
        assert_eq!(started, Started::AsAsked);
        assert_eq!(born.profile, saved_as);
        let row = profiles::row_of(&born.profile).expect("this Mac ships the row");
        let mut command = shell_integration::shell_command(
            &row,
            &profiles::shipped_kind(&row),
            &[],
            named_scripts(),
            &bt_pty::SystemShellEnvironment,
        );
        let mut expected = command.arguments.clone();
        if !expected.iter().any(|word| word == "-i") {
            expected.push(OsString::from("-i"));
        }
        expected.push(OsString::from("-c"));
        expected.push(OsString::from(command_word));
        assert!(
            compose_resume(&mut command, &born, &started, &row),
            "{saved_as}: the birth is a resume"
        );
        assert_eq!(
            command.arguments, expected,
            "{saved_as}: the shell's own words, then the vendor's resume"
        );
        let account = AgentAccount::born(&born.agent, true);
        assert_eq!(account.record(), Some(&record), "{saved_as}");
        assert!(account.is_resumed());
        assert_eq!(
            account.saved_profile(saved_as),
            saved_as,
            "{saved_as}: the layout goes on writing the pane as its shell"
        );
    }
}
