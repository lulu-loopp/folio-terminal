//! **The crate root: attention and notifications.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::{
    BOTH_NOTIFICATION_ROWS_ON, on_a_screen, one_turn, request_attention, ringing_tab,
};
use std::time::Duration;

/// PIN (§7.1.5b P1-8) — **`Ctrl+Shift+A` serves the oldest, then walks on,
/// then wraps.**
///
/// The ruling's three clauses against the one function that implements them,
/// with the queue handed over out of order on purpose: the walk is a function
/// of the serials, not of the order anything was collected in.
///
/// Red gate: order by the tuple's first element and the first assertion goes
/// red; drop the pivot and the second does; fall back to `.next()` instead of
/// rotating and the last does.
#[test]
fn the_attention_queue_serves_the_oldest_then_walks_on_and_wraps() {
    let queue = [("c", 9_u64), ("a", 2), ("b", 5)];
    assert_eq!(next_attention_stop::<&str>(&[], None), None);
    assert_eq!(
        next_attention_stop(&queue, None),
        Some("a"),
        "先到先服务 — the oldest serial, whatever order the window walked its \
             tabs in"
    );
    assert_eq!(
        next_attention_stop(&queue, Some(2)),
        Some("b"),
        "已在队列中再按 = 走到下一个"
    );
    assert_eq!(next_attention_stop(&queue, Some(5)), Some("c"));
    assert_eq!(
        next_attention_stop(&queue, Some(9)),
        Some("a"),
        "循环 — past the newest is back to the oldest"
    );
    assert_eq!(
        next_attention_stop(&[("only", 4_u64)], Some(4)),
        Some("only"),
        "a queue of one wraps onto itself rather than answering nothing"
    );
    assert_eq!(
        next_attention_stop(&queue, Some(7)),
        Some("c"),
        "and a pivot that is nobody's serial still names a position in the \
             order — the walk is over serials, not over membership"
    );
}

/// PIN (census-3, `docs/plans/design/ownership-census-2026-09-25.md` §R6) — **the window's
/// attention pass arms a wait's ten minutes when it arrives, forgets them when its clear arrives,
/// and spends them when they run out.**
///
/// The rule and the clock left this crate for `bt-workbench` (`attention::expiry`), where their own
/// pure tests live. What stayed here is *where the clock is driven*: `deliver_attention` arms and
/// forgets it as a hook's line lands, `settle_attention` spends what has come due on the next
/// pass. Neither half can be seen from the other crate, so this walks both through the product's
/// own functions — a real message naming a real pane's capability, looked up in the shipped
/// Claude Code rows — and reads the answer the loop reads, `attention_ledger_deadline`.
///
/// MUTATION: drop the `arm` arm in `deliver_attention` and the first assertion goes red; drop the
/// `forget` arm and the second does; take the `due` loop out of `settle_attention` and the last
/// pair does.
#[test]
fn a_waits_ten_minutes_are_armed_on_arrival_forgotten_on_its_clear_and_spent_when_they_run_out() {
    use attention::expiry::WAIT_TTL;

    let mut tabs = vec![ringing_tab(1, 1)];
    let seat = tabs[0].seats.terminals()[0];
    let capability = attention_wire::mint_capability();
    tabs[0]
        .sessions
        .get_mut(&seat)
        .expect("the fixture's seat holds a shell")
        .session
        .attention
        .capability
        .clone_from(&capability);
    let installed =
        attention_map::installed_rows(attention_map::ROWS, attention_map::CLAUDE_CODE, |_| true);
    let said = |event: &str| attention_wire::Message {
        capability: capability.clone(),
        family: attention_map::CLAUDE_CODE.to_owned(),
        event: event.to_owned(),
        id: None,
        text: None,
        resume: None,
        another_client: false,
    };
    let mut places = attention::Places::default();
    let start = Instant::now();
    let deliver = |tabs: &mut [TabState], places: &mut attention::Places, event: &str| {
        deliver_attention(
            tabs,
            1,
            on_a_screen(false),
            BOTH_NOTIFICATION_ROWS_ON,
            places,
            &[said(event)],
            &installed,
            start,
            None,
            &mut Vec::new(),
        );
    };
    let settle = |tabs: &mut [TabState], places: &mut attention::Places, now: Instant| {
        settle_attention(
            tabs,
            1,
            on_a_screen(false),
            BOTH_NOTIFICATION_ROWS_ON,
            places,
            now,
            None,
            &mut Vec::new(),
        );
    };

    deliver(&mut tabs, &mut places, "PermissionRequest");
    assert_eq!(
        attention_ledger_deadline(&tabs),
        Some(start + WAIT_TTL),
        "a wait that arrives starts its ten minutes"
    );
    deliver(&mut tabs, &mut places, "Stop");
    assert_eq!(
        attention_ledger_deadline(&tabs),
        None,
        "the clear that ends it takes its clock with it"
    );

    deliver(&mut tabs, &mut places, "PermissionRequest");
    settle(
        &mut tabs,
        &mut places,
        start + WAIT_TTL - Duration::from_secs(1),
    );
    assert_ne!(
        tabs[0].sessions[&seat].session.attention.ledger.state(),
        attention::State::Idle,
        "a second before the ten minutes are up, the pane is still asking"
    );
    settle(&mut tabs, &mut places, start + WAIT_TTL);
    assert_eq!(
        attention_ledger_deadline(&tabs),
        None,
        "a clock that ran out is spent, not repeated"
    );
    assert_eq!(
        tabs[0].sessions[&seat].session.attention.ledger.state(),
        attention::State::Idle,
        "and the wait it stood for is over"
    );
}

