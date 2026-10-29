//! Tells the test module which optimisation level this crate is compiled at (plan row 74).
//!
//! The stack a formula's render needs depends on it: the deepest admitted formula needs 1.76 MiB
//! in the test profile (`opt-level = 1`) and 6.16 MiB unoptimised (`MATH_WORKER_STACK_BYTES`'s
//! measurement). The suite holds an optimised build to a quarter of the worker's stack and every
//! build to the whole of it, so it has to know which one it is; no `cfg` says so, and Cargo hands
//! the level to a build script only. A product build reads nothing of this.

fn main() {
    let level = std::env::var("OPT_LEVEL").expect("Cargo sets OPT_LEVEL for every build script");
    println!("cargo::rustc-env=BT_MATH_OPT_LEVEL={level}");
    // The level is part of the profile, and a profile change is a new build of this script
    // already; nothing else it reads can change.
    println!("cargo::rerun-if-changed=build.rs");
}
