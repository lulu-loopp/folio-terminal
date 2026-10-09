//! **The crate root: profiles.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::saved_tab;

/// §5.4 逐叶降级, "未知 profile→默认": a profile this build does not have
/// costs you the shell choice, never the tab.
#[test]
fn a_seed_naming_a_profile_we_do_not_have_still_comes_back() {
    let tab = saved_tab("wsl-ubuntu", "C:\\a", Some("notes"), true);
    let (seats, seed, leaves, _files, _preview) = revive_plan(&tab);
    assert_eq!(
        leaves
            .get(&seats.identity())
            .map(|leaf| leaf.profile.as_str()),
        Some(profiles::fallback_profile_id()),
        "an id this build cannot place falls to the default profile"
    );
    assert_eq!(
        seed.manual_name.as_deref(),
        Some("notes"),
        "your name stays"
    );
    assert!(seed.pinned, "and so does the promise");
}
