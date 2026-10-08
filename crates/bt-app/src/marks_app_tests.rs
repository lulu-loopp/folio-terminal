//! **`marks`, as the application drives it.** Tests whose first assertion is about
//! `marks`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use std::time::Duration;

/// PIN — U8, P177. The veil is the bottom-most thing the overlay carries.
///
/// The dock drawing is already documented as the lowest layer in the stack
/// ([`Runtime::dock_overlay_layers`]), so "under the dock" is "under
/// everything": the menus, the restore prompt, the settings dialog, the
/// layout peek, the tip and the drag ghost are all stacked after it by
/// [`Runtime::refresh_overlay`]. What this fails is the reasonable-looking
/// mistake of appending the veil to the stack that is already built, which
/// would put a pane's fade over an open dialog.
#[test]
fn the_arriving_panes_veil_is_the_bottom_most_overlay_layer() {
    let veil = marks::OverlayLayer {
        quads: vec![bt_render::OverlayQuad {
            rect: [0.0, 0.0, 10.0, 10.0],
            color: bt_render::chrome_palette().seat_body,
            alpha: 0.5,
        }],
        ..marks::OverlayLayer::default()
    };
    let dock = vec![marks::OverlayLayer {
        quads: vec![bt_render::OverlayQuad {
            rect: [20.0, 20.0, 30.0, 30.0],
            color: bt_render::chrome_palette().title_bar,
            alpha: 1.0,
        }],
        ..marks::OverlayLayer::default()
    }];

    let stacked = ground_overlay_layers(vec![veil.clone()], dock.clone().into());
    assert_eq!(
        stacked.layers.first(),
        Some(&veil),
        "the veil paints first and is therefore covered by everything after it"
    );
    assert_eq!(
        &stacked.layers[1..],
        dock.as_slice(),
        "and the dock is over it"
    );
    assert_eq!(
        ground_overlay_layers(Vec::new(), dock.clone().into()),
        marks::Band::from(dock),
        "with nothing arriving the stack is the one that was there before P177"
    );
}

/// RED (46) — **under reduced motion no fade offers the renderer its group
/// path**: every surface a fade would have drawn apart stands at rest, and a
/// band that passes through the arrival register carries no span at all.
///
/// The fade audit's gate (c), the producers' half; the renderer's half
/// (`bt-render` `tests::overlay_groups::a_group_at_rest_never_takes_the_group_path`)
/// holds that a span at rest is drawn straight onto the frame. Asked of the
/// fades' own doors at the first instant of each fade: the arrival register
/// (menus, the palette, the settings dialog, the notice strip, the Cards
/// bubble), the hover fade the tip and the glance card both read, and a notice
/// card's opacity and slide. The tear-out ghost's 0.7 is a standing
/// translucency, not motion, so reduced motion (which is about motion) does not
/// forbid the group path there: it is a group in every motion mode, by the
/// coordinator's ruling of 2026-09-24, and not a hole in this pin.
///
/// MUTATION: sample the curve under `Reduced` in `hover_fade_opacity` (or
/// keep an entry in `Passages::stage` under `Reduced`) and a span below 1 is
/// handed over.
#[test]
fn under_reduced_motion_no_fade_offers_the_group_path() {
    let now = Instant::now();
    let layer = || marks::OverlayLayer {
        quads: vec![bt_render::OverlayQuad {
            rect: [10.0, 10.0, 110.0, 50.0],
            color: [40, 40, 40],
            alpha: 1.0,
        }],
        ..marks::OverlayLayer::default()
    };

    let mut passages = arrival::Passages::<u8>::default();
    let menu = passages.stage(
        1,
        vec![layer()].into(),
        Some(Travel::Down),
        now,
        Motion::Reduced,
        2.0,
    );
    assert!(
        menu.groups.is_empty(),
        "a band arriving under reduced motion is no surface in passage"
    );

    let tip = tooltip::hover_fade_opacity(Duration::ZERO, Motion::Reduced);
    let laid = tooltip::layout(
        "bash",
        [400.0, 10.0, 460.0, 40.0],
        &[40.0],
        (1000.0, 700.0),
        1.0,
        tooltip::TipFace::Chrome,
    )
    .expect("a tip is placed");
    let band = tooltip::build(
        &laid,
        &bt_render::chrome_palette(),
        1.0,
        tip,
        tooltip::TipFace::Chrome,
    );
    assert!(
        band.groups.iter().all(bt_render::OverlayGroup::at_rest),
        "the tip on its first frame under reduced motion: {:?}",
        band.groups
    );

    let mut host = toast::ToastHost::default();
    host.raise(
        toast::ToastKind::Ok,
        toast::ToastAnchor::Window,
        None,
        "done",
        None,
        Motion::Reduced,
        now,
    );
    let laid = toast::place(
        host.toasts(),
        |_| None,
        (1000.0, 700.0),
        1.0,
        &mut |run, _| run.chars().count() as f32 * 8.0,
    );
    let cards = toast::build(
        &laid,
        &host,
        toast::ToastPointer::default(),
        &bt_render::chrome_palette(),
        1.0,
        now,
        Motion::Reduced,
    );
    assert!(!cards.is_empty(), "the card is drawn on its first frame");
    assert!(
        cards.groups.iter().all(bt_render::OverlayGroup::at_rest),
        "a notice card on its first frame under reduced motion: {:?}",
        cards.groups
    );
}
