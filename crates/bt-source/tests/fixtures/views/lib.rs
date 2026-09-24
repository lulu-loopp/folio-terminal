//! A fixture for the reading contract of §2: hidden_in_a_doc_comment.
//!
//! Every needle in this file is spelled once and on purpose, so that a count is
//! a fact about the rule under test rather than about the fixture.

// A line comment: hidden_in_a_line_comment

/* A block comment: hidden_in_a_block_comment */

#[path = "second.rs"]
mod second;

/// The door §2.5 is written about — product code, with a declaration the guard
/// exempts and neighbours that a substring count would take for it.
pub struct NativeWindow;

impl NativeWindow {
    pub const fn stand_in() -> u8 {
        0
    }
}

/// `strip_stand_in` and `stand_in_window` both exist, which is why the guard
/// checks an identifier boundary on **both** sides.
pub fn strip_stand_in() -> u8 {
    1
}

pub fn stand_in_window() -> u8 {
    2
}

pub const ESCAPED: &str = "a\nb";
pub const RAW: &str = r"a\nb";
pub const BYTES: &[u8] = b"a_byte_string";

/// Documented, and kept out of the rendered documentation.
#[doc(hidden)]
pub fn hidden_but_not_a_comment() -> u8 {
    3
}

pub fn through_a_macro() {
    println!("{}", NativeWindow::stand_in());
}

mod inner {
    pub fn only_inside_inner() -> u8 {
        4
    }
}

#[cfg(windows)]
pub fn two_arms() -> u8 {
    6
}

#[cfg(not(windows))]
pub fn two_arms() -> u8 {
    7
}

// **This pair is deliberately not legal Rust**, and this fixture is read and
// never compiled: two declarations of one name standing on the *same*
// predicate are two things, not two arms of one identity, and `OnePerVariant`
// has to say so rather than pick either.
#[cfg(windows)]
pub fn same_predicate_twice() -> u8 {
    8
}

#[cfg(windows)]
pub fn same_predicate_twice() -> u8 {
    9
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_door_is_only_named_by_tests() {
        // Written out rather than inside `assert_eq!`, so that this call is one
        // the parser places and the call in `through_a_macro` is not.
        let door = NativeWindow::stand_in();
        let stripped = strip_stand_in();
        let windowed = stand_in_window();
        assert_eq!((door, stripped, windowed), (0, 1, 2));
        assert_eq!(inner::only_inside_inner(), 4);
    }
}
