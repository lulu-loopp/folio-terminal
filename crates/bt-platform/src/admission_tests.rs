//! The executed half of `admission`'s proofs (design note 2026-09-26, revision (d)1's A1a rows).
//!
//! **Isolation.** Every case runs on a thread it starts itself, so no case inherits another's role
//! or phase (libtest runs cases on the main thread under `--test-threads=1`). Every refusal case
//! refuses its own probe door, so its "exactly one" is its own counter's. The phase writers' five
//! counters are process-wide, so the cases that refuse a writer take [`WRITERS`] first. No case
//! installs the process's meter; the meter cases use `test_meter_scope`, which is the calling
//! thread's.

use std::cell::RefCell;
use std::sync::Mutex;
use std::time::Instant;

use super::*;

// One probe door per refusal case, outside the registry (these exist only under `cfg(test)`).
door!(ProbeCallback, "probe", 0, [Starting, Running, Exiting]);
door!(ProbeUnset, "probe", 0, [Starting, Running, Exiting]);
door!(ProbeRunning, "probe", 0, [Running]);
door!(ProbeStarting, "probe", 0, [Starting]);
door!(ProbeEarlyRunning, "probe", 0, [Running]);
door!(ProbeEarlyExiting, "probe", 0, [Exiting]);
door!(ProbeQuit, "probe", 0, [Running, Exiting]);
door!(ProbeMetered, "probe", 7, [Running]);
door!(ProbeOuter, "probe", 8, [Running]);
door!(ProbeInner, "probe", 9, [Running]);

/// Taken by every case that makes a phase writer refuse, and by no other.
static WRITERS: Mutex<()> = Mutex::new(());

/// Run `body` on a thread of its own and hand its answer (or its panic) back.
fn on_a_fresh_thread<T: Send>(body: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|threads| match threads.spawn(body).join() {
        Ok(answer) => answer,
        Err(panic) => std::panic::resume_unwind(panic),
    })
}

fn counted(counter: &AtomicU64) -> u64 {
    counter.load(Ordering::Relaxed)
}

/// RED (A1a, M4g′) — **a callback thread is refused an owner-thread wait, the work does not run,
/// and the thread is nobody's again when the callback ends.**
///
/// The OS threads that run our callbacks (Media Foundation's work queue, a toast's activation, a
/// device-loss notice) are not the window thread, and a wait admitted there would be a wait the
/// budget never sees. The refusal names the callback, carries no phase (a phase is the window
/// thread's), and costs the door exactly one on its own counter.
///
/// MUTATION: admit on any role but `Unset` in `admitted` and `ran` comes back true.
#[test]
fn admission_is_refused_on_a_callback_thread_and_the_role_comes_back_when_the_scope_ends() {
    on_a_fresh_thread(|| {
        let before = refusals_of::<ProbeCallback>();
        let total_before = refusals();
        let mut ran = false;
        {
            let _scope = enter_callback("probe");
            assert_eq!(role(), Role::Callback("probe"));
            let answer = admitted::<ProbeCallback, _>(|_token| ran = true);
            assert_eq!(
                answer,
                Err(Refused {
                    door: "ProbeCallback",
                    role: Role::Callback("probe"),
                    phase: None,
                })
            );
        }
        assert!(!ran, "the work of a refused admission does not run");
        assert_eq!(refusals_of::<ProbeCallback>(), before + 1);
        assert!(refusals() > total_before, "and the process total counts it");
        assert_eq!(role(), Role::Unset, "the scope gives the thread back");
    });
}

/// RED (A1a, M4g″) — **a thread nothing has named is refused, however its door's phases read.**
///
/// `Unset` is never the window thread: a door admitted in every phase is still refused there.
///
/// MUTATION: check only the phase in `admitted` and the probe runs.
#[test]
fn admission_is_refused_on_a_thread_nothing_has_named() {
    on_a_fresh_thread(|| {
        let before = refusals_of::<ProbeUnset>();
        let mut ran = false;
        let answer = admitted::<ProbeUnset, _>(|_token| ran = true);
        assert_eq!(
            answer,
            Err(Refused {
                door: "ProbeUnset",
                role: Role::Unset,
                phase: None,
            })
        );
        assert!(!ran);
        assert_eq!(refusals_of::<ProbeUnset>(), before + 1);
        assert_eq!(phase(), None, "a phase is the window thread's alone");
    });
}