/// The window the ending-step pins end: two tabs of one pane each, the second pane still in birth,
/// and both panes holding a standing request a settle pass has queued.
fn a_window_of_two_asking_panes() -> (Vec<TabState>, attention::Places) {
    let mut tabs = vec![ringing_tab(1, 1), ringing_tab(2, 1)];
    let in_birth = tabs[1].seats.terminals()[0];
    tabs[1]
        .sessions
        .get_mut(&in_birth)
        .expect("the fixture's seat holds a pane")
        .session
        .life
        .begin_birth(PaneBirth {
            seed: LeafSeed::default(),
            probe_input: None,
            typed: std::cell::RefCell::default(),
            waiting: BirthWait::Locale,
        });
    for tab in &mut tabs {
        let seat = tab.seats.terminals()[0];
        request_attention(tab, seat, "yes");
    }
    let mut places = attention::Places::default();
    one_turn(&mut tabs, 0, false, &mut places);
    for tab in &tabs {
        for (seat, leaf) in tab.leaves() {
            assert_ne!(
                leaf.attention.ledger.state(),
                attention::State::Idle,
                "the fixture's own precondition: {seat:?} holds a request"
            );
        }
    }
    (tabs, places)
}

/// [`end_leaves_of`] over every tab given, as a window-ending road asks it: the places it gave
/// up, counted per pane — one count per `LeafGone` the step *applied*, which a trace line cannot
/// tell, because the ledger answers a second `LeafGone` with nothing.
fn end_and_count<'t>(
    tabs: impl IntoIterator<Item = (usize, &'t mut TabState)>,
    moving: Option<&DragSource>,
    places: &mut attention::Places,
    counts: &mut BTreeMap<(usize, SeatId), usize>,
) {
    let place = on_a_screen(false);
    for site in end_leaves_of(
        tabs,
        None,
        moving,
        |index| notify::desktop_reach(index == 0, place),
        places,
        Instant::now(),
        None,
    ) {
        *counts.entry((site.tab, site.seat)).or_default() += 1;
    }
}

