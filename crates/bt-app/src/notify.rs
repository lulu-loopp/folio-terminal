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

/// **Where this window is, as far as the reader's eyes are concerned** (`attention` plan §5.2;
/// user rulings 2026-08-28 and 2026-09-01).
///
/// The four facts [`desktop_reach`] reads, and they travel together because every caller of it
/// carries all four and none of them means anything on its own: "not focused" is a different
/// sentence on a window that is minimised, "minimised" is a different sentence on a desktop whose
/// taskbar is not there, and "not focused" is a different sentence again on a window the reader can
/// see from where they are sitting. The pane's own fact — which tab is on screen — stays a separate
/// argument, because that one changes from leaf to leaf inside a single pass while these four are
/// sampled once for the whole window.
///
/// This is not the argument-list struct the passes below argue against: those lists are seven
/// unrelated questions about one turn, and a name in front of them would move the list rather than
/// shorten it. This is one question — *where is this window* — that happens to take four bits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowPlace {
    /// This window holds the desktop's keyboard.
    pub focused: bool,
    /// Minimised, or on a virtual desktop the reader has switched away from — `bt_app`'s
    /// `window_is_hidden`, which is `IsIconic` or `DWMWA_CLOAKED`.
    pub hidden: bool,
    /// **Some part of this window is actually under the reader's eyes** — `bt_app`'s
    /// `window_is_exposed`, which is [`bt_platform::window_is_exposed`] (user ruling 2026-09-01).
    ///
    /// The fact that separates a window standing in plain sight on a second monitor from one buried
    /// under a full-screen editor. Both of them are unfocused, both of them are on a screen, and
    /// until this bit existed the two were one row of the table — which is the defect the ruling is
    /// about: a reader whose taskbar hides itself was told on the desktop about a window they could
    /// see the whole time.
    ///
    /// **Asked of the window manager, not computed here.** Three `WindowFromPoint` hit tests
    /// resolved to `GA_ROOT`, and the only party entitled to answer "whose glass is under this
    /// point" answers them. Nothing on this side walks a z-order or differences a region.
    ///
    /// **Charged per delivery decision and not per frame.** It rides in `sample_window_place`
    /// beside `IsIconic`, `DWMWA_CLOAKED` and `SHAppBarMessage`, which is the one pass that decides
    /// what the reader is owed; the drawing path never asks.
    ///
    /// **A window that is [`Self::hidden`] is not exposed and is not probed.** `GetWindowRect` on a
    /// minimised window describes the icon Windows parked off-screen rather than the window (see
    /// [`bt_platform::is_window_minimized`]), so a hit test against it would be an answer about the
    /// wrong rectangle. The two bits are still independent inputs here, and [`desktop_reach`] owes
    /// every combination of them an answer.
    pub exposed: bool,
    /// The shell's taskbar is set to hide itself
    /// ([`bt_platform::taskbar_is_auto_hidden`]).
    pub taskbar_is_auto_hidden: bool,
}

