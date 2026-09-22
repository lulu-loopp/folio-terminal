// Reached by `#[cfg(test)] #[path = "named.rs"] mod by_path;`. The `#[path]`
// decides which file, and the `#[cfg]` beside it decides what stands on every
// item in it.

pub fn reached_by_a_path() -> u8 {
    5
}
