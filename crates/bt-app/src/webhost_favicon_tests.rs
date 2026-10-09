//! `webhost`'s favicon tests: the seat's bookkeeping of one site's icon. Declared from
//! `webhost.rs` as `webhost::favicon_tests`; the Windows-only cases (WebView2 fetches the
//! icon) and their macOS twin name their platform here, so the product file names none.

use super::rehost_address_tests::{detached, page, window};
use super::*;

/// A detached seat standing on one address, as if a navigation had
/// committed.
fn seat_on(url: &str) -> WebSeat {
    let mut web = detached(SeatAddress {
        page: page(1, 1),
        window: window(0x40),
    });
    web.page.url = url.to_owned();
    web
}

fn digest(web: &mut WebSeat, event: bt_platform::WebEvent) -> Vec<WebOutcome> {
    let mut outcomes = Vec::new();
    web.digest(&event, &mut outcomes);
    outcomes
}

/// **Red gate: an announcement is an ask, and the answer is filed under the
/// site that was asked about.**
///
/// The whole engine half in one run. `FaviconChanged` carries an address and
/// no bytes, so the seat has to ask; and the answer arrives tens of
/// milliseconds later, so what it is *about* has to have been written down
/// at the moment of asking.
///
/// MUTATION: file the answer under `webnav::site_key(&self.page.url)` read at
/// delivery instead of the recorded site, and the last assertion says
/// `https://second.test` — one server's drawing under another's name.
///
/// Windows only, like the next one: the ask is WebView2's `GetFavicon`. A
/// `WKWebView` names no icon to its host, so on macOS the ask is refused
/// and nothing is in flight —
/// `an_engine_that_names_no_icon_leaves_nothing_in_flight`.
#[cfg(windows)]
#[test]
fn an_answer_is_filed_under_the_site_that_was_asked_about() {
    let mut web = seat_on("https://first.test/a");
    let asked = digest(
        &mut web,
        bt_platform::WebEvent::FaviconChanged {
            uri: "https://first.test/favicon.ico".to_owned(),
        },
    );
    assert!(
        asked.is_empty(),
        "an announcement reports nothing by itself"
    );
    assert_eq!(web.fetching_favicon.as_deref(), Some("https://first.test"));

    // The page moves on while the engine is still fetching.
    web.page.url = "https://second.test/b".to_owned();
    let answered = digest(
        &mut web,
        bt_platform::WebEvent::Favicon {
            png: Some(vec![1, 2, 3]),
        },
    );
    assert_eq!(
        answered,
        vec![WebOutcome::Favicon {
            site: "https://first.test".to_owned(),
            png: Some(vec![1, 2, 3]),
        }]
    );
    assert_eq!(web.fetching_favicon, None, "the flight is over");
}

/// **Red gate: a page with no icon says so, and says it about its own
/// site.**
///
/// §7.7 ②'s second half has to be *reported*, not merely not-reported: a
/// page navigating from a site that had an icon to one that has none fires
/// this with an empty address, and a seat that swallowed it would leave the
/// first server's drawing standing on the head.
///
/// MUTATION: return early on an empty `uri` and the outcome list is empty —
/// the store keeps an icon for a site that just said it has none.
#[test]
fn a_page_with_no_icon_says_so() {
    let mut web = seat_on("https://first.test/a");
    assert_eq!(
        digest(
            &mut web,
            bt_platform::WebEvent::FaviconChanged { uri: String::new() },
        ),
        vec![WebOutcome::Favicon {
            site: "https://first.test".to_owned(),
            png: None,
        }]
    );
    assert_eq!(web.fetching_favicon, None, "and nothing was asked for");
}

