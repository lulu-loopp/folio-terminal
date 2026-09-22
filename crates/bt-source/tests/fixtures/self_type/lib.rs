//! One owner, spelled three ways — the shape a move makes (§2.4).
//!
//! `Runtime` is declared here, and its methods are written in three places: this
//! file, a newly declared submodule that has to name the type `crate::Runtime`,
//! and a sibling that reaches it as `super::Runtime`. A relocation is exactly
//! when the second and third spellings appear, so all three are the owner
//! `Runtime` and every pin on a moved method goes on answering.

mod peek;
mod runtime;

pub struct Runtime<'a> {
    pub name: &'a str,
}

impl Runtime<'_> {
    /// The method that did not move: written where the type is.
    pub fn turn_stays_in_the_root(&self) -> usize {
        self.name.len()
    }
}