/// **How far a request about this pane gets** (`attention` plan §10.7's table).
///
/// Four answers and not two, and each one past the second was bought by a fact the table did not
/// used to have. **What used to stand here** was `reaches_the_desktop`, argued as "the attention
/// ledger's own predicate, read the other way round": exactly when `attention_is_consumed` would
/// say the reader has seen it, the desktop heard nothing. That argument is *correct in a two-tier
/// model and false in every one since* — "on this screen but not in front of your eyes" is neither
/// "they have seen it" nor "the desktop is all that is left", and a predicate has room for two.
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
/// if place.hidden                  { Toast }    // out of reach; the desktop is what is left
/// else if place.focused && active  { Nothing }  // you are looking at it
/// else if place.exposed            { Marks }    // in plain sight; the dot on the tab is enough
/// else if taskbar_is_auto_hidden   { Toast }    // covered, and no mark to glance at
/// else                             { Flash }    // covered, but the taskbar is there to call you
/// ```
///
/// **`place.hidden` outranks the focus bits, and the unreachable rows are the reason it is written
/// outermost.** The window facts owe every one of their combinations an answer, and the ones that
/// pair `hidden` with `focused` — a minimised window that holds the keyboard — cannot happen.
/// Answering them from the hidden bit is not defensive: it is what covers the frame after a
/// minimise, before the focus bits have caught up. The pairs of `hidden` with `exposed` are
/// unreachable for a second, plainer reason: a window that is on no screen is under no point, and
/// `sample_window_place` does not even probe one.
///
/// # The exposure row (user ruling 2026-09-01)
///
/// **A window the reader can see is a window whose own marks have already said it.** The reader's
/// report is the whole argument: multiple monitors, an auto-hiding taskbar, Folio sitting in plain
/// sight on the second screen — and a desktop toast every time a turn ended. Every tier below
/// `Nothing` was written for a window the reader would have to be *called* to, and this one is not.
/// The dot on its tab is already in their field of view, so the flash and the toast are both
/// pointing at something they can see.
///
/// **What it takes away is the taskbar flash and the desktop message, and nothing else.** The
/// in-window marks are not this function's to give or withhold — the tab's dot, the title bar's
/// badge and the queue's own badge are painted from the ledger, which this does not touch — and
/// [`Interruption::Nothing`] is by its own definition "the in-window marks and nothing beyond
/// them". That is why the arm is [`crate::attention::Reach::Marks`] rather than a fourth
/// [`Interruption`]: what a delivery *asks of the window* really is the same as `Nothing`'s, and it
/// is the *sentence about the reader* that differs. A tier that could not say which of the two
/// happened would make the trace unable to explain a silence.
///
/// **It is tested after the focus pair and before the taskbar bit.** After the focus pair, because
/// "you are looking at this pane" is a stronger statement than "you could see this window" and the
/// ledger spends a look on the first only. Before the taskbar bit, because the taskbar rows are
/// about *how to call a reader who is not looking*, and a reader who can see the window does not
/// have to be called at all — this is the row that takes the reader's own defect off the table.
///
/// **It makes the covered case reachable for the first time.** Under the three-tier table an
/// unfocused window answered `Flash` (or `Toast` on an auto-hiding desktop) whether it was in plain
/// sight or buried under a full-screen editor, because nothing here could tell those apart. Now
/// they part here: in sight is `Marks`, and buried falls through to exactly the pair of rows that
/// were always underneath — the flash where there is a taskbar to see it, and the desktop where
/// there is not.
///
/// **The old second row is retired and its argument with it.** It used to read `Flash` for a
/// background tab of the *focused* window, defended as honest-not-literal: `FlashWindowEx` on the
/// foreground window is a documented no-op, so the visible product was the in-window marks anyway.
/// The defence was always that `Nothing` would make that row and "the pane you are staring at"
/// indistinguishable. `Marks` is the name that row wanted: it says the visible product exactly,
/// keeps it distinct from `Nothing`, and stops relying on a Win32 no-op to be correct.
///
/// # The taskbar row (user ruling 2026-08-28)
///
/// **The flash tier does not exist on a desktop whose taskbar hides itself.** That tier is defined
/// as *a mark you can glance at and not be interrupted by*, and the definition has a premise: the
/// mark is on screen. With the taskbar set to auto-hide it is not, and `FLASHW_TRAY` does not draw
/// a quiet mark — it makes the shell slide the whole bar out over the reader's work and leave it
/// there, blinking, until this window comes to the foreground. That is louder than the tier above
/// it, and the reader reported it as exactly that: they expected a notification and got a taskbar
/// that would not go away. So where that tier does not exist the request goes on to the desktop,
/// the same road a minimised window takes, and for the same reason — there is nowhere nearer to say
/// it.
///
/// **Asked per delivery rather than remembered**, because it is a setting the reader can change
/// between one wait and the next and nothing tells this process when they do. It is a fact of the
/// desktop and not of this window, which is why it rides in beside the others rather than being
/// read here: `desktop_reach` is a function of facts and reads nothing itself.
///
/// **No new settings row.** A switch would ask the reader to describe their own desktop to this
/// program, and the shell already knows the answer.
#[must_use]
pub fn desktop_reach(tab_is_active: bool, place: WindowPlace) -> crate::attention::Reach {
    use crate::attention::Reach;
    if place.hidden {
        Reach::Toast
    } else if place.focused && tab_is_active {
        Reach::Nothing
    } else if place.exposed {
        Reach::Marks
    } else if place.taskbar_is_auto_hidden {
        Reach::Toast
    } else {
        Reach::Flash
    }
}

