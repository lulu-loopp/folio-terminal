//! Desktop notifications — the policy half (DESIGN §7.6, Windows landing slice 3).
//!
//! `bt-term` decides what a program *said* (`OSC 9`, `OSC 777;notify`) and `bt_platform::Notifier`
//! carries a finished toast to Windows. Everything between those two is here, and all of it is a
//! pure function of facts the caller already has: whether a request reaches the desktop at all,
//! what name the toast wears, and how a click finds its way back to the pane that sent it.
//!
//! Nothing in this module holds state, and that is the point of it existing beside `main.rs`
//! rather than inside: the three decisions below are the whole of what could be got wrong, and
//! each of them is a table a test can walk.

/// Where a clicked notification puts the user.
///
/// Three ids and no lookup table, which is the ruling worth stating: the route travels **in the
/// toast**, as the `launch` string Windows hands back verbatim, rather than in a map on this side
/// keyed by a notification id. A map would have to be bounded (a person can click a toast from
/// half an hour ago), and its bound would silently become "how far back a notification still
/// works". Ids in the string have no such horizon, and every one of them is already a number this
/// process assigns.
///
/// A route naming something that is gone — a closed window, a closed tab, a pane that was split
/// away — resolves to nothing and the click does nothing. That is the honest answer and not a
/// failure: the thing the notification pointed at is not there.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NotificationRoute {
    /// `winit::window::WindowId` as its `u64`, which on Windows is the `HWND`.
    pub window: u64,
    /// `TabId`'s number. Unique within its window and not across them, which is why the window is
    /// carried too and why the two are always read together.
    pub tab: u64,
    /// `SeatId`'s number — the pane inside that tab.
    pub seat: u64,
}

impl NotificationRoute {
    /// The `launch` string this route travels as.
    ///
    /// A query string because that is the shape everything in the notification ecosystem uses for
    /// this field (the platform's own samples, `ToastActivatedEventArgs::Arguments`), and because
    /// it survives being read by a person looking at a toast's XML. The `&` is what makes
    /// `bt_platform::toast_xml`'s escaping load-bearing rather than decorative.
    #[must_use]
    pub fn launch(self) -> String {
        format!("w={}&t={}&s={}", self.window, self.tab, self.seat)
    }

    /// The seat this route names, as the layout's own id.
    ///
    /// A method rather than a `SeatId(route.seat)` at the call site so that the one place a wire
    /// number becomes a layout identity is written down: `bt_layout::SeatId` is a tuple struct
    /// with a public field, and "construct it from a `u64` that came out of a toast" is a
    /// sentence that should exist exactly once.
    #[must_use]
    pub fn seat_id(self) -> bt_layout::SeatId {
        bt_layout::SeatId(self.seat)
    }

    /// Read one back, or refuse.
    ///
    /// Strict on every count — all three fields present, each parsing as a `u64`, no unknown
    /// keys tolerated silently by filling a default — because the only thing this string can be
    /// is one this build wrote. Anything else is a toast from another version or another program
    /// under the same identity, and half-reading it would send the click to a pane picked by
    /// whichever field happened to parse.
    #[must_use]
    pub fn parse(launch: &str) -> Option<Self> {
        let mut window = None;
        let mut tab = None;
        let mut seat = None;
        for field in launch.split('&') {
            let (key, value) = field.split_once('=')?;
            let value = value.parse::<u64>().ok()?;
            let slot = match key {
                "w" => &mut window,
                "t" => &mut tab,
                "s" => &mut seat,
                _ => return None,
            };
            if slot.replace(value).is_some() {
                return None;
            }
        }
        Some(Self {
            window: window?,
            tab: tab?,
            seat: seat?,
        })
    }
}

/// Whether one request reaches the desktop.
///
/// **The gate is the attention ledger's own predicate, read the other way round.**
/// `attention_is_consumed(tab_is_active, window_is_focused)` is what already decides that a bell
/// which rings on the tab you are looking at, in the window that has the keyboard, has been
/// answered by the looking. A notification is the same sentence from the other side: exactly when
/// the ledger would say "they have seen it", the desktop hears nothing.
///
/// Written as one call rather than as a copy of the condition, because a second spelling of it is
/// the way the two drift — and if they drifted, the failure would be a toast for a pane the user
/// is staring at, which is the single most annoying thing a terminal can do.
///
/// The setting is the outer half and it is a *silence*, not a concealment: with it off nothing is
/// raised and everything else — the unread dot, the bell latch, the pane's own account of itself
/// — goes on exactly as before. A notification adds no state, so there is none to suppress.
#[must_use]
pub fn reaches_the_desktop(enabled: bool, tab_is_active: bool, window_is_focused: bool) -> bool {
    enabled && !crate::attention_is_consumed(tab_is_active, window_is_focused)
}

