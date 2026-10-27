//! **The root-topic-paths guard** — its fixture.
//!
//! ```text
//! (root)    the PLANTED paths into a topic module (T1 a `use`, T2 a full path with no `use`,
//!           T5 a group, T7 a rename and a glob, T8 a statement's own gate, T9 a macro's
//!           argument) and the passing controls (T3 a test-gated `use`, T4 the re-export road,
//!           T10 a doc link and a string, a field gated to tests)
//! other     a module outside `runtime/`: T6's shapes — a field type, an `impl` header, UFCS
//! (end)     F1's foreign heads (G2, G3, G1), F2's body group, glob and rename, a glob into a
//!           topic and a nested module's `use`, F3's comment inside a path
//! t         a file reached by `#[cfg(test)] mod t;`: T12, accepted
//! runtime   `pub(crate) mod clipboard;`, `pub(crate) mod pointer;`, a private `mod quiet;`, and
//!           the re-export `exported` (an item: accepted) and `inner` (a module: F4, refused);
//!           inside it, T11's sibling path is accepted
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

// F1 — PLANTED: a head the guard does not resolve, refused in each reading — a module-level
// `use` (G2), a group in a body (G3) and a path (G1).
use crate as c;
use c::runtime::pointer::Press as ForeignPress;

pub fn foreign_heads() -> u8 {
    use c::runtime::{pointer::press};
    c::runtime::pointer::press() + press()
}

// F2 — PLANTED: a group, a glob and a rename in a body (G3), each refused.
pub fn body_shapes() -> u8 {
    use runtime::{pointer::press};
    use runtime::*;
    use crate::runtime as body_rt;
    press() + body_rt::exported(6) + exported(7)
}

// F2 — PLANTED: a module-level glob into a topic (G2), a finding.
use runtime::pointer::*;

mod nested {
    // F2 — PLANTED: a module-level `use` from a nested module, its `super::` read past (G2).
    use super::runtime::pointer::Press;
}

pub fn commented() -> u8 {
    // F3 — PLANTED: a comment inside the path, which the path search does not match: refused.
    runtime::/* hidden */pointer::press()
}
