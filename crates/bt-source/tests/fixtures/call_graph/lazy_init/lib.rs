//! A `static`'s and a `const`'s initializer, walked from the value's use: a `LazyLock`'s closure
//! calls a function and hands two as values; a `const` holds a function pointer.

use std::sync::LazyLock;

fn probe() -> bool {
    true
}

fn note(_: &str) {}

fn run(_: fn(&str)) -> Lane {
    Lane
}

pub struct Lane;

impl Lane {
    pub fn ask(&self) {}
}

pub static CELL: LazyLock<Lane> = LazyLock::new(|| {
    let _ = probe();
    run(note)
});

pub fn root() {
    CELL.ask();
}

fn target() {}

pub const F: fn() = target;

pub fn calls_const() {
    F();
}

pub fn names_const() {
    let _ = F;
}
