//! What `Dialog` reaches outside the guarded scope: no list names this module.

/// (1) A label defined outside the scope. PLANTED where `Dialog::from_a_const` names it.
pub const OUTSIDE_LABEL: &str = "Copy from a const";

/// No word. PASSES.
pub const SEPARATOR: &str = ";";

/// A number. PASSES.
pub const OUTSIDE_ID: u32 = 7;

/// (2) A crate-local macro whose definition holds a word: PLANTED at its call.
macro_rules! outside_label {
    () => {
        "Copy from a macro"
    };
}

/// The same shape, on the guard's `MACROS` list. PASSES.
macro_rules! listed_label {
    () => {
        "Copy from a listed macro"
    };
}

/// A macro with no word. PASSES.
macro_rules! no_words {
    () => {
        1 + 1
    };
}

/// (3) A helper on none of the lists that builds the label. PLANTED.
pub fn helper_label() -> &'static str {
    "Copy from a helper"
}

/// A helper whose literal has a row (an entity, written `&amp;`). PASSES.
pub fn helper_entity() -> &'static str {
    "&amp;"
}

/// A first-party trait a guarded item calls through a trait object.
pub trait Named {
    fn name(&self) -> &'static str;
}

pub struct Formula;

impl Named for Formula {
    /// (3) Reached through `&dyn Named`. PLANTED.
    fn name(&self) -> &'static str {
        "Copy through a trait object"
    }
}

/// A first-party `push`: a candidate of every `.push` on a receiver of no known type.
pub struct Stack;

impl Stack {
    /// Reached only as an ambiguous candidate. PLANTED unless a `BOUNDARY` row stops the walk.
    pub fn push(&mut self, _row: u32) -> &'static str {
        "Copy from an ambiguous candidate"
    }
}
