//! **`webhost`, as the application drives it.** Tests whose first assertion is about
//! `webhost`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;

/// **The switcher's identity is the recovery machine's field, and there is no
/// second account of it** (`plan.md` §3 与 §4).
///
/// "The URL a session file may record" and "what the switcher calls this
/// seat" are one sentence, so they are one field. This drives the machine
/// through the three things that must *not* move it — a navigation in flight,
/// a failure page, an `about:blank` — and the one that must, and reads the
/// switcher's answer off the machine each time.
///
/// Red gate: let `WebMachine::on_navigation_completed` write its
/// `recoverable_url` whatever `success` said, and the two assertions about a
/// page that never loaded both move.
#[test]
fn the_switchers_identity_is_the_machines_last_committed_url() {
    let mut machine = webhost::WebMachine::new();
    machine.request("http://LocalHost:5173/app?tab=logs#top");
    let generation = machine.generation();
    machine.on_environment(generation, true);
    machine.on_controller(generation, true);
    machine.on_events_installed(generation);
    assert_eq!(
        webnav::switcher_identity(machine.recoverable_url()),
        None,
        "a navigation that has not committed has no identity — a row for a \
             page that never existed is a row the switcher cannot honour"
    );
    machine.on_navigation_completed(generation, "http://localhost:5173/app?tab=logs#top", true);
    assert_eq!(
        webnav::switcher_identity(machine.recoverable_url()),
        Some("http://localhost:5173/app?tab=logs#top".to_owned()),
        "query and fragment participate, and only a default port is dropped"
    );
    machine.request("http://localhost:5173/gone");
    assert_eq!(
        webnav::switcher_identity(machine.recoverable_url()),
        Some("http://localhost:5173/app?tab=logs#top".to_owned()),
        "asking for a page is not being on it"
    );
    machine.on_navigation_completed(generation, "http://localhost:5173/gone", false);
    machine.on_navigation_completed(generation, "about:blank", true);
    assert_eq!(
        webnav::switcher_identity(machine.recoverable_url()),
        Some("http://localhost:5173/app?tab=logs#top".to_owned()),
        "a failure page and a blank page are neither of them where you are"
    );
    // A redirect: what committed is the identity, not what was asked for.
    machine.request("http://localhost:5173/old");
    machine.on_navigation_completed(generation, "http://localhost:5173/new", true);
    assert_eq!(
        webnav::switcher_identity(machine.recoverable_url()),
        Some("http://localhost:5173/new".to_owned()),
        "after a redirect the seat is where it landed"
    );
}

/// PIN (§7.7 ⑨) — **"never been anywhere" is the recovery machine's own
/// state and not a second account kept beside it.**
///
/// The withdrawal has to be able to tell a page the person walked away from
/// apart from a page they typed an address into, and the difference is
/// already a field: `recoverable_url` is written by a successful navigation
/// and the line that writes it names `about:blank` in order to refuse it. So
/// a blank page's identity stays `None` however many times the engine says it
/// arrived, and the first real address ends the door.
///
/// MUTATION: let `on_navigation_completed` write `about:blank` through — the
/// first assertion goes red, and every `Ctrl+Shift+L` becomes unwithdrawable
/// the moment its blank page finishes loading.
#[test]
fn a_blank_page_is_a_page_that_has_never_been_anywhere() {
    let mut machine = webhost::WebMachine::new();
    machine.request(webnav::BLANK_PAGE);
    let generation = machine.generation();
    machine.on_environment(generation, true);
    machine.on_controller(generation, true);
    machine.on_events_installed(generation);
    machine.on_navigation_completed(generation, webnav::BLANK_PAGE, true);
    assert_eq!(
        machine.recoverable_url(),
        None,
        "the host's own scaffolding is not somewhere a person went"
    );
    machine.on_navigation_completed(generation, "http://localhost:5173/app", true);
    assert_eq!(
        machine.recoverable_url(),
        Some("http://localhost:5173/app"),
        "and the first address that is one ends the door"
    );
}

/// **A hole is only ever cut where a floor already stands** (§7.14; user
/// ruling 2026-08-25, from a photograph of a first-opened page showing the
/// desktop).
///
/// The hole and the floor were on two different clocks and only one of them
/// was the pane's. A seat's rectangle exists the moment its pane does, so
/// `sync_web_page` cut the hole on the first frame; the floor was minted by
/// `attach_web_visual`, which runs when WebView2 hands back a controller —
/// hundreds of milliseconds later on a first open, and never at all if the
/// engine fails to arrive. In between, the pane is a rectangle nothing in
/// the composition tree paints, over a `topmost = true` target on a
/// per-pixel-alpha HWND: the desktop, at full size, for as long as it takes.
///
/// **Measured on the machine before the fix** (release, cold profile,
/// isolated `APPDATA`/`LOCALAPPDATA`, a magenta board window behind Folio):
/// twelve presented frames carried the hole with `engine_up=false`, spanning
/// 403 ms, and the camera caught the board filling the whole pane body from
/// 88.5 ms to 514.6 ms — 120 056 sampled pixels, the pane's body exactly.
/// Two runs out of two. After the fix, two runs out of two: not one board
/// pixel inside the frame, peak zero.
///
/// The fix is that the placement now answers the hole — `WebSeat::place`
/// returns whether a floor stands — and this is the sentence that reads that
/// answer. It is pure, so unlike the pane it decides for it can be held
/// here rather than described.
///
/// MUTATIONS:
/// ① make [`super::hole_for`] ignore `floored` — the first assertion goes
///    red, and that is exactly the shipped build the user photographed;
/// ② let it cut a hole for a `Hidden` page — the second goes red, and a page
///    behind a modal or on a background tab becomes a window-shaped window.
#[test]
fn a_hole_is_only_cut_where_a_floor_already_stands() {
    let bounds = super::webhost::WebBounds {
        x: 12,
        y: 34,
        width: 500,
        height: 400,
    };
    let shown = super::webhost::WebPresence::Shown(bounds);

    assert_eq!(
        super::hole_for(shown, false, None),
        None,
        "a page whose floor is not down yet is still given a hole, which is \
             a rectangle of desktop inside this window"
    );
    assert_eq!(
        super::hole_for(shown, true, None).map(|hole| hole.rect),
        Some(bounds.as_rect()),
        "a page standing on its own floor is given no hole, so the page \
             nobody can see is hosted perfectly"
    );
    assert_eq!(
        super::hole_for(super::webhost::WebPresence::Hidden, true, None),
        None,
        "a hidden page is cut a hole anyway"
    );
}