/// **What one allowed delivery asks of the window** (`attention` plan §11.7, slice C3).
///
/// The three-armed consumer of [`crate::attention::Reach`], as a value rather than as three arms
/// of a `match` inside the method that performs them. It is written out here for the reason the
/// reach itself is: the arms have a *shape* — that a `Toast` never flashes and a `Flash` never
/// reaches the desktop — and a shape nothing can name is a shape nothing can hold. The ruling of
/// 2026-08-28 turns on exactly that pair of negatives.
///
/// **Three arms against four reaches, and that is not a gap** (user ruling 2026-09-01). This says
/// what a delivery *asks of the window*, and there are only three things it can ask; the reach says
/// *how far the reader is*, and there are four answers to that. `Reach::Nothing` and
/// `Reach::Marks` differ in the sentence about the reader and not in the syscall, so they land on
/// the same arm here — a fourth variant would name two things that do the same thing, and the trace
/// already carries the distinction where it means something.
///
/// **Not a second gate** (red line 12). Whether the interruption was owed was settled at the door;
/// this is the last translation between a decision and a syscall, and it asks nothing new.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Interruption {
    /// The in-window marks and nothing beyond them.
    Nothing,
    /// `bt_platform::flash_window` on this window's taskbar button — **once per turn however many
    /// panes spoke**, because the button is the window's and flashing it twice is not twice as
    /// much of anything.
    FlashTheTaskbarButton,
    /// A message on the desktop.
    PutItOnTheDesktop,
}

