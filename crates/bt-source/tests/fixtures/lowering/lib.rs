//! A fixture with one of everything the lowering has to get right.
//!
//! The doc comment above is a comment, not three string literals.

// A line comment naming a needle: hidden_only_in_a_comment

/* A block /* nested */ comment naming another: hidden_only_in_a_block */

#[path = "twin.rs"]
mod twin;

/// A free function with a doc comment and a string that looks like a comment.
pub fn plain() -> &'static str {
    "// stand_in is not a comment here"
}

pub struct Door<'a> {
    pub name: &'a str,
}

impl Door<'_> {
    /// Its lifetime is written one way here and another way in the twin impl,
    /// and both are `Door`.
    pub fn open(&self) -> usize {
        self.name.len()
    }
}

impl<'a> Door<'a> {
    pub fn shut(&self) -> usize {
        0
    }
}

pub trait Latch {
    /// A signature with no body at all.
    fn required(&self);

    fn spare(&self) -> u8 {
        7
    }
}

impl Latch for Door<'_> {
    fn required(&self) {
        let r#type = 1u8;
        let _ = r#type;
    }
}

#[cfg(windows)]
pub fn only_one_arm() -> u8 {
    1
}

#[cfg(not(windows))]
pub fn only_one_arm() -> u8 {
    2
}

mod inner {
    pub fn nested() -> &'static str {
        // Two facts, not one: the integer below decodes to a value no byte of
        // this file spells, and the string beside it decodes to itself.
        let _mask: u8 = 0xFF;
        "0xFF"
    }
}

/// Documented, and kept out of the rendered documentation. The line below is an
/// attribute and not documentation text, so it stays in the code view.
#[doc(hidden)]
pub fn hidden_but_not_a_comment() -> u8 {
    3
}

pub fn through_a_macro() {
    println!("{}", stand_in_inside_a_macro());
}

fn stand_in_inside_a_macro() -> u8 {
    0
}