/// PIN (ownership contract 2026-10-11, (d)2 finding F-1) — **every road that ends a pane emits
/// its `LeafGone` once**: the ending step the five roads share (`close_pane`, `close_tab`,
/// `let_go_of_this_window` for a window close and for the quit, `abandon_a_window_nothing_arrived_in`,
/// and `answer_restore`'s placeholder removal), counted in applications.
///
/// (i) Every leaf of a window, the pane in birth included, is given up once. (ii) The same window
/// walked a second time — a window closed and then retired by a quit before it is reaped — gives
/// nothing more up. (iii) A tab a pending handover is carrying out keeps its places, and gives
/// them up once when it later ends where it landed. (iv) The launch placeholder, which holds a real
/// shell, gives its place up once and has its shell retired after.
///
/// The roads' own calls into this step are not driven here: a `Runtime` is a window, and no test
/// builds one (`docs/plans/structural-debt.md`, the ending-step row).
///
/// MUTATIONS: drop the `attention_ended` mark and (ii) goes red at 2; ignore `moving` and (iii)
/// goes red on the tab in flight; pass over a pane in birth and (i) goes red on it.
#[test]
fn every_road_that_ends_a_pane_emits_leaf_gone_once() {
    use bt_pty::test_shell::TestShell;

    // (i) and (ii): one window, ended, and ended again.
    let (mut tabs, mut places) = a_window_of_two_asking_panes();
    let panes: Vec<(usize, SeatId)> = tabs
        .iter()
        .enumerate()
        .flat_map(|(index, tab)| tab.leaves().map(move |(seat, _)| (index, *seat)))
        .collect();
    let mut counts = BTreeMap::new();
    end_and_count(tabs.iter_mut().enumerate(), None, &mut places, &mut counts);
    for pane in &panes {
        assert_eq!(counts.get(pane), Some(&1), "(i) {pane:?} is ended once");
    }
    end_and_count(tabs.iter_mut().enumerate(), None, &mut places, &mut counts);
    for pane in &panes {
        assert_eq!(
            counts.get(pane),
            Some(&1),
            "(ii) a second walk of the same window ends {pane:?} again"
        );
    }

    // (iii): a handover carrying tab 2 out of a window that closes in the same turn.
    let (mut tabs, mut places) = a_window_of_two_asking_panes();
    let moving = DragSource::Tab(tabs[1].id);
    let mut counts = BTreeMap::new();
    end_and_count(
        tabs.iter_mut().enumerate(),
        Some(&moving),
        &mut places,
        &mut counts,
    );
    let staying = (0, tabs[0].seats.terminals()[0]);
    let moved = tabs[1].seats.terminals()[0];
    assert_eq!(counts.get(&staying), Some(&1), "(iii) the staying tab ends");
    assert_eq!(
        counts.get(&(1, moved)),
        None,
        "(iii) the tab in flight is not ending"
    );
    assert_ne!(
        tabs[1].sessions[&moved].session.attention.ledger.state(),
        attention::State::Idle,
        "(iii) and still holds its request"
    );
    // It lands as the only tab of another window, and is closed there.
    let mut landed = tabs.remove(1);
    let mut there = attention::Places::default();
    let mut counts = BTreeMap::new();
    end_and_count([(0, &mut landed)], None, &mut there, &mut counts);
    assert_eq!(
        counts.get(&(0, moved)),
        Some(&1),
        "(iii) once, where it ended"
    );

    // (iv): the launch placeholder, holding a real shell, taken away once the restored tabs stand.
    let size = PtySize::cells(
        std::num::NonZeroU16::new(48).expect("columns"),
        std::num::NonZeroU16::new(8).expect("rows"),
    );
    let (shell, hygiene) = TestShell::spawn_default(size)
        .expect("a real shell")
        .into_session();
    let mut placeholder = ringing_tab(1, 1);
    let seat = placeholder.seats.terminals()[0];
    placeholder
        .sessions
        .get_mut(&seat)
        .expect("the placeholder's pane")
        .session
        .life
        .land(Some(shell));
    request_attention(&mut placeholder, seat, "yes");
    let mut tabs = vec![placeholder];
    let mut places = attention::Places::default();
    one_turn(&mut tabs, 0, false, &mut places);
    let mut counts = BTreeMap::new();
    end_and_count([(0, &mut tabs[0])], None, &mut places, &mut counts);
    assert_eq!(
        counts.get(&(0, seat)),
        Some(&1),
        "the placeholder's pane ends once"
    );
    assert!(
        tabs[0].sessions[&seat].session.life.pty().is_none(),
        "and its shell is retired"
    );
    drop(tabs);
    drop(hygiene);
}

// ── 0.4.10 plan row 75: an agent pane's blue dot is read from its turns ──
//
// T-UNREAD-DOT-ON-EVERY-BACKGROUND-TAB. The sequences below are the shapes the hooks reference
// documents and the owner's machine runs — a pane whose program is the agent (every pane 0.4.9's
// resume brings back), a turn the agent starts by itself, an agent typed into an integrated shell —
// with invented capabilities and no folder at all: nothing here asks the disk.

