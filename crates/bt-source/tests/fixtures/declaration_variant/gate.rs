// Nothing in this file writes a `cfg` on itself, and no build of the shipped
// program contains any of it: the declaration that reaches it is
// `#[cfg(test)] mod gate;`.

#[path = "plain.rs"]
mod plain_again;

pub fn reached_by_a_gate() -> u8 {
    3
}

// The two halves at once: `test` from the declaration, `windows` from here.
#[cfg(windows)]
pub fn gated_twice() -> u8 {
    4
}
