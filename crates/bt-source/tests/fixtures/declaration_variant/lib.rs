//! **What a declaration says stands on every item in the file it reaches** —
//! §2.3's rule read into §2.4's identity.
//!
//! ```text
//! lib.rs   mod plain;                                     nothing stands on it
//!          #[cfg(test)] mod gate;                         a gate, out of line
//!          #[cfg(test)] #[path = "named.rs"] mod by_path;  the same, by #[path]
//!          #[cfg(test)] mod inline_gate { … }             the same, inline
//! gate.rs  #[path = "plain.rs"] mod plain_again;          a second, gated path
//!                                                          to a product file
//! ```
//!
//! The inline gate and the out-of-line one are the same statement written two
//! ways, so the two spellings have to answer alike; `plain.rs` is reached both
//! ways at once, which is where they have to answer differently.
//!
//! One needle, the string `at_the_root` returns to nobody, is written seven
//! times and spelled nowhere else — not even here: in every place a build of
//! the shipped program either does or does not reach. Inside an item and
//! outside every item, in a file a product build compiles and in one it does
//! not, under a gate written on the item and under one written on the
//! declaration that reaches the file.

mod plain;

#[cfg(test)]
mod gate;

#[cfg(test)]
#[path = "named.rs"]
mod by_path;

// Outside every item, in a file a product build compiles: the file is the only
// grain there is for these bytes.
pub const OUTSIDE_EVERY_ITEM: &str = "the_needle_this_fixture_counts";

pub fn at_the_root() -> u8 {
    let _ = "the_needle_this_fixture_counts";
    0
}

// A gate on the item itself, in a file a product build compiles: the file says
// yes and the item says no.
#[cfg(test)]
pub fn only_in_tests() -> u8 {
    let _ = "the_needle_this_fixture_counts";
    6
}

#[cfg(test)]
mod inline_gate {
    pub fn inside_the_braces() -> u8 {
        let _ = "the_needle_this_fixture_counts";
        1
    }
}
