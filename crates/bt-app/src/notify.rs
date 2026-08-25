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

/// **How far a request about this pane gets** (`attention` plan §10.7's eight-row table).
///
/// Three answers and not two, and the third row of the table is the reason the two-answer version
/// had to go. **What used to stand here** was `reaches_the_desktop`, argued as "the attention
/// ledger's own predicate, read the other way round": exactly when `attention_is_consumed` would
/// say the reader has seen it, the desktop heard nothing. That argument is *correct in a two-tier
/// model and false in a three-tier one* — the middle answer, "on this screen but not in front of
/// your eyes", is neither "they have seen it" nor "the desktop is all that is left". It is a third
/// answer, and a predicate has room for two.
///
/// So the ledger's predicate keeps the job it always had — retiring a latch a look has spent — and
/// this owns the desktop's. `attention_is_consumed` is forbidden a third parameter for the same
/// reason (red line 4): one of them is about the account and the other is about how far away the
/// reader is, and `44f9f2d`'s lesson is that tangling those two says the causality backwards.
///
/// **It answers how far, never whether.** Whether one particular request is entitled to interrupt
/// at all is the ledger's single gate ([`crate::attention::AttentionLedger`], red line 12); this
/// says only how much of the desktop is reachable from here, and it says it identically for both
/// lanes.
///
/// ```text
/// if window_is_hidden        { Toast }    // out of reach; the desktop is what is left
/// else if focused && active  { Nothing }  // you are looking at it
/// else                       { Flash }    // on this screen, but not in front of you
/// ```
///
/// **`window_is_hidden` outranks the focus bits, and the two unreachable rows are the reason it is
/// written outermost.** A three-`bool` function owes all eight inputs an answer, and rows 5 and 6 —
/// a minimised window that holds the keyboard — cannot happen. Answering them from the hidden bit
/// is not defensive: it is what covers the frame after a minimise, before the focus bits have
/// caught up.
///
/// **The second row is honest rather than literal, and the doc has to say so or the branch reads
/// as a mistake.** `FlashWindowEx` on the *foreground* window is a documented no-op, so the
/// visible product of `Flash` for a background tab of the window you are in is the in-window
/// marks — the dot on that tab and the badge in the title bar. It is written `Flash` and not
/// `Nothing` because the sentence it answers is "on this screen, but not in front of you", which
/// is word for word rows three and four; the instant you look away the same arm becomes a real
/// flash and **not one character of the predicate changes**. Writing it `Nothing` would make "a
/// background tab of the focused window" and "the pane you are staring at" indistinguishable here,
/// which is the one distinction the row exists to draw.
#[must_use]
pub fn desktop_reach(
    tab_is_active: bool,
    window_is_focused: bool,
    window_is_hidden: bool,
) -> crate::attention::Reach {
    use crate::attention::Reach;
    if window_is_hidden {
        Reach::Toast
    } else if window_is_focused && tab_is_active {
        Reach::Nothing
    } else {
        Reach::Flash
    }
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
    use super::{NotificationRoute, desktop_reach, toast_title};
    use crate::attention::Reach;

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

    /// PIN (§7.6, `attention` plan §10.7) — **all eight inputs, and the three answers they fall
    /// into.**
    ///
    /// The table is written out row by row rather than folded, because it *is* the specification:
    /// eight rows of three booleans, and the whole behavioural claim of the desktop half of this
    /// block is which of the three each one lands in.
    ///
    /// The two rows nobody can reach are here for the reason a total function has them at all: a
    /// minimised window does not hold the keyboard, so rows five and six describe no moment a
    /// reader can be in — except the frame *just after* a minimise, when the focus bits have not
    /// caught up yet, and that frame is precisely why the hidden bit is tested outermost.
    ///
    /// MUTATIONS: test the focus pair before the hidden bit and rows five and six answer
    /// `Nothing`/`Flash` — a window that is not on any screen reported as one the reader can see.
    /// Answer `Nothing` for row two and the background tab of a focused window becomes
    /// indistinguishable from the pane under the reader's eyes. Answer `Toast` for rows three and
    /// four and every notification arriving while the reader is in another application becomes a
    /// desktop interruption about a window they can see.
    #[test]
    fn every_one_of_the_eight_positions_a_reader_can_be_in_has_an_answer() {
        // (window_is_focused, tab_is_active, window_is_hidden) → Reach, in the plan's own order.
        let table = [
            ((true, true, false), Reach::Nothing),
            ((true, false, false), Reach::Flash),
            ((false, true, false), Reach::Flash),
            ((false, false, false), Reach::Flash),
            ((true, true, true), Reach::Toast),
            ((true, false, true), Reach::Toast),
            ((false, true, true), Reach::Toast),
            ((false, false, true), Reach::Toast),
        ];
        for ((focused, active, hidden), expected) in table {
            assert_eq!(
                desktop_reach(active, focused, hidden),
                expected,
                "focused={focused} active={active} hidden={hidden}"
            );
        }
        assert_eq!(table.len(), 8, "a three-bool function owes eight answers");
    }

    /// PIN (`attention` plan §10.7, red line 4) — **the ledger's predicate and the desktop's
    /// answer are two questions, and the second row is where they part.**
    ///
    /// `attention_is_consumed` says the reader has seen it; `desktop_reach` says how far away the
    /// reader is. In the two-tier model the second was written as the negation of the first, and
    /// this is the row that made that false: a background tab of the focused window is *not*
    /// consumed — the ledger admits it to the queue — and it is *not* reachable by a toast either.
    /// The old predicate had one answer for those two facts.
    #[test]
    fn a_background_tab_of_the_focused_window_is_neither_seen_nor_worth_a_toast() {
        assert!(
            !crate::attention_is_consumed(false, true),
            "the ledger admits it: nobody is looking at that tab"
        );
        assert_eq!(desktop_reach(false, true, false), Reach::Flash);
        assert_eq!(
            desktop_reach(true, true, false),
            Reach::Nothing,
            "and the pane that *is* consumed is the one that adds nothing"
        );
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
