//! The contained-panic guard's fixture (`tests/contained_panic.rs`): the one boundary, a catch
//! inside a closure handed to it, a test's own catch, and three bare catches the guard refuses.

pub mod contained_panic {
    /// The boundary itself: its catch is the one the guard allows.
    pub fn contain<T>(work: impl FnOnce() -> T) -> std::thread::Result<T> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
    }
}

/// Inside a closure handed to the boundary: allowed.
pub fn nested() -> bool {
    crate::contained_panic::contain(|| std::panic::catch_unwind(|| 1).is_ok()).unwrap_or(false)
}

/// A bare catch: refused.
pub fn planted_bare() -> bool {
    std::panic::catch_unwind(|| 2).is_ok()
}

/// A catch imported under another name: refused at the import.
use std::panic::catch_unwind as quietly;

pub fn planted_renamed() -> bool {
    quietly(|| 3).is_ok()
}

/// A function named like the boundary in another module: refused.
pub mod elsewhere {
    pub fn contain() -> bool {
        std::panic::catch_unwind(|| 4).is_ok()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_test_may_catch() {
        assert!(std::panic::catch_unwind(|| 5).is_ok());
    }
}
