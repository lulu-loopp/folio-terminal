//! **A root, a tree beside it, and a sibling whose name begins the same way** —
//! the shape a relocation leaves behind (§2.4).
//!
//! A guard whose claim is "in the crate root and everywhere under
//! `crate::runtime`" cannot say so with one exact path: the methods it is about
//! are written on both sides of the move. So the fixture writes one needle once
//! in each of the six places a member of a union can reach, and the counts below
//! are what tells the three shapes apart.
//!
//! ```text
//! crate                    the const beside this comment
//! crate::inline_child      a `mod x { … }`, whose bytes are inside this file
//! crate::runtime           runtime/mod.rs, which is a file of its own
//! crate::runtime::quake    a grandchild file, reached from that one
//! crate::runtime::quake::deeper   an inline module inside the grandchild
//! crate::runtimes          a sibling whose path starts with the tree's and is
//!                          not under it
//! ```

mod runtime;
mod runtimes;

pub const AT_THE_ROOT: &str = "the_needle_the_union_counts";

pub mod inline_child {
    pub const INSIDE_THE_BRACES: &str = "the_needle_the_union_counts";
}
