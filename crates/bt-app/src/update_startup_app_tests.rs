//! **`update_startup`, as the application drives it.** Tests whose first assertion is about
//! `update_startup`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;

/// A cap that bites by a hair is a half-second spent making the picture worse
/// (user ruling 2026-08-25; [`PREVIEW_IMAGE_TEXTURE_SLACK`]).
#[test]
fn a_decode_inside_the_slack_keeps_its_own_pixels_rather_than_paying_for_a_pass() {
    let allowance =
        bt_viewport::MATH_TEXTURE_CACHE_BUDGET_BYTES as f64 * PREVIEW_IMAGE_TEXTURE_SHARE;
    let bytes = |image: [u32; 2]| f64::from(image[0]) * f64::from(image[1]) * 4.0;

    // 33.2 MiB against a 32 MiB share: over it, and inside the slack.
    let barely = [3000_u32, 2900];
    assert!(bytes(barely) > allowance, "the share really is crossed");
    assert!(
        bytes(barely) <= allowance * PREVIEW_IMAGE_TEXTURE_SLACK,
        "and crossed by less than the slack"
    );
    assert_eq!(
        image_raster_cap(barely),
        (barely[0], barely[1]),
        "so 100% is the file's own pixels and no pass is run at all"
    );

    // 39.1 MiB: past the slack, so the pass is worth what it costs.
    let past = [3200_u32, 3200];
    assert!(
        bytes(past) > allowance * PREVIEW_IMAGE_TEXTURE_SLACK,
        "this one is past the slack"
    );
    let capped = image_raster_cap(past);
    assert_ne!(capped, (past[0], past[1]), "and is capped");
    assert!(
        f64::from(capped.0) * f64::from(capped.1) * 4.0 <= allowance,
        "down to the share itself and not merely to the slack — once the pass \
             is paid for there is no reason to stop short of it"
    );

    // The slack admits at most 0.6 of the whole budget, which is the number
    // `ByteLru` would refuse a texture over.
    assert!(
        allowance * PREVIEW_IMAGE_TEXTURE_SLACK
            < bt_viewport::MATH_TEXTURE_CACHE_BUDGET_BYTES as f64,
        "nothing the slack admits can be refused outright by the shared cache"
    );
}
