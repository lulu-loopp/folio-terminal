//! The joined-panic guard's fixture (`tests/contained_panic.rs`), read beside `bt-platform`: two
//! joins whose errors are real values, five the guard refuses, one that is no thread's and two it
//! does not judge.

use bt_platform::contained_panic::contain as keep;
use std::thread::JoinHandle;

pub enum Exit {
    Ended,
    Panicked,
}

/// A lookalike of the boundary — the same module and function names, not the item.
pub mod contained_panic {
    pub fn contain<T>(work: impl FnOnce() -> T) -> std::thread::Result<T> {
        Ok(work())
    }
}

/// A bare thread joined with a panic arm: refused.
pub fn planted_bare() -> Exit {
    let reader = std::thread::spawn(|| {});
    match reader.join() {
        Ok(()) => Exit::Ended,
        Err(_) => Exit::Panicked,
    }
}

/// Through the thread door, its body handed to the real boundary by an alias: allowed.
pub fn through_the_door() -> bool {
    let thread = bt_platform::spawn_at_priority(
        "fixture-door",
        bt_platform::ThreadPriority::Normal,
        |_ctx| keep(|| 1),
    )
    .expect("a thread");
    thread.join().is_ok_and(|contained| contained.is_err())
}

/// A body handed to the lookalike: refused.
pub fn through_the_lookalike() -> bool {
    let thread = std::thread::Builder::new()
        .spawn(|| crate::contained_panic::contain(|| 2))
        .expect("a thread");
    thread.join().is_err()
}

/// A contained handle given to a field, joined by a method: allowed.
pub struct Drain {
    thread: Option<JoinHandle<std::thread::Result<u8>>>,
}

impl Drain {
    pub fn start() -> Self {
        let thread = std::thread::spawn(move || bt_platform::contained_panic::contain(|| 3));
        Self {
            thread: Some(thread),
        }
    }

    pub fn finish(&mut self) -> Option<u8> {
        let thread = self.thread.take()?;
        thread.join().ok()?.ok()
    }
}

/// A free function's bare handle.
fn reader() -> JoinHandle<Vec<u8>> {
    std::thread::spawn(Vec::new)
}

/// Its panic made a default: refused.
pub fn defaulted() -> Vec<u8> {
    reader().join().unwrap_or_default()
}

/// A handle handed in: refused, not followed.
pub fn handed_in(thread: JoinHandle<()>) -> bool {
    thread.join().is_err()
}

/// A function that hands its work to the real boundary.
fn helper() -> u8 {
    keep(|| 4).unwrap_or(0)
}

/// A body that calls the helper — no direct edge: refused.
pub fn through_a_helper() -> bool {
    let thread = std::thread::spawn(|| helper());
    thread.join().is_err()
}

/// Bare threads whose joins throw the result away and carry the panic on: not judged.
pub fn thrown_away_and_carried_on() {
    let first = std::thread::spawn(|| {});
    let _ = first.join();
    let second = std::thread::spawn(|| {});
    second.join().expect("the panic is carried on");
}

/// A foreign operation with a `join` of its own: not a thread's.
pub struct Operation;

impl Operation {
    pub fn begin() -> Self {
        Self
    }

    pub fn join(self) -> Result<u8, String> {
        Ok(5)
    }
}

/// Its error made a value: not judged as a thread's.
pub fn not_a_thread() -> Result<u8, String> {
    let operation = Operation::begin();
    operation.join().map_err(|error| error.to_uppercase())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_test_may_name_a_panic() {
        let thread = std::thread::spawn(|| {});
        assert!(thread.join().is_ok());
    }
}
