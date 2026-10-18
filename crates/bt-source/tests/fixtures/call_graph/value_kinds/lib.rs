//! Functions named as values whose paths are no one item: every kind is an edge with `Via::Value`;
//! data — a `const`, a `static`, a unit struct, an associated `const` — is no edge.

use std::sync::{LazyLock, Mutex};

#[derive(Default)]
pub struct Settings;

pub trait Twice {
    fn f(&self);
}

pub trait Again {
    fn f(&self);
}

pub struct Both;

impl Twice for Both {
    fn f(&self) {}
}

impl Again for Both {
    fn f(&self) {}
}

impl Both {
    pub const CAP: usize = 2;

    pub fn root(&self, maybe: Option<Settings>) {
        let _ = maybe.unwrap_or_else(Settings::default);
        spawn(Self::f);
        spawn(worker);
        spawn(generated_fn);
        spawn(LIMIT);
        spawn(NAMES);
        spawn(Unit);
        spawn(Self::CAP);
        let _ = INBOX.lock();
        LANE.ask();
    }
}

pub trait Lonely {
    fn alone();
}

pub fn generic<T: Lonely>() {
    spawn(T::alone);
}

#[cfg(test)]
fn worker() {}

macro_rules! generate {
    () => {
        pub fn generated_fn() {}
    };
}

generate!();

pub const LIMIT: usize = 3;
pub static NAMES: [&str; 1] = ["a"];
pub static INBOX: Mutex<Vec<u8>> = Mutex::new(Vec::new());

pub static LANE: LazyLock<Lane> = LazyLock::new(|| Lane);

pub struct Lane;

impl Lane {
    pub fn ask(&self) {}
}

pub struct Unit;

pub fn spawn<T>(_: T) {}
