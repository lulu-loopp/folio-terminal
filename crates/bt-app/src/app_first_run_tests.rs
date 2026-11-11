//! **The crate root: first run and integration.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;

#[test]
fn private_resize_repaint_input_is_exact_and_integration_gated() {
    assert_eq!(
        psreadline_resize_repaint_input(profiles::Integration::PowerShellOptIn, true),
        Some(PSREADLINE_INVOKE_PROMPT_INPUT)
    );
    assert_eq!(
        psreadline_resize_repaint_input(profiles::Integration::PowerShellOptIn, false),
        None,
        "a session without an open OSC 133 input region injects zero bytes"
    );
}

#[test]
fn resize_storm_reanchor_debt_is_replaced_and_paid_once() {
    fn powershell(pending: &mut bool) -> ResizeReanchor<'_> {
        ResizeReanchor {
            pending,
            integration: profiles::Integration::PowerShellOptIn,
        }
    }
    let mut pending = false;
    for _ in 0..3 {
        replace_psreadline_resize_reanchor_debt(powershell(&mut pending), true);
    }
    assert_eq!(
        take_psreadline_resize_reanchor_input(powershell(&mut pending), true),
        Some(PSREADLINE_INVOKE_PROMPT_INPUT),
        "three commits in one open-input transaction coalesce to one chord"
    );
    assert_eq!(
        take_psreadline_resize_reanchor_input(powershell(&mut pending), true),
        None,
        "the repair debt is one shot"
    );

    replace_psreadline_resize_reanchor_debt(powershell(&mut pending), true);
    replace_psreadline_resize_reanchor_debt(powershell(&mut pending), false);
    assert_eq!(
        take_psreadline_resize_reanchor_input(powershell(&mut pending), true),
        None,
        "a later closed-region commit replaces stale open-prompt debt"
    );
    replace_psreadline_resize_reanchor_debt(powershell(&mut pending), true);
    assert_eq!(
        take_psreadline_resize_reanchor_input(powershell(&mut pending), false),
        None,
        "a prompt that closes before quiescence receives no stale chord"
    );
}

/// A carry-pass answer for the three agents: each `found`, nothing unrepaired, Copilot's readiness
/// read after its probe, codex's install not refused.
fn scanned(found: [attention_ownership::HooksFound; 3]) -> agent_hooks_step::HooksSnapshot {
    agent_hooks_step::HooksSnapshot {
        found,
        unrepaired: [None; 3],
        copilot_readiness: attention_copilot::Readiness::Unknown,
        copilot_read_after_probe: true,
        codex_install_refused: false,
        notices: Vec::new(),
    }
}

/// RED (0.4.10 plan row 73, rulings R3 and R6) — **the first-run card opens on the scan's first
/// answer, in one piece, and nothing after changes it**. A first launch with Claude Code on the
/// path: turn after turn the card is not up and `Shown` is not written — no card is drawn
/// without its agent rows, so none can be refilled later; the carry pass's answer is adopted
/// (`adopt_agent_hooks_snapshot`, the adoption `AppEvent::CarryPassAnswered` runs, called here
/// directly so no other test's pass can take it) and the card opens with every row it will ever
/// have, once the install channel's one-turn wait — spent only by a card that is otherwise ready —
/// is over; a later answer — this copy's hooks there now, whose row a fresh card would not list —
/// leaves the open card's rows as they were. On a platform that offers no agent row (a Mac) the
/// card does not wait for a scan at all.
///
/// MUTATIONS: the card goes ahead with a found agent unanswered
/// (`plant-73-card-opens-before-the-answer.py`); a later answer reopens the card with its rows
/// (`plant-73-card-refilled-by-a-later-round.py`).
#[test]
fn the_card_opens_on_the_scans_first_answer() {
    use attention_ownership::HooksFound;
    use first_run::RowKind;
    let mut folio = crate::headless::HeadlessFolio::with_tabs(
        "first-run-scan",
        vec![crate::test_support::tab_holding(
            crate::test_support::leaf_saying("first run"),
        )],
        0,
    );
    let claude = profiles::position_of("claude");
    folio.app.profile_programs =
        profiles::ProfilePrograms::with_only(&claude.into_iter().collect::<Vec<_>>());
    let turn = |folio: &mut crate::headless::HeadlessFolio| {
        folio
            .runtime()
            .turn(Instant::now(), true)
            .expect("the turn is answered");
    };
    // Turn after turn with the scan unanswered; on a Mac, enough for the install channel's wait.
    for _ in 0..4 {
        turn(&mut folio);
    }
    if claude.is_none() || !RowKind::Claude.offered_on(bt_platform::host_platform()) {
        assert!(
            folio.window.first_run.is_open(),
            "a platform that offers no agent row: the card does not wait for the scan"
        );
        return;
    }
    assert!(
        !folio.window.first_run.is_open(),
        "the card came up before the scan answered"
    );
    assert_eq!(
        folio.app.settings_store.loaded().first_run_card,
        bt_persist::FirstRunCardV1::NotShown,
        "a card that is not up is not recorded"
    );
    adopt_agent_hooks_snapshot(
        &mut folio.app,
        &scanned([
            HooksFound::Nothing,
            HooksFound::Nothing,
            HooksFound::Nothing,
        ]),
    );
    // The install channel's one turn, then the card.
    turn(&mut folio);
    turn(&mut folio);
    assert!(
        folio.window.first_run.is_open(),
        "the card did not open on the scan's answer"
    );
    let kinds: Vec<RowKind> = folio
        .window
        .first_run
        .rows()
        .iter()
        .map(|row| row.kind)
        .collect();
    assert_eq!(
        kinds,
        [RowKind::Update, RowKind::Explorer, RowKind::Claude],
        "the first paint holds every row"
    );
    let resume = folio.app.settings_store.loaded().resume_agents;
    assert_eq!(
        folio.window.first_run.rows()[2].on,
        first_run::agent_arrives_on(RowKind::Claude, resume)
    );
    assert_eq!(
        folio.app.settings_store.loaded().first_run_card,
        bt_persist::FirstRunCardV1::Shown
    );
    assert_eq!(
        folio.app.carry_answers_adopted, 1,
        "the card's opening line counts the one hooks answer this window adopted (the review's F1)"
    );
    let opened = folio.window.first_run.rows().to_vec();
    adopt_agent_hooks_snapshot(
        &mut folio.app,
        &scanned([
            HooksFound::Current,
            HooksFound::Nothing,
            HooksFound::Nothing,
        ]),
    );
    for _ in 0..3 {
        turn(&mut folio);
    }
    assert!(folio.window.first_run.is_open());
    assert_eq!(
        folio.window.first_run.rows(),
        &opened[..],
        "a later answer changed the open card"
    );
}
