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

mod plain;

#[cfg(test)]
mod gate;

#[cfg(test)]
#[path = "named.rs"]
mod by_path;

pub fn at_the_root() -> u8 {
    0
}

#[cfg(test)]
mod inline_gate {
    pub fn inside_the_braces() -> u8 {
        1
    }
}
