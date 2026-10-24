//! The request-door guard's universe: a lane with a mailbox lock, a worker that holds its own
//! state's lock across work and sleeps, a stored refusal hook, an owner door's body, and one door
//! per mutation of the side-effects contract's (b)2.

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};

pub struct Lane {
    jobs: Mutex<VecDeque<u32>>,
    asked: Condvar,
    worker: Mutex<u32>,
    on_full: Box<dyn Fn() + Send + Sync>,
    on_done: Box<dyn Fn() + Send + Sync>,
}

impl Lane {
    /// The mailbox: the one lock a door may take, held for a queue operation.
    pub fn mailbox(&self, job: u32) -> bool {
        let mut jobs = self
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if jobs.len() >= 8 {
            return false;
        }
        jobs.push_back(job);
        true
    }

    /// The door's road: the mailbox, a refusal through the stored hook, the notify.
    pub fn offer(&self, job: u32) {
        if !self.mailbox(job) {
            let on_full = &self.on_full;
            on_full();
            return;
        }
        self.asked.notify_one();
    }

    /// The worker: its own state's lock held across work, and a sleep.
    pub fn serve(&self) {
        let state = self
            .worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::thread::sleep(std::time::Duration::from_millis(u64::from(*state)));
    }

    /// A helper that looks at the worker's state: the lock the worker holds across work.
    pub fn peek(&self) -> u32 {
        *self
            .worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// The thread door: its closure argument is the worker's.
pub fn spawn<F: FnOnce() + Send + 'static>(work: F) {
    work();
}

/// An owner door's body.
pub fn present() {}

fn record(job: u32) {
    let _ = std::fs::write("job", job.to_string());
}

/// The clean door: the mailbox, the notify, and a worker handed to the thread door.
pub fn submit(lane: &'static Lane, job: u32) {
    lane.offer(job);
    spawn(move || lane.serve());
}

/// Mutation 1: a helper of the door writes a file.
pub fn submit_writes(lane: &'static Lane, job: u32) {
    record(job);
    submit(lane, job);
}

/// Mutation 2: a helper of the door takes the lock the worker holds.
pub fn submit_peeks(lane: &'static Lane, job: u32) {
    let _ = lane.peek();
    submit(lane, job);
}

/// Mutation 3: a stored closure called on the door's road that no row lists.
pub fn submit_calls_back(lane: &'static Lane, job: u32) {
    submit(lane, job);
    let on_done = &lane.on_done;
    on_done();
}

/// An owner door's body reached from a request door.
pub fn submit_and_draw(lane: &'static Lane, job: u32) {
    submit(lane, job);
    present();
}

/// The worker's body run inline, nothing handed and nothing enqueued.
pub fn serve_inline(lane: &'static Lane) {
    lane.serve();
}

/// A table of the worker's functions, evaluated by the compiler.
pub struct Hooks {
    pub write: fn(u32),
}

pub const HOOKS: Hooks = Hooks { write: record };

/// An initializer that runs on its first deref, on whichever thread derefs it first.
pub static SEED: std::sync::LazyLock<bool> =
    std::sync::LazyLock::new(|| std::fs::read("seed").is_ok());

/// A constant that names the worker's writer: nothing in it runs here.
pub fn submit_with_hooks(lane: &'static Lane, job: u32) -> usize {
    let hooks = &HOOKS;
    submit(lane, job);
    std::mem::size_of_val(hooks)
}

/// A lazy initializer touched on the door's road: it may run here.
pub fn submit_seeded(lane: &'static Lane, job: u32) -> bool {
    submit(lane, job);
    *SEED
}