/// RED (A1a, M4i) — **on the window thread a door is admitted in its phases and refused outside
/// them, and the phase is what its writers last made it.**
///
/// The sequence the product walks: entered at `Starting`, a `Running` door refused (with the phase
/// in the refusal); the loop's first turn, and the same door admitted and run; the way out, and a
/// `Starting` door refused; a quit given up, and the `Running` door admitted again.
///
/// MUTATION: make `loop_running` a no-op and the second admission is refused.
#[test]
fn a_door_is_admitted_only_in_its_phases_and_the_phase_follows_its_writers() {
    on_a_fresh_thread(|| {
        assert!(enter_window_thread());
        assert_eq!(role(), Role::Window);
        assert_eq!(phase(), Some(Phase::Starting));

        let refused_before = refusals_of::<ProbeRunning>();
        let mut runs = 0;
        assert_eq!(
            admitted::<ProbeRunning, _>(|_token| runs += 1),
            Err(Refused {
                door: "ProbeRunning",
                role: Role::Window,
                phase: Some(Phase::Starting),
            })
        );
        assert_eq!(runs, 0);
        assert_eq!(refusals_of::<ProbeRunning>(), refused_before + 1);

        assert!(loop_running());
        assert_eq!(phase(), Some(Phase::Running));
        assert_eq!(admitted::<ProbeRunning, _>(|_token| runs += 1), Ok(()));
        assert_eq!(runs, 1, "admitted, and run once");

        assert!(exiting());
        assert_eq!(phase(), Some(Phase::Exiting));
        let starting_before = refusals_of::<ProbeStarting>();
        assert_eq!(
            admitted::<ProbeStarting, _>(|_token| runs += 1),
            Err(Refused {
                door: "ProbeStarting",
                role: Role::Window,
                phase: Some(Phase::Exiting),
            })
        );
        assert_eq!(refusals_of::<ProbeStarting>(), starting_before + 1);

        assert!(quit_abandoned());
        assert_eq!(phase(), Some(Phase::Running));
        assert_eq!(admitted::<ProbeRunning, _>(|_token| runs += 1), Ok(()));
        assert_eq!(runs, 2);
        assert_eq!(
            refusals_of::<ProbeRunning>(),
            refused_before + 1,
            "an admission counts nothing"
        );
    });
}

/// RED (A1a, revision (b)1) — **a loop that fails before its first turn still leaves through the
/// exit doors.**
///
/// `run_app` can return before winit's first callback, with the phase never having turned. `fn
/// main`'s `exiting()` must be taken from `Starting`, or the trace flush and the two bounded waits
/// of the way out would be refused on that road and a legitimate call counted as a violation.
///
/// MUTATION: refuse `Starting` in `exiting` and the third assertion goes red.
#[test]
fn a_loop_that_fails_before_its_first_turn_still_leaves_through_the_exit_doors() {
    on_a_fresh_thread(|| {
        assert!(enter_window_thread());
        let refused_before = refusals_of::<ProbeEarlyRunning>();
        assert_eq!(
            admitted::<ProbeEarlyRunning, _>(|_token| ()),
            Err(Refused {
                door: "ProbeEarlyRunning",
                role: Role::Window,
                phase: Some(Phase::Starting),
            })
        );
        assert_eq!(refusals_of::<ProbeEarlyRunning>(), refused_before + 1);
        assert!(exiting(), "the way out is taken from `Starting`");
        assert_eq!(phase(), Some(Phase::Exiting));
        let mut flushed = false;
        assert_eq!(
            admitted::<ProbeEarlyExiting, _>(|_token| flushed = true),
            Ok(())
        );
        assert!(flushed, "an exit door runs on that road");
    });
}

