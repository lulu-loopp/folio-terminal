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
