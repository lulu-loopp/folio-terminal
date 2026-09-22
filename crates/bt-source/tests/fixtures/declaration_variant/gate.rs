// Nothing in this file writes a `cfg` on itself, and no build of the shipped
// program contains any of it: the declaration that reaches it is
// `#[cfg(test)] mod gate;`.

#[path = "plain.rs"]
mod plain_again;

// Outside every item, in a file no product build compiles.
pub const OUTSIDE_EVERY_ITEM_HERE: &str = "the_needle_this_fixture_counts";

pub fn reached_by_a_gate() -> u8 {
    let _ = "the_needle_this_fixture_counts";
    3
}

// The two halves at once: `test` from the declaration, `windows` from here.
#[cfg(windows)]
pub fn gated_twice() -> u8 {
    4
}