/// The name one toast wears.
///
/// Three layers, and the order is a protocol fact rather than a preference. `OSC 777;notify`
/// carries a title field and a program that filled it in has named its own message; `OSC 9` has
/// no such field at all, so its notification is unnamed and the pane it came from is the truest
/// thing left to call it. The profile's name is the floor, for exactly `terminal_name`'s reason:
/// a shell that has announced nothing has no name of its own, and `Windows PowerShell` is what
/// the window would call that pane anywhere else.
///
/// A carried title that is only whitespace falls through as though it were absent. `bt-term`
/// already refuses an *empty* one; this is the layer beneath that, and it is the same rule
/// `title_layer` applies to every other program-supplied name in this window.
#[must_use]
pub fn toast_title(carried: Option<&str>, pane_name: Option<&str>, profile_name: &str) -> String {
    carried
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .or_else(|| pane_name.map(str::trim).filter(|name| !name.is_empty()))
        .unwrap_or(profile_name)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::{NotificationRoute, reaches_the_desktop, toast_title};

    /// PIN (§7.6) — **a route survives the round trip and nothing else is read as one.**
    ///
    /// The string leaves this process, is stored by Windows, and comes back through a COM
    /// boundary possibly minutes later. The failure this pins is the tolerant parser: one that
    /// filled a missing field with a zero would route every malformed toast to window 0, tab 0,
    /// seat 0 — a real pane in most windows.
    ///
    /// MUTATIONS: default any absent field and the four refusals below go green; split on `=`
    /// from the right and a value containing `=` would silently parse.
    #[test]
    fn a_route_round_trips_and_a_string_this_build_did_not_write_is_refused() {
        for route in [
            NotificationRoute {
                window: 0,
                tab: 0,
                seat: 0,
            },
            NotificationRoute {
                window: 140_732_705_144_832,
                tab: 7,
                seat: 3,
            },
            NotificationRoute {
                window: u64::MAX,
                tab: u64::MAX,
                seat: u64::MAX,
            },
        ] {
            assert_eq!(NotificationRoute::parse(&route.launch()), Some(route));
        }

        for refused in [
            "",
            "w=1&t=2",
            "w=1&t=2&s=3&x=4",
            "w=1&t=2&s=-3",
            "w=1&t=2&s=",
            "w=1&t=2&s=3&s=4",
            "action=focusTab",
        ] {
            assert_eq!(
                NotificationRoute::parse(refused),
                None,
                "accepted {refused:?}"
            );
        }
    }

    /// PIN (§7.6) — **nothing reaches the desktop that the ledger would call already seen.**
    ///
    /// The whole behavioural claim of the block, as a truth table. The two middle rows are the
    /// ones that matter: a pane in a background window and a background tab in the focused
    /// window are both cases where the person cannot see what was printed, and both notify.
    ///
    /// MUTATIONS: gate on `tab_is_active` alone and row two goes red — which is the bug where
    /// every notification that arrives while the user is in another application is swallowed,
    /// the one moment the feature exists for. Gate on `window_is_focused` alone and row three
    /// goes red. Drop the setting and row one goes red in all four positions.
    #[test]
    fn only_a_pane_nobody_is_looking_at_reaches_the_desktop() {
        for tab_is_active in [false, true] {
            for window_is_focused in [false, true] {
                assert!(
                    !reaches_the_desktop(false, tab_is_active, window_is_focused),
                    "off is silent at {tab_is_active}/{window_is_focused}"
                );
            }
        }
        assert!(
            !reaches_the_desktop(true, true, true),
            "the pane in front of the reader interrupts nobody"
        );
        assert!(
            reaches_the_desktop(true, true, false),
            "the window is behind another application"
        );
        assert!(
            reaches_the_desktop(true, false, true),
            "the tab is not the one on screen"
        );
        assert!(reaches_the_desktop(true, false, false));
    }

    /// PIN (§7.6) — **the title falls through the three layers in protocol order.**
    ///
    /// `OSC 777` names its own message; `OSC 9` cannot, so the pane names it; a pane that has
    /// announced nothing is named by its profile. The whitespace case is the one that would ship
    /// broken: `\e]777;notify; ;body` is a title field that is *present* and says nothing, and a
    /// toast headed by a single space is a toast with no name at all.
    #[test]
    fn a_toast_takes_the_first_name_anybody_actually_gave_it() {
        assert_eq!(
            toast_title(Some("cargo"), Some("BetterTerminal"), "PowerShell"),
            "cargo"
        );
        assert_eq!(
            toast_title(None, Some("BetterTerminal"), "PowerShell"),
            "BetterTerminal"
        );
        assert_eq!(toast_title(None, None, "PowerShell"), "PowerShell");
        assert_eq!(
            toast_title(Some("   "), Some("BetterTerminal"), "PowerShell"),
            "BetterTerminal"
        );
        assert_eq!(
            toast_title(Some("  "), Some(" "), "PowerShell"),
            "PowerShell"
        );
    }
}