/// **Red gate: one ask at a time, and a change during a flight costs one
/// more ask and not one per announcement.**
///
/// The engine re-reads the icon resource on every ask, so an unguarded seat
/// would fetch a file once per announcement — and a shell that paints a
/// placeholder and then the real icon announces twice inside one
/// navigation.
///
/// MUTATION: drop the `fetching_favicon.is_some()` guard and the second
/// announcement overwrites the first flight's site, so the first answer is
/// filed under the wrong name. MUTATION: drop `favicon_changed_again` and
/// the placeholder stays up for good.
///
/// Windows only: a flight is a `GetFavicon` in progress, which only WebView2
/// makes.
#[cfg(windows)]
#[test]
fn a_second_announcement_during_a_flight_is_one_more_ask_and_not_two() {
    let mut web = seat_on("https://first.test/a");
    digest(
        &mut web,
        bt_platform::WebEvent::FaviconChanged {
            uri: "https://first.test/one.png".to_owned(),
        },
    );
    for _ in 0..5 {
        digest(
            &mut web,
            bt_platform::WebEvent::FaviconChanged {
                uri: "https://first.test/two.png".to_owned(),
            },
        );
    }
    assert_eq!(
        web.fetching_favicon.as_deref(),
        Some("https://first.test"),
        "still the one flight"
    );
    assert!(web.favicon_changed_again, "and one ask is owed");

    digest(
        &mut web,
        bt_platform::WebEvent::Favicon { png: Some(vec![9]) },
    );
    assert_eq!(
        web.fetching_favicon.as_deref(),
        Some("https://first.test"),
        "the owed ask went out on the answer's heels"
    );
    assert!(
        !web.favicon_changed_again,
        "and it is owed once, however many times it was announced"
    );
}

/// **Red gate: an engine that names no icon leaves nothing in flight, and
/// says nothing.**
///
/// The macOS twin of the two above. A `WKWebView` does not tell its host what
/// icon a page wears, so the host refuses the ask — and a refused ask is
/// rule ④'s: silent, and recorded nowhere, so the site goes on wearing the
/// globe and no later announcement waits behind a flight that never left.
///
/// MUTATION: record the site before asking in `ask_for_the_favicon` and
/// `fetching_favicon` is `Some` for ever, with every later announcement
/// queued behind it.
#[cfg(not(windows))]
#[test]
fn an_engine_that_names_no_icon_leaves_nothing_in_flight() {
    let mut web = seat_on("https://first.test/a");
    for uri in ["https://first.test/one.png", "https://first.test/two.png"] {
        assert!(
            digest(
                &mut web,
                bt_platform::WebEvent::FaviconChanged {
                    uri: uri.to_owned(),
                },
            )
            .is_empty(),
            "a refused ask is not reported"
        );
        assert_eq!(web.fetching_favicon, None, "and nothing is in flight");
        assert!(!web.favicon_changed_again, "so nothing is owed either");
    }
}

/// **Red gate: a seat that cannot name its own site asks for nothing.**
///
/// A page that has committed nothing has an empty address, and the store is
/// keyed by site — so there would be nowhere to file the answer. Asking
/// anyway would be spending a fetch on a picture with no name.
///
/// MUTATION: ask regardless and `fetching_favicon` is `Some("")`, which is
/// an entry the store would file every unnamed page's icon into.
#[test]
fn a_seat_with_no_address_asks_for_nothing() {
    let mut web = seat_on("");
    assert!(
        digest(
            &mut web,
            bt_platform::WebEvent::FaviconChanged {
                uri: "https://first.test/one.png".to_owned(),
            },
        )
        .is_empty()
    );
    assert_eq!(web.fetching_favicon, None);
}

/// **An answer nobody asked for is dropped.**
///
/// The seat's own version of `web_thumb`'s `page-stale`: a `Favicon` event
/// arriving with no flight recorded — a rebuilt engine answering for the one
/// before it — has no site to be about, and inventing one out of wherever
/// the seat happens to be now is the bug the recorded site exists to
/// prevent.
#[test]
fn an_answer_with_no_flight_behind_it_is_dropped() {
    let mut web = seat_on("https://first.test/a");
    assert!(
        digest(
            &mut web,
            bt_platform::WebEvent::Favicon { png: Some(vec![1]) },
        )
        .is_empty()
    );
}
