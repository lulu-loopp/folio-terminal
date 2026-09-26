//! `admission::enter_standalone_main` — once per process, so it is proved in a process of its own
//! (design note 2026-09-26, revision (d)1: this target holds nothing else).

use bt_platform::admission::{self, Role};

/// RED (A1a, revision (c)6) — **a standalone process's main thread becomes a worker once: the
/// first entry lends a `WorkerCtx` by its name and the thread keeps the role; a second entry is
/// refused, counted, and its body does not run.**
///
/// The `attention` verb and the two Explorer-menu removals run in processes that never have a
/// window, and their main threads wait like any worker's. The entry is one of the two
/// places a `WorkerCtx` is ever made, so it must hand out exactly one per process.
///
/// MUTATION: drop the `STANDALONE_ENTERED` swap from `enter_standalone_main` and the second entry
/// runs its body.
#[test]
fn a_standalone_main_thread_is_entered_once_and_stays_a_worker() {
    assert_eq!(admission::role(), Role::Unset);
    let lent =
        admission::enter_standalone_main("standalone-probe", |ctx| (ctx.name(), admission::role()));
    assert_eq!(
        lent,
        Ok(("standalone-probe", Role::Worker("standalone-probe")))
    );
    assert_eq!(
        admission::role(),
        Role::Worker("standalone-probe"),
        "the role outlives the body"
    );

    let refusals = admission::refusals();
    let mut ran = false;
    let again = admission::enter_standalone_main("standalone-probe", |_ctx| ran = true);
    assert_eq!(
        again,
        Err(admission::Refused {
            door: "enter_standalone_main",
            role: Role::Worker("standalone-probe"),
            phase: None,
        })
    );
    assert!(!ran, "a refused entry runs nothing");
    assert_eq!(admission::refusals(), refusals + 1, "and is counted");

    // And once per process means once: a fresh thread with no role is refused too.
    let elsewhere = std::thread::spawn(|| admission::enter_standalone_main("second", |_ctx| ()))
        .join()
        .expect("the probe thread does not panic");
    assert_eq!(
        elsewhere,
        Err(admission::Refused {
            door: "enter_standalone_main",
            role: Role::Unset,
            phase: None,
        })
    );
}
