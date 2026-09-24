// No `#[path]` and no directory of its own: rustc looks for `child.rs` beside
// `lib.rs`, and its E0583 for a missing one says to create exactly that.
mod child;