/// RED (A1a, revision (c)5) — **a quit that never reached its write changes no phase and counts
/// nothing; one that wrote and was refused comes back to `Running`.**
///
/// Cancel at the card and an incomplete save both reach the quit's `Abandon` arm without passing
/// `Write`, so `quit_abandoned` finds `Running` — an ordinary gesture, not a violation. `exiting` is
/// idempotent from `Exiting` for the same reason: its three call sites can meet on one way out.
///
/// MUTATION: refuse `Running` in `quit_abandoned` and the first assertion goes red.
#[test]
fn a_quit_given_up_before_its_write_changes_no_phase_and_counts_nothing() {
    on_a_fresh_thread(|| {
        assert!(enter_window_thread());
        assert!(loop_running());
        assert!(quit_abandoned(), "from `Running`, nothing to undo");
        assert_eq!(phase(), Some(Phase::Running));
        assert!(exiting());
        assert!(
            exiting(),
            "a second road out finds the thread already leaving"
        );
        assert_eq!(phase(), Some(Phase::Exiting));
        assert_eq!(admitted::<ProbeQuit, _>(|_token| ()), Ok(()));
        assert!(quit_abandoned());
        assert_eq!(phase(), Some(Phase::Running));
        assert_eq!(refusals_of::<ProbeQuit>(), 0);
    });
}

/// RED (A1a, M4i′ and revision (c)8 item 8) — **every phase writer refuses, and counts once, off
/// the window thread and on a transition not in its row; a second window entry cannot put a
/// running window thread back to `Starting`.**
///
/// The writers are pinned to a handful of call sites in `bt-app`; one called from the wrong thread
/// or at the wrong moment must neither move the phase nor pass unrecorded.
///
/// MUTATION: drop the role check in `write_phase` and the `Unset` thread's `loop_running` is
/// taken; drop the `count` in `enter_window_thread` and the duplicate entry goes uncounted.
#[test]
fn the_phase_writers_refuse_and_count_off_the_window_thread_and_off_their_rows() {
    let _writers = WRITERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Off the window thread: an `Unset` thread, and a callback.
    on_a_fresh_thread(|| {
        let before = [
            counted(&LOOP_RUNNING_REFUSED),
            counted(&EXITING_REFUSED),
            counted(&QUIT_ABANDONED_REFUSED),
            counted(&ENTER_WINDOW_REFUSED),
        ];
        assert!(!loop_running());
        assert!(!exiting());
        assert!(!quit_abandoned());
        assert_eq!(phase(), None);
        let scope = enter_callback("probe");
        assert!(
            !enter_window_thread(),
            "a callback does not become the window"
        );
        assert_eq!(role(), Role::Callback("probe"));
        drop(scope);
        assert_eq!(
            [
                counted(&LOOP_RUNNING_REFUSED),
                counted(&EXITING_REFUSED),
                counted(&QUIT_ABANDONED_REFUSED),
                counted(&ENTER_WINDOW_REFUSED),
            ],
            before.map(|count| count + 1)
        );
    });
    // On the window thread, transitions not in their rows.
    on_a_fresh_thread(|| {
        assert!(enter_window_thread());
        let before = [
            counted(&LOOP_RUNNING_REFUSED),
            counted(&QUIT_ABANDONED_REFUSED),
            counted(&ENTER_WINDOW_REFUSED),
        ];
        assert!(
            !quit_abandoned(),
            "nothing to give up before the loop turns"
        );
        assert_eq!(phase(), Some(Phase::Starting));
        assert!(loop_running());
        assert!(!loop_running(), "the first turn happens once");
        assert!(
            !enter_window_thread(),
            "a second entry is refused on the window thread"
        );
        assert_eq!(
            phase(),
            Some(Phase::Running),
            "and does not put it back to `Starting`"
        );
        assert!(exiting());
        assert!(
            !loop_running(),
            "nor does the way out turn into a first turn"
        );
        assert_eq!(phase(), Some(Phase::Exiting));
        assert_eq!(
            [
                counted(&LOOP_RUNNING_REFUSED),
                counted(&QUIT_ABANDONED_REFUSED),
                counted(&ENTER_WINDOW_REFUSED),
            ],
            [before[0] + 2, before[1] + 1, before[2] + 1]
        );
    });
}