/// One pane in a background tab (the window's active tab is 1, and there is only tab 0), running
/// `program`, holding a capability its hooks can name.
fn an_agent_pane(program: &str) -> (Vec<TabState>, SeatId, String) {
    let mut tabs = vec![ringing_tab(1, 1)];
    let seat = tabs[0].seats.terminals()[0];
    let capability = attention_wire::mint_capability();
    let leaf = &mut tabs[0]
        .sessions
        .get_mut(&seat)
        .expect("the fixture's seat holds a shell")
        .session;
    leaf.attention.capability.clone_from(&capability);
    leaf.launch.program = Some(PathBuf::from(program));
    (tabs, seat, capability)
}

/// Every family's installed rows, as a machine that installed all three would hold them.
fn every_familys_rows() -> Vec<attention::MappingRow> {
    [
        attention_map::CLAUDE_CODE,
        attention_map::CODEX,
        attention_map::COPILOT,
    ]
    .into_iter()
    .flat_map(|family| attention_map::installed_rows(attention_map::ROWS, family, |_| true))
    .collect()
}

/// `family:event` from the pane holding `capability`, delivered with the window's active tab
/// `active` — 1 for "the user is elsewhere", 0 for "the user is on this tab".
fn hook(tabs: &mut [TabState], capability: &str, family: &str, event: &str, active: usize) {
    deliver_attention(
        tabs,
        active,
        on_a_screen(true),
        BOTH_NOTIFICATION_ROWS_ON,
        &mut attention::Places::default(),
        &[attention_wire::Message {
            capability: capability.to_owned(),
            family: family.to_owned(),
            event: event.to_owned(),
            id: None,
            text: None,
            resume: None,
            another_client: false,
        }],
        &every_familys_rows(),
        Instant::now(),
        None,
        &mut Vec::new(),
    );
}

/// The program in the pane writes `chunks` reads' worth — a spinner frame, a status line, a
/// repaint — counted the way the drain counts them.
fn the_agent_draws(tabs: &mut [TabState], seat: SeatId, chunks: u64) {
    let leaf = &mut tabs[0]
        .sessions
        .get_mut(&seat)
        .expect("the fixture's pane")
        .session;
    for _ in 0..chunks {
        leaf.output_revision =
            runtime::terminal::output_revision(leaf.output_revision, true, false);
    }
}

/// The pane's cells go to the glass: the user is looking at it.
fn the_pane_is_painted(tabs: &mut [TabState], seat: SeatId) {
    mark_leaf_painted(
        &mut tabs[0]
            .sessions
            .get_mut(&seat)
            .expect("the fixture's pane")
            .session,
    );
}

/// The dot tab 0 wears while it is not the tab on screen.
fn background_dot(tabs: &[TabState]) -> StatusClaim {
    tabs[0].fleet_claim_for(false)
}