/// **The last gate a delivery passes, and it is `Notifications` and not the door**
/// (§7.1.5o ③, user ruling 2026-08-26).
///
/// `terminal_notifications` governs whether a program may *put a message on the desktop*, so it
/// takes the third arm and leaves the second alone: a taskbar button calling for attention is not
/// a message and leaves nothing behind. A `Reach::Flash` the door let through still flashes with
/// `Notifications` off.
///
/// A refused toast is [`Interruption::Nothing`] rather than a demotion to the flash. The row says
/// the desktop is not to be written to; it does not say to interrupt some other way, and on the
/// desktop this block was reopened for — one whose taskbar hides itself — a demotion would be the
/// very slide-out the ruling took the flash off.
#[must_use]
pub fn interruption(reach: crate::attention::Reach, desktop_messages: bool) -> Interruption {
    use crate::attention::Reach;
    match reach {
        Reach::Nothing | Reach::Marks => Interruption::Nothing,
        Reach::Flash => Interruption::FlashTheTaskbarButton,
        Reach::Toast if desktop_messages => Interruption::PutItOnTheDesktop,
        Reach::Toast => Interruption::Nothing,
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
    use super::{
        Interruption, NotificationRoute, WindowPlace, desktop_reach, interruption, toast_title,
    };
    use crate::attention::Reach;

    /// **A window on a screen, unfocused, with something on top of it**, on a desktop whose taskbar
    /// is where Windows puts it.
    ///
    /// The row the flash was always the answer to, and — since the exposure probe — the only row it
    /// still is. Tests that are not about the probe start here and say so by name, because "covered"
    /// used to be a thing this module could not tell and is now the thing that keeps the flash.
    const BURIED: WindowPlace = WindowPlace {
        focused: false,
        hidden: false,
        exposed: false,
        taskbar_is_auto_hidden: false,
    };

    /// The same window with nothing on top of it: the reader can see it from where they sit.
    const IN_SIGHT: WindowPlace = WindowPlace {
        exposed: true,
        ..BURIED
    };

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

    /// PIN (§7.6, `attention` plan §10.7) — **every position a reader can be in, and the four
    /// answers they fall into.**
    ///
    /// Four window facts and the pane's own, so thirty-two inputs, and the whole behavioural claim
    /// of the desktop half of this block is which answer each one lands in. The sixteen with the
    /// window *on a screen* are written out row by row, because they are the specification. The
    /// sixteen with it hidden are one sentence and are asserted as one — a window that is on no
    /// screen answers `Toast` whatever else is true of it — and the set of visited combinations is
    /// counted at the end so that "one sentence" cannot quietly become "fifteen rows and a gap".
    ///
    /// The rows nobody can reach are here for the reason a total function has them at all. A
    /// minimised window does not hold the keyboard, so the hidden-and-focused rows describe no
    /// moment a reader can be in — except the frame *just after* a minimise, when the focus bits
    /// have not caught up, and that frame is precisely why the hidden bit is tested outermost. The
    /// hidden-and-exposed rows are unreachable for a plainer reason still: `sample_window_place`
    /// does not probe a window that is on no screen.
    ///
    /// MUTATIONS: test the focus pair before the hidden bit and the hidden-and-focused rows answer
    /// `Nothing`/`Marks` — a window that is not on any screen reported as one the reader can see.
    /// Drop the exposure arm and all six `Marks` rows answer `Flash` or `Toast` — the reader's own
    /// report, restored. Test exposure *before* the focus pair and the pane under the reader's eyes
    /// stops being told apart from the window merely in their field of view, which is the one
    /// distinction the ledger spends a look on. Test exposure *after* the taskbar bit and the
    /// auto-hiding desktop's `Marks` rows go back to `Toast`, which is the defect this row exists
    /// to remove.
    #[test]
    fn every_one_of_the_thirty_two_positions_a_reader_can_be_in_has_an_answer() {
        use std::collections::HashSet;

        // (window_is_focused, tab_is_active, exposed, taskbar_is_auto_hidden) → Reach, for the
        // sixteen positions of a window that is on a screen.
        let on_a_screen = [
            ((false, false, false, false), Reach::Flash),
            ((false, false, false, true), Reach::Toast),
            ((false, false, true, false), Reach::Marks),
            ((false, false, true, true), Reach::Marks),
            ((false, true, false, false), Reach::Flash),
            ((false, true, false, true), Reach::Toast),
            ((false, true, true, false), Reach::Marks),
            ((false, true, true, true), Reach::Marks),
            ((true, false, false, false), Reach::Flash),
            ((true, false, false, true), Reach::Toast),
            ((true, false, true, false), Reach::Marks),
            ((true, false, true, true), Reach::Marks),
            ((true, true, false, false), Reach::Nothing),
            ((true, true, false, true), Reach::Nothing),
            ((true, true, true, false), Reach::Nothing),
            ((true, true, true, true), Reach::Nothing),
        ];

        let mut visited = HashSet::new();
        for ((focused, active, exposed, taskbar_is_auto_hidden), expected) in on_a_screen {
            let place = WindowPlace {
                focused,
                hidden: false,
                exposed,
                taskbar_is_auto_hidden,
            };
            visited.insert((focused, active, false, exposed, taskbar_is_auto_hidden));
            assert_eq!(
                desktop_reach(active, place),
                expected,
                "on a screen: {place:?} active={active}"
            );
        }

        for focused in [false, true] {
            for active in [false, true] {
                for exposed in [false, true] {
                    for taskbar_is_auto_hidden in [false, true] {
                        let place = WindowPlace {
                            focused,
                            hidden: true,
                            exposed,
                            taskbar_is_auto_hidden,
                        };
                        visited.insert((focused, active, true, exposed, taskbar_is_auto_hidden));
                        assert_eq!(
                            desktop_reach(active, place),
                            Reach::Toast,
                            "a window on no screen has only the desktop: {place:?} active={active}"
                        );
                    }
                }
            }
        }

        assert_eq!(
            visited.len(),
            32,
            "a five-bool function owes thirty-two answers"
        );
    }

    /// RED GATE (user ruling 2026-09-01) — **a window the reader can see is not called out to.**
    ///
    /// # The bug this is the gate for
    ///
    /// Multiple monitors, a taskbar set to auto-hide, Folio standing in plain sight on the second
    /// screen: every turn that ended arrived as a desktop toast. The window was not minimised and
    /// not cloaked, so it was not hidden; it did not have the keyboard, so it was not the focused
    /// row; and the taskbar row then sent it straight to the desktop. Three facts, and not one of
    /// them was "the reader is looking right at it".
    ///
    /// The probe is that fact. With it, the same window answers `Marks`: the dot on its tab is
    /// already in the reader's field of view, and there is nothing to add.
    ///
    /// MUTATIONS: drop the exposure arm and both halves go red — the auto-hiding desktop back to
    /// `Toast`, which is the report, and the ordinary desktop back to `Flash`, which is the same
    /// mistake made quietly. Map `Marks` to `FlashTheTaskbarButton` in `interruption` and the last
    /// block goes red: the flash comes back under a different name.
    #[test]
    fn a_window_standing_in_plain_sight_is_told_nothing_it_is_not_already_showing() {
        for taskbar_is_auto_hidden in [false, true] {
            for (focused, active) in [(false, false), (false, true), (true, false)] {
                let place = WindowPlace {
                    focused,
                    taskbar_is_auto_hidden,
                    ..IN_SIGHT
                };
                let reach = desktop_reach(active, place);
                assert_eq!(
                    reach,
                    Reach::Marks,
                    "the reader can see this window: {place:?} active={active}"
                );
                assert_eq!(
                    interruption(reach, true),
                    Interruption::Nothing,
                    "the marks inside the window are the whole of it"
                );
            }
        }

        assert_ne!(
            desktop_reach(false, IN_SIGHT),
            Reach::Flash,
            "nothing asks the shell to call an eye that is already here"
        );
        assert_ne!(
            desktop_reach(
                false,
                WindowPlace {
                    taskbar_is_auto_hidden: true,
                    ..IN_SIGHT
                }
            ),
            Reach::Toast,
            "and nothing writes to a desktop the window is standing on"
        );
    }

    /// RED GATE (user ruling 2026-09-01, correcting `attention` plan §5.2's stated cost) — **a
    /// window buried under somebody else's is reachable at last, and it falls through to the two
    /// rows that were always underneath.**
    ///
    /// Slice C1 wrote "completely covered by another window" off as unaffordable and priced the
    /// cost out loud: such a window would flash a taskbar button instead of raising a toast. That
    /// cost was only ever paid because the fact was missing — the covered window and the window in
    /// plain sight were one row. Now they are two, and the covered one keeps the road it was
    /// always meant to have: the flash where there is a bar to see it, and the desktop where there
    /// is not.
    ///
    /// MUTATIONS: answer `Marks` for a covered window and a reader with a full-screen editor over
    /// Folio is never told anything again. Answer `Toast` for the covered row on a *visible*
    /// taskbar and the quiet tier is lost on the desktop the ruling was not about — a taskbar
    /// button is visible in exactly that situation, which is C1's own argument and it survives.
    #[test]
    fn a_covered_window_still_takes_the_road_that_was_always_under_it() {
        assert_eq!(
            desktop_reach(true, BURIED),
            Reach::Flash,
            "there is a taskbar to call the eye to"
        );
        assert_eq!(
            desktop_reach(
                true,
                WindowPlace {
                    taskbar_is_auto_hidden: true,
                    ..BURIED
                }
            ),
            Reach::Toast,
            "and where there is not, the desktop is what is left"
        );
        assert_eq!(
            desktop_reach(
                true,
                WindowPlace {
                    focused: true,
                    ..BURIED
                }
            ),
            Reach::Nothing,
            "a window that holds the keyboard is one the reader is at, whatever the probe says"
        );
    }

    /// PIN (§7.1.5o ⑩, user ruling 2026-08-28) — **a taskbar that hides itself has no middle
    /// tier, so the request goes on to the third.**
    ///
    /// The defect this pins is the one the reader reported from `next16`: their taskbar is set to
    /// auto-hide, so the second tier's "a mark you can glance at" was in fact the shell shoving
    /// the whole bar out over their work and blinking it until they switched back — louder than
    /// the desktop notification they had expected, and the one thing they could not dismiss.
    ///
    /// The window here is **not hidden and not exposed**: it is on a screen, unfocused, with
    /// something on top of it — exactly the row that used to be the flash. That is what makes this
    /// a new row rather than the minimised one restated. The exposure bit is `false` throughout and
    /// that is load-bearing since 2026-09-01: a window the reader can *see* never reaches the
    /// taskbar row at all, which is the point of the row above it.
    ///
    /// MUTATIONS: test the taskbar bit before the focus pair and a reader looking straight at the
    /// pane gets a desktop toast — the loudest way this block can be wrong. Drop the bit from
    /// `desktop_reach` entirely and every row goes back to flashing a bar that is not there.
    /// Answer `FlashTheTaskbarButton` from `interruption` for a `Toast` and the flash comes back
    /// under a different name.
    #[test]
    fn an_auto_hidden_taskbar_makes_the_second_tier_unreachable() {
        let hidden_bar = WindowPlace {
            taskbar_is_auto_hidden: true,
            ..BURIED
        };
        for (active, focused) in [(true, false), (false, false), (false, true)] {
            let reach = desktop_reach(
                active,
                WindowPlace {
                    focused,
                    ..hidden_bar
                },
            );
            assert_eq!(
                reach,
                Reach::Toast,
                "the flash's own rows go to the desktop: active={active} focused={focused}"
            );
            assert_eq!(interruption(reach, true), Interruption::PutItOnTheDesktop);
            assert_ne!(
                interruption(reach, true),
                Interruption::FlashTheTaskbarButton,
                "nothing on this road asks the shell to slide the bar out"
            );
        }
        assert_eq!(
            desktop_reach(
                true,
                WindowPlace {
                    focused: true,
                    ..hidden_bar
                }
            ),
            Reach::Nothing,
            "a reader looking at the pane is owed nothing, whatever their taskbar does"
        );
    }

    /// PIN (§7.1.5o ⑩) — **and a taskbar that is on the screen keeps the flash.**
    ///
    /// The other half of the ruling, and the half a fix is likeliest to lose: `FLASHW_TIMERNOFG`
    /// on a visible taskbar is what Windows Terminal and VS Code do, the reader did not complain
    /// about it, and it stays. Written as its own test so that "the flash survived" is a red
    /// light of its own rather than a row inside the test about removing it.
    ///
    /// MUTATIONS: send every unfocused window to the desktop and this goes red — a machine with a
    /// perfectly visible taskbar loses the quiet tier it was built for.
    #[test]
    fn a_visible_taskbar_keeps_the_flash() {
        let reach = desktop_reach(true, BURIED);
        assert_eq!(reach, Reach::Flash);
        assert_eq!(
            interruption(reach, true),
            Interruption::FlashTheTaskbarButton
        );
        assert_eq!(
            interruption(reach, false),
            Interruption::FlashTheTaskbarButton,
            "a taskbar button is not a message, so `Notifications` does not gate it"
        );
    }

    /// PIN (§7.1.5o ③) — **the desktop row gates the third tier and nothing else.**
    ///
    /// `terminal_notifications` says whether a program may put a message on the desktop. A toast
    /// it refuses is nothing at all — not a flash by another route, which on the desktop this
    /// block was reopened for is the slide-out the ruling above just removed.
    #[test]
    fn a_refused_toast_is_silence_and_not_a_flash() {
        assert_eq!(
            interruption(Reach::Toast, false),
            Interruption::Nothing,
            "the row says do not write to the desktop, not interrupt some other way"
        );
        assert_eq!(interruption(Reach::Nothing, true), Interruption::Nothing);
    }

    /// PIN (`attention` plan §10.7, red line 4) — **the ledger's predicate and the desktop's
    /// answer are two questions, and the second row is where they part.**
    ///
    /// `attention_is_consumed` says the reader has seen it; `desktop_reach` says how far away the
    /// reader is. In the two-tier model the second was written as the negation of the first, and
    /// this is the row that made that false: a background tab of the focused window is *not*
    /// consumed — the ledger admits it to the queue — and it is *not* reachable by a toast either.
    /// The old predicate had one answer for those two facts.
    ///
    /// **The answer for that row is `Marks` since 2026-09-01 and the point stands unchanged.** It
    /// used to be `Flash`, defended as honest-not-literal because `FlashWindowEx` on the foreground
    /// window is a documented no-op; `Marks` is that same visible product said outright. What the
    /// row is here to show is that it is *not* `Nothing` — the ledger admits it, and this function
    /// still has an answer of its own for it.
    #[test]
    fn a_background_tab_of_the_focused_window_is_neither_seen_nor_worth_a_toast() {
        assert!(
            !crate::attention_is_consumed(false, true),
            "the ledger admits it: nobody is looking at that tab"
        );
        assert_eq!(
            desktop_reach(
                false,
                WindowPlace {
                    focused: true,
                    ..IN_SIGHT
                }
            ),
            Reach::Marks,
            "a focused window is one the reader can see, and its other tab's dot with it"
        );
        assert_eq!(
            desktop_reach(
                false,
                WindowPlace {
                    focused: true,
                    ..BURIED
                }
            ),
            Reach::Flash,
            "and the same tab of a window buried under something else is still called out to"
        );
        assert_eq!(
            desktop_reach(
                true,
                WindowPlace {
                    focused: true,
                    ..IN_SIGHT
                }
            ),
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