/// RED (A1a, revision (c)7) — **nested callbacks on a thread nobody named: the outer one owns it,
/// the inner one does not, and the thread is nobody's after both.**
///
/// MUTATION: have every scope restore `Unset` (not only the owning one) and the inner scope's drop
/// takes the outer callback's name away while it is still running.
#[test]
fn nested_callbacks_on_an_unnamed_thread_leave_it_unnamed() {
    on_a_fresh_thread(|| {
        let outer = enter_callback("outer");
        assert_eq!(role(), Role::Callback("outer"));
        let inner = enter_callback("inner");
        assert_eq!(
            role(),
            Role::Callback("outer"),
            "the inner scope finds a role"
        );
        drop(inner);
        assert_eq!(role(), Role::Callback("outer"), "and restores nothing");
        drop(outer);
        assert_eq!(role(), Role::Unset);
    });
}

/// RED (A1a, revision (c)7) — **a callback delivered on the window thread is the window thread.**
///
/// AppKit's main queue and the message pump deliver most callbacks on the window thread, and
/// `Window` is the true answer there — before, inside and after the scope, and the phase with it.
///
/// MUTATION: make `enter_callback` set `Callback` whatever it finds and the window thread
/// answers as a callback.
#[test]
fn a_callback_on_the_window_thread_is_the_window_thread() {
    on_a_fresh_thread(|| {
        assert!(enter_window_thread());
        assert!(loop_running());
        {
            let _scope = enter_callback("probe");
            assert_eq!(role(), Role::Window);
            assert_eq!(phase(), Some(Phase::Running));
        }
        assert_eq!(role(), Role::Window);
    });
}

/// RED (A1a, revision (c)7) — **a callback that panics still gives its thread back.**
///
/// The operating system's pooled threads run the next block right after; a scope that restored
/// only on a normal return would leave that block wearing a stale name.
///
/// MUTATION: restore the role in a statement after the callback body instead of in `Drop` and
/// the thread stays `Callback`.
#[test]
fn a_callback_that_panics_still_gives_its_thread_back() {
    on_a_fresh_thread(|| {
        let unwound = std::panic::catch_unwind(|| {
            let _scope = enter_callback("probe");
            panic!("a callback that fails");
        });
        assert!(unwound.is_err());
        assert_eq!(role(), Role::Unset);
    });
}

