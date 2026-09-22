// The sibling, and the reason a tree is not a prefix match: `crate::runtimes`
// begins with `crate::runtime` and is no part of it. This is where the guard
// that widened to the whole universe inverts — the needle is spelled here for a
// register of this module's own.

pub const IN_THE_SIBLING: &str = "the_needle_the_union_counts";