/// RED (row 75) — **a resumed agent's pane wears no dot for what it draws, and wears one for a
/// turn that ended while the user was away** — the owner's every background tab of 2026-11-06,
/// and the fix. Born as its agent's resume, the pane is read by its turns from birth: the bytes
/// its agent draws while it reloads, and between turns, are not "finished". A turn that ended out
/// of view is, until it is painted; and what it draws after that — a status line, a repaint for
/// the focus report leaving the tab sends — brings nothing back.
///
/// MUTATION: in `SessionFacts::claim`, read `self.has_unseen_output()` in the `Some(turn)` arm
/// (the byte rule kept for agent panes) and the first assertion goes red.
#[test]
fn a_resumed_agents_pane_is_unread_by_its_turns_and_not_by_its_bytes() {
    let (mut tabs, seat, capability) = an_agent_pane("claude.exe");
    let resumed = crate::agent_resume::AgentAccount::born(
        &crate::agent_resume::SeedAgent::Resume(crate::agent_resume::Resume {
            record: bt_persist::AgentRecordV1 {
                vendor: attention_map::CLAUDE_CODE.to_owned(),
                resume_id: "0b9c7e1a-4f3d-4c2b-9a8e-5d6f7a8b9c0d".to_owned(),
                cwd: r"C:\work\resume".to_owned(),
                home: None,
            },
            hand_back: None,
        }),
        true,
    );
    let leaf = &mut tabs[0]
        .sessions
        .get_mut(&seat)
        .expect("the fixture's pane")
        .session;
    leaf.turn_account = agent_turns::TurnAccount::born_beside(&resumed);
    leaf.agent = resumed;
    the_agent_draws(&mut tabs, seat, 40);
    assert_eq!(
        background_dot(&tabs),
        StatusClaim::Silent,
        "what a resumed agent draws is no finished turn"
    );
    let claude = attention_map::CLAUDE_CODE;
    hook(&mut tabs, &capability, claude, "UserPromptSubmit", 0);
    the_pane_is_painted(&mut tabs, seat);
    the_agent_draws(&mut tabs, seat, 12);
    assert_eq!(
        background_dot(&tabs),
        StatusClaim::Silent,
        "a turn under way, whatever it draws"
    );
    hook(&mut tabs, &capability, claude, "Stop", 1);
    assert_eq!(
        background_dot(&tabs),
        StatusClaim::Unread,
        "a turn that went on writing after the user left, and ended"
    );
    the_pane_is_painted(&mut tabs, seat);
    the_agent_draws(&mut tabs, seat, 3);
    assert_eq!(
        background_dot(&tabs),
        StatusClaim::Silent,
        "looked at; what it draws afterwards is not news"
    );
}

/// RED (row 75) — **a turn the agent starts by itself hides the end before it, and its own end
/// lights the dot**: `Stop`, then the `UserPromptSubmit` Claude Code's reference says fires on
/// turns it starts on its own, a tool reporting in, and the next `Stop` — work in flight outranks
/// a finished claim (D35) in an agent's pane exactly as in a shell's. And a turn whose start no
/// hook announced is heard from inside it: the `PostToolUse` alone puts the earlier end away.
///
/// MUTATION: `TurnStep::Began | TurnStep::Within => {}` in `TurnAccount::hear_turn_step` and the
/// second assertion goes red.
#[test]
fn a_turn_the_agent_starts_itself_is_work_in_flight_until_it_ends() {
    let (mut tabs, seat, capability) = an_agent_pane("claude.exe");
    let claude = attention_map::CLAUDE_CODE;
    hook(&mut tabs, &capability, claude, "UserPromptSubmit", 0);
    the_pane_is_painted(&mut tabs, seat);
    the_agent_draws(&mut tabs, seat, 5);
    hook(&mut tabs, &capability, claude, "Stop", 1);
    assert_eq!(background_dot(&tabs), StatusClaim::Unread);
    hook(&mut tabs, &capability, claude, "UserPromptSubmit", 1);
    the_agent_draws(&mut tabs, seat, 5);
    assert_eq!(
        background_dot(&tabs),
        StatusClaim::Silent,
        "a self-started turn is under way"
    );
    hook(&mut tabs, &capability, claude, "PostToolUse", 1);
    hook(&mut tabs, &capability, claude, "Stop", 1);
    assert_eq!(background_dot(&tabs), StatusClaim::Unread, "and it ended");
    // No announced start this time: the tool's receipt is the turn running.
    hook(&mut tabs, &capability, claude, "PostToolUse", 1);
    assert_eq!(
        background_dot(&tabs),
        StatusClaim::Silent,
        "a turn heard from inside it"
    );
}

/// RED (row 75) — **a turn that ended on the tab the user is looking at is seen, so leaving the
/// tab owes nothing**, whatever the agent draws after the user has gone. Seeing is painting, and
/// the end is held at the revision it was heard at.
///
/// MUTATION: hold the end at `u64::MAX` in `TurnAccount::hear_turn_step` instead of at
/// `output_revision`, and the pane is unread for a turn the user watched end.
#[test]
fn a_turn_that_ended_in_view_owes_nothing_after_the_user_leaves() {
    let (mut tabs, seat, capability) = an_agent_pane("claude.exe");
    let claude = attention_map::CLAUDE_CODE;
    hook(&mut tabs, &capability, claude, "UserPromptSubmit", 0);
    the_agent_draws(&mut tabs, seat, 8);
    the_pane_is_painted(&mut tabs, seat);
    hook(&mut tabs, &capability, claude, "Stop", 0);
    the_pane_is_painted(&mut tabs, seat);
    the_agent_draws(&mut tabs, seat, 2);
    assert_eq!(background_dot(&tabs), StatusClaim::Silent);
}