// ---------------------------------------------------------------------------
// The meter
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Heard {
    Enter(&'static str),
    Leave(&'static str, Cookie, Instant, Instant),
}

thread_local! {
    static HEARD: RefCell<Vec<Heard>> = const { RefCell::new(Vec::new()) };
}

fn heard() -> Vec<Heard> {
    HEARD.with(|heard| heard.borrow().clone())
}

/// A meter that writes down what it is told and hands out a cookie per entry.
fn listening() -> Meter {
    fn enter(key: DoorKey) -> Cookie {
        HEARD.with(|heard| {
            let mut heard = heard.borrow_mut();
            heard.push(Heard::Enter(key.name()));
            Cookie::from_raw(heard.len() as u64)
        })
    }
    fn leave(key: DoorKey, cookie: Cookie, start: Instant, end: Instant) {
        HEARD.with(|heard| {
            heard
                .borrow_mut()
                .push(Heard::Leave(key.name(), cookie, start, end));
        });
    }
    Meter { enter, leave }
}

fn on_a_running_window_thread<T: Send>(body: impl FnOnce() -> T + Send) -> T {
    on_a_fresh_thread(|| {
        assert!(enter_window_thread());
        assert!(loop_running());
        body()
    })
}

/// RED (A1a, revision (c)2 / (d)2) — **an admitted call is entered once before its work and left
/// once after it, with the cookie its entry gave and a start no later than its end.**
///
/// The entry comes first so that a call that never returns is already named in the hang report;
/// the leave carries the cookie so the meter's owner can put back exactly what it moved.
///
/// MUTATION: call `leave` before `work` and the order assertion goes red.
#[test]
fn an_admitted_call_is_entered_before_its_work_and_left_after_it() {
    on_a_running_window_thread(|| {
        let _meter = test_meter_scope(listening());
        let answer = admitted::<ProbeMetered, _>(|_token| {
            assert_eq!(heard(), vec![Heard::Enter("ProbeMetered")], "entered first");
            7
        });
        assert_eq!(answer, Ok(7));
        let heard = heard();
        assert_eq!(heard.len(), 2);
        let Heard::Leave(name, cookie, start, end) = heard[1] else {
            panic!("the second thing the meter hears is the leave: {heard:?}");
        };
        assert_eq!(name, "ProbeMetered");
        assert_eq!(
            cookie,
            Cookie::from_raw(1),
            "the cookie the entry handed out"
        );
        assert!(start <= end);
    });
}

/// RED (A1a, revision (b)4) — **a call whose work panics is entered and never left.**
///
/// No guard runs on the unwind path: the meter's owner keeps the door as the thread's current
/// station, which is the true report — the last admitted call on that thread did not come back.
///
/// MUTATION: call `leave` from a drop guard and the meter hears a leave.
#[test]
fn a_call_whose_work_panics_is_entered_and_never_left() {
    on_a_running_window_thread(|| {
        let _meter = test_meter_scope(listening());
        let unwound = std::panic::catch_unwind(|| {
            let _ = admitted::<ProbeMetered, ()>(|_token| panic!("the door failed"));
        });
        assert!(unwound.is_err());
        assert_eq!(heard(), vec![Heard::Enter("ProbeMetered")]);
    });
}

/// RED (A1a, revision (c)2) — **an admitted call nested in another lies inside it.**
///
/// A3 adds up inclusive intervals; an inner interval that stuck out of its outer one would be time
/// counted in the outer call that it did not spend.
///
/// MUTATION: read `start` after calling `work` and the inner interval ends up after the outer.
#[test]
fn a_nested_call_lies_inside_the_call_it_is_nested_in() {
    on_a_running_window_thread(|| {
        let _meter = test_meter_scope(listening());
        let outer = admitted::<ProbeOuter, _>(|_outer| admitted::<ProbeInner, _>(|_inner| ()));
        assert_eq!(outer, Ok(Ok(())));
        let heard = heard();
        let [
            Heard::Enter("ProbeOuter"),
            Heard::Enter("ProbeInner"),
            Heard::Leave("ProbeInner", inner_cookie, inner_start, inner_end),
            Heard::Leave("ProbeOuter", outer_cookie, outer_start, outer_end),
        ] = heard[..]
        else {
            panic!("entered outer, inner; left inner, outer: {heard:?}");
        };
        assert_eq!(inner_cookie, Cookie::from_raw(2));
        assert_eq!(outer_cookie, Cookie::from_raw(1));
        assert!(outer_start <= inner_start && inner_end <= outer_end);
    });
}

/// RED (A1a) — **before a meter is installed, an admitted call is measured by nothing and still
/// runs.** Row 18's hand-over is admitted before `hang_watch::start` installs the meter.
///
/// MUTATION: refuse when no meter is installed and the work does not run.
#[test]
fn before_a_meter_is_installed_an_admitted_call_still_runs() {
    on_a_running_window_thread(|| {
        assert!(meter().is_none(), "this crate's tests install no meter");
        assert_eq!(admitted::<ProbeMetered, _>(|_token| 3), Ok(3));
        assert!(heard().is_empty());
    });
}

/// RED (A1a) — **a door's constants are its key's.** `ROW`, `STATION` and `PHASES` are what the
/// registry equality in `bt-app` reads, through `KEY`; they must not be able to disagree with it.
///
/// MUTATION: give `Door::STATION` a default other than `Self::KEY.station` and it goes red.
#[test]
fn a_doors_constants_are_its_keys() {
    assert_eq!(<doors::PtyResize as Door>::ROW, Row("12"));
    assert_eq!(<doors::PtyResize as Door>::STATION, 4);
    assert_eq!(
        <doors::PtyResize as Door>::PHASES,
        Phases::of(&[Phase::Running, Phase::Exiting])
    );
    assert_eq!(<doors::PtyResize as Door>::KEY.name(), "PtyResize");
    assert!(
        doors::ALL.contains(&<doors::PtyResize as Door>::KEY),
        "every door type is in the list"
    );
}
