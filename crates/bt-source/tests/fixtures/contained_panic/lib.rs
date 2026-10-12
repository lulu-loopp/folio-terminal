//! The contained-panic guard's fixture (`tests/contained_panic.rs`), read beside `bt-platform`:
//! two catches handed to the real boundary, and five the guard refuses.

use bt_platform::contained_panic::contain as keep;
use std::panic::catch_unwind as quietly;

/// A lookalike of the boundary — the same module and function names, not the item.
pub mod contained_panic {
    pub fn contain<T>(work: impl FnOnce() -> T) -> T {
        let _ = std::panic::catch_unwind(|| 0);
        work()
    }
}

/// Handed to the real boundary through an alias: allowed.
pub fn through_the_alias() -> bool {
    keep(|| std::panic::catch_unwind(|| 1).is_ok()).unwrap_or(false)
}

/// Handed to the real boundary by its path: allowed.
pub fn through_the_path() -> bool {
    bt_platform::contained_panic::contain(|| std::panic::catch_unwind(|| 2).is_ok())
        .unwrap_or(false)
}

/// Handed to the lookalike: refused.
pub fn through_the_lookalike() -> bool {
    crate::contained_panic::contain(|| std::panic::catch_unwind(|| 3).is_ok())
}

/// A function that hands its work to the real boundary.
fn helper(work: impl FnOnce() -> bool) -> bool {
    keep(work).unwrap_or(false)
}

/// Handed to a function that calls the boundary — no direct edge: refused.
pub fn through_a_helper() -> bool {
    helper(|| std::panic::catch_unwind(|| 4).is_ok())
}

/// A bare catch: refused.
pub fn planted_bare() -> bool {
    std::panic::catch_unwind(|| 5).is_ok()
}

/// A catch imported under another name: refused at the import.
pub fn planted_renamed() -> bool {
    quietly(|| 6).is_ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_test_may_catch() {
        assert!(std::panic::catch_unwind(|| 7).is_ok());
    }
}