/// RED (row 75, the owner's ruling R4) — **a fresh agent pane before its first hook keeps the
/// shell's rule**: what it draws reads as unread exactly as before this row, and its first turn
/// arrival binds it to its turns.
///
/// MUTATION: in `ShellSession::session_facts`, read every pane by its turns (`Some` of an unbound
/// account as an `AgentTurn` with nothing ended) and the first assertion goes red.
#[test]
fn a_fresh_agent_pane_keeps_the_byte_rule_until_its_first_hook() {
    let (mut tabs, seat, capability) = an_agent_pane("claude.exe");
    the_agent_draws(&mut tabs, seat, 3);
    assert_eq!(
        background_dot(&tabs),
        StatusClaim::Unread,
        "no hook heard: the shell's rule"
    );
    hook(
        &mut tabs,
        &capability,
        attention_map::CLAUDE_CODE,
        "UserPromptSubmit",
        1,
    );
    assert_eq!(
        background_dot(&tabs),
        StatusClaim::Silent,
        "bound, and a turn under way"
    );
}

/// RED (row 75, D4's first branch) — **an agent typed into an integrated shell gets its dot at
/// its turns' ends**, although the shell's `C` says "working" for the agent's whole life; and the
/// shell's next prompt hands the pane back to the shell's rule, where its command boundary is
/// read again.
///
/// MUTATION: keep `self.status.working` in the `Some(turn)` arm of `SessionFacts::claim` and the
/// first `Unread` assertion goes red (the pane wears nothing, as before this row).
#[test]
fn an_agent_typed_into_a_shell_wears_its_dot_at_its_turns_ends() {
    let (mut tabs, seat, capability) = an_agent_pane("pwsh.exe");
    tabs[0]
        .sessions
        .get_mut(&seat)
        .expect("the fixture's pane")
        .session
        .session
        .feed(b"\x1b]133;A\x07PS> \x1b]133;B\x07claude\r\n\x1b]133;C\x07")
        .expect("the marks parse");
    let claude = attention_map::CLAUDE_CODE;
    hook(&mut tabs, &capability, claude, "UserPromptSubmit", 0);
    the_pane_is_painted(&mut tabs, seat);
    the_agent_draws(&mut tabs, seat, 6);
    hook(&mut tabs, &capability, claude, "Stop", 1);
    assert_eq!(background_dot(&tabs), StatusClaim::Unread);
    the_pane_is_painted(&mut tabs, seat);
    let leaf = &mut tabs[0]
        .sessions
        .get_mut(&seat)
        .expect("the fixture's pane")
        .session;
    leaf.session
        .feed(b"\x1b]133;D;0\x07\x1b]133;A\x07PS> ")
        .expect("the marks parse");
    leaf.turn_account
        .settle_turn_command(leaf.session.command_marks());
    assert_eq!(leaf.turn_account.agent_turn(), None, "the shell's again");
    the_agent_draws(&mut tabs, seat, 1);
    assert_eq!(
        background_dot(&tabs),
        StatusClaim::Unread,
        "and its own command boundary is read as it always was"
    );
}

