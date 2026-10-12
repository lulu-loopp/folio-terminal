//! The joined-panic guard's fixture (`tests/contained_panic.rs`), read beside `bt-platform`: two
//! joins whose errors are real values, the joins and shapes the guard refuses, a foreign crate's
//! `join` named in full, and the joins and occurrences it does not judge.

use bt_platform::contained_panic::contain as keep;
use std::thread::JoinHandle;
use std::thread::spawn as start;

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

/// An operation of this crate with a `join` of its own (a declaration: no join).
pub struct Operation;

impl Operation {
    pub fn begin() -> Self {
        Self
    }

    pub fn join(self) -> Result<u8, String> {
        Ok(5)
    }
}

/// A handle an associated function returns: refused, not followed.
pub fn from_an_associated_function() -> Result<u8, String> {
    let operation = Operation::begin();
    operation.join().map_err(|error| error.to_uppercase())
}

/// A contained body started through a renamed `use` of `std::thread::spawn`: refused, not
/// followed (the reading knows `std`'s start only by its path).
pub fn through_a_renamed_spawn() -> bool {
    let thread = start(|| bt_platform::contained_panic::contain(|| 6));
    thread.join().is_err()
}

/// A handle kept in a collection: refused, not followed.
pub fn from_a_collection() -> bool {
    let mut threads = Vec::new();
    threads.push(std::thread::spawn(|| bt_platform::contained_panic::contain(|| 7)));
    threads.pop().is_some_and(|thread| thread.join().is_err())
}

/// A zero-argument `join` in a macro's tokens: refused.
pub fn in_a_macro() {
    let thread = std::thread::spawn(|| {});
    println!("{}", thread.join().is_ok());
}

/// `JoinHandle::join` handed on as a value: refused.
pub fn as_a_value(threads: Vec<JoinHandle<()>>) -> usize {
    threads.into_iter().map(JoinHandle::join).count()
}

/// A foreign crate's operation, its `join` named by its type's full path: no thread's.
pub fn a_foreign_operation(operation: &u8) -> Result<u8, String> {
    windows_future::IAsyncOperation::join(operation).map_err(|error| error.message())
}

/// A foreign operation's `join` through an imported name, not the full path: refused.
pub fn a_foreign_operation_imported(operation: &u8) -> Result<u8, String> {
    use windows_future::IAsyncOperation as Operated;
    Operated::join(operation).map_err(|error| error.message())
}

/// A path-called `join` in a macro's tokens, with its one argument: refused.
pub fn a_path_join_in_a_macro(thread: JoinHandle<()>) {
    assert!(matches!(JoinHandle::join(thread), Err(_)));
}

/// A trait whose associated type is a thread's handle.
pub trait Started {
    type Handle;
}

/// A generic parameter's associated type's `join`: refused (no foreign operation on the list).
pub fn a_generic_handle<T: Started<Handle = JoinHandle<()>>>(handle: T::Handle) -> bool {
    T::Handle::join(handle).is_err()
}

/// A dependency's own thread handle, written in full from its crate's root: refused (not on the
/// list).
pub fn a_dependencys_thread_handle(handle: u8) -> bool {
    crossbeam_utils::thread::ScopedJoinHandle::join(handle).is_err()
}

/// A listed type's full path whose crate name a `use` in scope shadows: refused (the first
/// segment is not that crate here).
pub fn a_shadowed_crate_name(handle: u8) -> bool {
    use crate::contained_panic as windows_future;
    windows_future::IAsyncOperation::join(handle).is_err()
}

/// A binding named `join` and its value: names, no join.
pub fn a_binding_named_join() -> u8 {
    let (join, run) = (1, 2);
    join + run
}

/// A `join` with arguments, in code and in a macro: no thread's.
pub fn with_arguments() -> String {
    let folder = std::path::Path::new("一个").join("folder");
    format!("{}", ["a", "b"].join(&folder.display().to_string()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_test_may_name_a_panic() {
        let thread = std::thread::spawn(|| {});
        assert!(thread.join().is_ok());
    }
}
