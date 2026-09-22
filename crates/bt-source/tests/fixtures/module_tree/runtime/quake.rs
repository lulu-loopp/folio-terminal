// A grandchild file, and an inline module inside it: a tree reaches both, and
// a member that stopped at the child would answer about neither.

pub const IN_A_GRANDCHILD: &str = "the_needle_the_union_counts";

pub mod deeper {
    pub const DEEPER_STILL: &str = "the_needle_the_union_counts";
}
