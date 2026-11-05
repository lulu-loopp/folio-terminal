//! The thread door's own proofs (design note 2026-09-26, revision (d)1's A1b rows and A1c's role
//! witness for this crate). What admission does on the worker the door makes — the refusals, the
//! phase writers, the callback scope — is proved in `bt-effects`, on a worker made by the same
//! `lend_worker` this door calls.
//!
//! **Isolation.** Every case runs on a thread the door starts for it, so no case inherits another's
//! role.

use super::*;

/// Run `body` on a thread the thread door starts, lending it the capability, and hand its answer
/// (or its panic) back — the real producer of a worker (revision (d)1: no stand-in).
fn on_a_door_started_thread<T: Send + 'static>(
    name: &'static str,
    body: impl FnOnce(&WorkerCtx) -> T + Send + 'static,
) -> T {
    let worker = spawn_at_priority(name, crate::ThreadPriority::BelowNormal, body)
        .expect("the door starts a thread");
    match worker.join() {
        Ok(answer) => answer,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// RED (A1b, M4g) — **a thread the door started is a worker by its name: its role is `Worker`
/// with the name it was started under, its lent capability carries the same name, and it has no
/// phase** (a phase is the window thread's).
///
/// The door half of the role boundary with the real producer: the thread is started by
/// [`spawn_at_priority`], not made a worker by hand. That a worker is refused an owner-thread wait
/// is `bt-effects`' `a_worker_is_refused_an_owner_wait`, on the worker `lend_worker` makes.
///
/// MUTATION: in `spawn_at_priority_with_stack`, call `lend_worker("bt-door")` instead of
/// `lend_worker(name)` and the role and name assertions go red.
#[test]
fn a_thread_the_door_started_is_a_worker_by_its_name() {
    let (role_inside, lent, phase_inside) =
        on_a_door_started_thread("bt-probe-worker", |ctx| (role(), ctx.name(), phase()));
    assert_eq!(role_inside, Role::Worker("bt-probe-worker"));
    assert_eq!(lent, "bt-probe-worker", "the capability names its thread");
    assert_eq!(phase_inside, None, "a worker has no phase");
}

/// RED (A1c) — **the attention endpoint's listener is a worker the thread door started: the line it
/// delivers is delivered on `Worker("folio-attention-endpoint")`.**
///
/// A1c's role witness for this crate, with the real producer end to end: a real endpoint started
/// by its own `start`, a real client writing one line, and the sink — which runs on the listener
/// thread — recording the role it runs under. Before A1c the listener was a bare
/// `std::thread::Builder`, and every thread started that way is `Unset`: neither a worker nor the
/// window, so a worker-only door would not compile there and nothing said what kind of thread
/// was delivering Folio's input. The same holds for the directory watches, the launch endpoint and
/// the video threads, which changed in the same way; this one is asked because it runs on every
/// platform that has an endpoint.
///
/// The endpoint's waits are on a clock that awaits the other end, and the line is heard within
/// the suite's patience (T-ATTENTION-ENDPOINT-WAITS-ON-A-CLOCK): `start_on` spawns the listener
/// exactly as the product's `start` does, which is the one thing asked here.
///
/// MUTATION: start the listener in `AttentionPipe::start_on` with
/// `std::thread::Builder::new().name("folio-attention-endpoint".to_owned()).spawn(move || …)`
/// again and the role delivered is `Unset`.
#[cfg(any(windows, unix))]
#[test]
fn the_attention_endpoint_delivers_on_a_worker_the_door_started() {
    let directory = bt_testpath::temp_path("bt-platform-attention-role");
    std::fs::create_dir_all(&directory).expect("make the data directory");
    let (sender, heard) = std::sync::mpsc::channel();
    let endpoint = crate::attention_pipe::AttentionPipe::start_on(
        &directory,
        std::sync::Arc::new(crate::launch_conversation::CountedClock::answering()),
        move |line| {
            let _ = sender.send((line, role()));
        },
    )
    .expect("open the attention endpoint");
    crate::attention_pipe::send_line(endpoint.name(), "raise probe:Role cap=abc")
        .expect("the endpoint took the line");
    assert_eq!(
        crate::door_skipped::received(&heard, "the endpoint's delivery"),
        (
            "raise probe:Role cap=abc".to_owned(),
            Role::Worker("folio-attention-endpoint")
        )
    );
}
