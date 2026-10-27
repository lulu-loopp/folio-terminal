//! **The root-topic-paths guard** — its fixture.
//!
//! ```text
//! (root)    the PLANTED paths into a topic module (T1 a `use`, T2 a full path with no `use`,
//!           T5 a group, T7 a rename and a glob, T8 a statement's own gate, T9 a macro's
//!           argument) and the passing controls (T3 a test-gated `use`, T4 the re-export road,
//!           T10 a doc link and a string, a field gated to tests)
//! other     a module outside `runtime/`: T6's shapes — a field type, an `impl` header, UFCS
//! t         a file reached by `#[cfg(test)] mod t;`: T12, accepted
//! runtime   `pub(crate) mod clipboard;`, `pub(crate) mod pointer;`, a private `mod quiet;`, and
//!           the re-export `exported`; inside it, T11's sibling path is accepted
//! ```

mod other;
mod runtime;
#[cfg(test)]
mod t;

// T1 — PLANTED: a non-test `use` into a topic.
use runtime::clipboard::copy;

// T3 — a test-gated `use` into a topic: accepted (row 77 is not inherited).
#[cfg(test)]
use runtime::clipboard::paste;

// T4 — the re-export road: accepted.
use runtime::exported;

// T5 — PLANTED: a group, one arm of it into a topic.
use runtime::{exported as again, pointer::Press};

// T7 — PLANTED: a rename that hides the head, and a glob over runtime's names.
use crate::runtime as rt;
use crate::runtime::*;

/// T10 — a doc link is not code: [`runtime::clipboard::copy`].
pub fn the_road() -> u8 {
    // T2 — PLANTED: a full path, no `use`.
    let first = runtime::clipboard::copy(1);
    // T4 — the re-export road, as a path.
    let second = runtime::exported(2);
    // T10 — a string is not code.
    let _ = "runtime::clipboard::copy";
    first + second + again(0)
}

pub fn gated_statement() -> u8 {
    // T8 — PLANTED: a statement's own gate, which the item grain does not read.
    #[cfg(test)]
    let a = runtime::clipboard::copy(0);
    #[cfg(not(test))]
    let a = 0;
    a
}

pub fn in_a_macro() -> Vec<u8> {
    // T9 — PLANTED: a path among a macro's arguments.
    vec![runtime::clipboard::copy(3)]
}

pub struct Holder {
    /// A field gated to tests: the item grain reads a field's own gate, accepted.
    #[cfg(test)]
    pub tested: runtime::pointer::Press,
    pub kept: u8,
}

pub fn reaches_the_private_one() -> u8 {
    // T2's shape into a private topic — PLANTED (the topic set is every child module).
    runtime::quiet::hush()
}

#[cfg(test)]
mod tests {
    // A test module's path: accepted.
    fn reads() -> u8 {
        super::runtime::clipboard::copy(4)
    }
}