/// RED (row 75) — **codex and copilot by their own rows, and nothing else**: codex's `notify`
/// turn end alone (no hook of its says a turn began), copilot's `userPromptSubmitted` and
/// `agentStop`. The same account, the same claim, no family named in the code.
///
/// MUTATION: answer `turn: None` in `asks_of` (no arrival says anything about a turn) and the
/// codex pane stays on the byte rule: its first assertion goes red.
#[test]
fn codex_and_copilot_panes_are_read_by_their_own_turn_rows() {
    let (mut tabs, seat, capability) = an_agent_pane("codex.exe");
    let codex = attention_map::CODEX;
    hook(&mut tabs, &capability, codex, "agent-turn-complete", 0);
    the_pane_is_painted(&mut tabs, seat);
    the_agent_draws(&mut tabs, seat, 30);
    assert_eq!(
        background_dot(&tabs),
        StatusClaim::Silent,
        "codex streaming its next answer is not finished"
    );
    hook(&mut tabs, &capability, codex, "agent-turn-complete", 1);
    assert_eq!(background_dot(&tabs), StatusClaim::Unread);

    let (mut tabs, seat, capability) = an_agent_pane("pwsh.exe");
    tabs[0]
        .sessions
        .get_mut(&seat)
        .expect("the fixture's pane")
        .session
        .session
        .feed(b"\x1b]133;A\x07PS> \x1b]133;B\x07copilot\r\n\x1b]133;C\x07")
        .expect("the marks parse");
    let copilot = attention_map::COPILOT;
    hook(&mut tabs, &capability, copilot, "userPromptSubmitted", 0);
    the_pane_is_painted(&mut tabs, seat);
    the_agent_draws(&mut tabs, seat, 4);
    assert_eq!(background_dot(&tabs), StatusClaim::Silent);
    hook(&mut tabs, &capability, copilot, "agentStop", 1);
    assert_eq!(background_dot(&tabs), StatusClaim::Unread);
}

/// PIN (row 75) — **the claim ladder over an agent's turns**: a standing request outranks
/// everything, as before; an unseen turn end is blue and never red — the shell's exit code is not
/// the agent's turn (the owner's ruling R5); a turn under way, or an end already painted, is
/// silent whatever the bytes say.
#[test]
fn an_agent_turns_claim_ladder() {
    use crate::test_support::{agent_turn_facts, quiet};
    assert_eq!(
        agent_turn_facts(Some(5), false, 9, 4, false).claim(),
        StatusClaim::Unread
    );
    assert_eq!(
        agent_turn_facts(Some(5), false, 9, 4, true).claim(),
        StatusClaim::Silent,
        "the tab on screen is never unread"
    );
    assert_eq!(
        agent_turn_facts(Some(5), true, 9, 4, false).claim(),
        StatusClaim::Silent,
        "a turn under way"
    );
    assert_eq!(
        agent_turn_facts(Some(5), false, 9, 5, false).claim(),
        StatusClaim::Silent,
        "the end was painted; the bytes after it are not news"
    );
    assert_eq!(
        agent_turn_facts(None, false, 9, 0, false).claim(),
        StatusClaim::Silent,
        "no turn has ended"
    );
    let failed_shell = SessionFacts {
        status: SessionStatus {
            failure_exit_code: Some(1),
            ..quiet()
        },
        ..agent_turn_facts(Some(5), false, 9, 4, false)
    };
    assert_eq!(
        failed_shell.claim(),
        StatusClaim::Unread,
        "the shell's last exit code is not the agent's turn"
    );
    let asking = SessionFacts {
        awaiting: true,
        ..agent_turn_facts(Some(5), true, 9, 4, false)
    };
    assert_eq!(asking.claim(), StatusClaim::Awaiting);
}

