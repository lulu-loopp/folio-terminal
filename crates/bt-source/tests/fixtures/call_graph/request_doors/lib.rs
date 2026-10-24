//! The request-door guard's universe: a lane with a mailbox lock, a worker that holds its own
//! state's lock across work and sleeps, a stored refusal hook, an owner door's body, and one door
//! per mutation of the side-effects contract's (b)2 and per class and unresolved kind of the guard.
//! Its rows are `registry.tsv`, in `window_waits.tsv`'s shape.

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

impl Lane {
    /// An enqueue that runs its caller's closure under the mailbox lock, on the caller's thread.
    pub fn admit_with(&self, job: u32, between: impl FnOnce()) -> bool {
        let mut jobs = self
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        between();
        jobs.push_back(job);
        true
    }

    /// The mailbox through the generic lock helper.
    pub fn mailbox_through(&self, job: u32) {
        lock(&self.jobs).push_back(job);
    }

    /// The door's road through the generic helper: the mailbox, then the notify.
    pub fn offer_through(&self, job: u32) {
        self.mailbox_through(job);
        self.asked.notify_one();
    }

    /// The lock the worker holds, taken through the generic helper.
    pub fn peek_through(&self) -> u32 {
        *lock(&self.worker)
    }

    /// An entrance of another door.
    pub fn poke(&self) {}
}

/// The generic lock helper: it locks whatever mutex its caller hands it.
pub fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A thread door with a voice: `say` runs here, `work` on the thread it starts.
pub fn spawn_with(say: impl Fn(), work: impl FnOnce() + Send + 'static) {
    say();
    work();
}

/// H1: a closure handed to an enqueue that runs it inline is the door's.
pub fn submit_between(lane: &'static Lane, job: u32) {
    lane.admit_with(job, || record(job));
}

/// H1: of a thread door's two closures, the voice runs here and the work does not.
pub fn submit_with_voice(lane: &'static Lane, job: u32) {
    spawn_with(move || record(job), move || lane.serve());
}

/// H3: the mailbox taken through the generic helper by a declared mailbox method.
pub fn submit_through(lane: &'static Lane, job: u32) {
    lane.offer_through(job);
    spawn(move || lane.serve());
}

/// H3: the worker's lock taken through the generic helper by a method no row declares.
pub fn submit_peeks_through(lane: &'static Lane, job: u32) {
    let _ = lane.peek_through();
    submit_through(lane, job);
}

/// A door asked before the loop: the guard does not walk it.
pub fn prewarm() {}

/// H2: a walked door that reaches a door not walked.
pub fn submit_prewarms(lane: &'static Lane, job: u32) {
    prewarm();
    submit(lane, job);
}

/// A window door whose status names its repair.
pub fn note(_text: &str) {}

/// (vi): a window door under repair inherited.
pub fn submit_noting(lane: &'static Lane, job: u32) {
    note("asked");
    submit(lane, job);
}

/// A first-party effect of the vocabulary.
pub fn resize() {}

/// (i) first-party and (ii): an entrance and a first-party vocabulary entry reached.
pub fn submit_pokes(lane: &'static Lane, job: u32) {
    lane.poke();
    resize();
    submit(lane, job);
}

/// A worker's entry, named as a value.
fn work_fn() {}

/// A handed worker named as a value: the worker's.
pub fn submit_handing(lane: &'static Lane, job: u32) {
    lane.offer(job);
    spawn(work_fn);
}

/// A handed worker called on the door's road.
pub fn submit_runs_handed(lane: &'static Lane, job: u32) {
    work_fn();
    submit(lane, job);
}

/// Q6: a print macro on the door's road; a `format!` is pure.
pub fn submit_says(lane: &'static Lane, job: u32) {
    eprintln!("{job}");
    let _ = format!("{job}");
    submit(lane, job);
}

#[cfg(test)]
fn test_only() {}

pub struct Writer;

impl Writer {
    /// The one first-party method of its name: an untyped receiver's call has one candidate.
    pub fn writes_once(&self) {
        let _ = std::fs::write("once", b"");
    }
}

/// (v): a name nothing holds, a call on another arm, and an untyped receiver with one candidate.
pub fn submit_strays(lane: &'static Lane, job: u32) {
    nowhere();
    test_only();
    let thing = nowhere_made();
    thing.writes_once();
    submit(lane, job);
}

#[derive(Clone)]
pub struct Copyable;

/// A derive's callee: it passes.
pub fn submit_copies(lane: &'static Lane, job: u32, value: &Copyable) {
    let _ = value.clone();
    submit(lane, job);
}

/// The cutoff's target.
pub fn target() {}

/// A path chained past the hop limit.
pub mod chain {
    pub use crate::target as link0;
    pub use link0 as link1;
    pub use link1 as link2;
    pub use link2 as link3;
    pub use link3 as link4;
    pub use link4 as link5;
    pub use link5 as link6;
    pub use link6 as link7;
    pub use link7 as link8;
    pub use link8 as link9;
    pub use link9 as link10;
    pub use link10 as link11;
    pub use link11 as link12;
    pub use link12 as link13;
    pub use link13 as link14;
    pub use link14 as link15;
    pub use link15 as link16;
    pub use link16 as link17;
    pub use link17 as link18;
    pub use link18 as link19;
    pub use link19 as link20;
}

/// A cutoff on the door's road: refused in the walk.
pub fn submit_far(lane: &'static Lane, job: u32) {
    chain::link20();
    submit(lane, job);
}

/// One name in two modules on one arm: a bare key naming it is refused.
pub mod a {
    pub fn twin() {}
}

pub mod b {
    pub fn twin() {}
}

/// One name per platform's module: a bare key names one item per arm.
#[cfg(windows)]
pub mod on_windows {
    pub fn armed() {}
}

#[cfg(not(windows))]
pub mod elsewhere {
    pub fn armed() {}
}

/// Nit 5: a Win32 file-system function, a Win32 type's constructor (not a call of the module), a
/// path's question to the disk, and a park.
pub fn submit_asks_the_disk(lane: &'static Lane, job: u32) {
    let _ = windows::Win32::Storage::FileSystem::FILE_SHARE_MODE(0);
    let _ = windows::Win32::Storage::FileSystem::DeleteFileW(std::ptr::null());
    let _ = std::path::Path::exists(std::path::Path::new("job"));
    std::thread::park();
    submit(lane, job);
}

/// R2: the worker's lock under an alias of `Mutex`.
pub type Held = Mutex<u32>;

/// A lock helper whose argument is written through the alias: it locks whatever cell its caller
/// hands it, as [`lock`] does.
pub fn lock_cell(cell: &Held) -> std::sync::MutexGuard<'_, u32> {
    cell.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Lane {
    /// The lock the worker holds, taken through the aliased helper.
    pub fn peek_through_alias(&self) -> u32 {
        *lock_cell(&self.worker)
    }
}

/// R2: the worker's lock taken through the aliased helper by a method no row declares.
pub fn submit_peeks_through_alias(lane: &'static Lane, job: u32) {
    let _ = lane.peek_through_alias();
    submit_through(lane, job);
}