/// RED (row 75) — **every hook that reaches a pane writes a `hear` line**, the decisions or not:
/// a `PostToolUse` that clears nothing and a second `Stop` whose turn's end was already announced
/// decide nothing, and are in the file all the same, so the gap between a turn's end and the next
/// arrival can be read off it. A hook naming no pane writes nothing.
///
/// MUTATION: drop the `hear_line` emission from `deliver_attention` and the assertion goes red.
#[test]
fn every_hook_that_reaches_a_pane_writes_a_hear_line() {
    let path = std::env::temp_dir().join(format!(
        "{}.log",
        bt_testpath::unique_name("bt-attention-trace-hear")
    ));
    let _ = std::fs::remove_file(&path);
    let capture = crate::trace_sink::Capture::new();
    let trace = crate::trace::Trace::create(&path, "# pin", capture.queue());
    let (mut tabs, seat, capability) = an_agent_pane("claude.exe");
    let claude = attention_map::CLAUDE_CODE;
    let mut places = attention::Places::default();
    for (named, event) in [
        (capability.as_str(), "UserPromptSubmit"),
        (capability.as_str(), "PostToolUse"),
        (capability.as_str(), "Stop"),
        (capability.as_str(), "Stop"),
        ("a capability no pane holds", "Stop"),
    ] {
        deliver_attention(
            &mut tabs,
            1,
            on_a_screen(true),
            BOTH_NOTIFICATION_ROWS_ON,
            &mut places,
            &[attention_wire::Message {
                capability: named.to_owned(),
                family: claude.to_owned(),
                event: event.to_owned(),
                id: None,
                text: None,
                resume: None,
                another_client: false,
            }],
            &every_familys_rows(),
            Instant::now(),
            Some(&trace),
            &mut Vec::new(),
        );
    }
    capture.write_out();
    let heard: Vec<String> = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .skip(1)
        .filter_map(|line| line.trim_start().split_once(' ').map(|(_, rest)| rest))
        .filter(|line| line.starts_with("hear "))
        .map(str::to_owned)
        .collect();
    let _ = std::fs::remove_file(&path);
    assert_eq!(
        heard,
        ["UserPromptSubmit", "PostToolUse", "Stop", "Stop"]
            .map(|event| format!("hear tab=0 seat={seat:?} episode=- event=claude-code:{event}"))
    );
}

/// What reaches the pane when an agent in it runs `codex exec`: the nested codex's `notify`, its
/// payload naming the client that ran it, made by the verb exactly as the hook makes it.
fn a_codex_exec_ends_its_turn(tabs: &mut [TabState], capability: &str, active: usize) {
    let payload = serde_json::json!({
        "type": "agent-turn-complete",
        "thread-id": "0b9c7e1a-4f3d-4c2b-9a8e-5d6f7a8b9c0d",
        "cwd": "/srv/work",
        "client": "codex-exec",
    });
    deliver_attention(
        tabs,
        active,
        on_a_screen(true),
        BOTH_NOTIFICATION_ROWS_ON,
        &mut attention::Places::default(),
        &[attention_wire::message_for(
            capability.to_owned(),
            attention_map::CODEX,
            "agent-turn-complete",
            Some(&payload),
        )],
        &every_familys_rows(),
        Instant::now(),
        None,
        &mut Vec::new(),
    );
}

/// RED (row 75, review N1) — **a `codex exec` an agent runs ends no turn of the pane's**: Claude
/// Code's Bash tool runs it, and its hooks name the pane, because they inherit its capability.
/// Through codex's `Stop` hook or its `notify`, the nested run's end is another vendor's, and the
/// Claude pane's turn under way stays under way; Claude's own `Stop` is the end. In a codex pane,
/// the nested run's `notify` says another client ran it, and lights nothing.
///
/// MUTATION: drop the `own.family != family` refusal in `TurnAccount::hear_turn_step` and the
/// first assertion goes red (plant `plant-75-any-family-is-the-panes.py`).
#[test]
fn a_codex_exec_inside_an_agent_pane_ends_no_turn_of_its() {
    let (mut tabs, seat, capability) = an_agent_pane("claude.exe");
    let claude = attention_map::CLAUDE_CODE;
    hook(&mut tabs, &capability, claude, "UserPromptSubmit", 0);
    the_pane_is_painted(&mut tabs, seat);
    the_agent_draws(&mut tabs, seat, 7);
    hook(&mut tabs, &capability, attention_map::CODEX, "stop", 1);
    a_codex_exec_ends_its_turn(&mut tabs, &capability, 1);
    assert_eq!(
        background_dot(&tabs),
        StatusClaim::Silent,
        "Claude's turn is still under way"
    );
    hook(&mut tabs, &capability, claude, "Stop", 1);
    assert_eq!(
        background_dot(&tabs),
        StatusClaim::Unread,
        "and this is its end"
    );

    let (mut tabs, seat, capability) = an_agent_pane("codex.exe");
    hook(&mut tabs, &capability, attention_map::CODEX, "stop", 0);
    the_pane_is_painted(&mut tabs, seat);
    the_agent_draws(&mut tabs, seat, 4);
    a_codex_exec_ends_its_turn(&mut tabs, &capability, 1);
    assert_eq!(
        background_dot(&tabs),
        StatusClaim::Silent,
        "a turn another client ran is not the codex pane's"
    );
}
