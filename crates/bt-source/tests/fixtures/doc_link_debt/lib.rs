//! The `doc_link_debt` fixture: one documented shape per marker, read by
//! `tests/doc_link_debt.rs` and never compiled. A crate doc naming [`crate_doc_marker`].

/// A function's doc naming [`free_fn_marker`].
pub fn free() {
    /// A local item's doc, inside a body: [`body_marker`].
    fn local() {}
    local();
}

/// A doc block whose link is on its second line:
/// [`second_line_marker`].
pub fn two_lines() {}

/// A struct's doc naming [`struct_marker`].
pub struct Pane {
    /// A field's doc naming [`field_marker`].
    pub width: u32,
}

/// An enum's doc.
pub enum Seat {
    /// A variant's doc naming [`variant_marker`].
    Pane,
}

impl Pane {
    /// An associated function's doc naming [`method_marker`].
    pub fn grow(&mut self) {}

    /// An associated const's doc naming [`assoc_const_marker`].
    pub const LIMIT: u32 = 3;
}

/// An impl block's doc naming [`impl_marker`].
impl Default for Pane {
    fn default() -> Self {
        Self { width: 0 }
    }
}

/// A trait's doc naming [`trait_marker`].
pub trait Grow {
    /// An associated type's doc naming [`assoc_type_marker`].
    type Step;

    /// A default method's doc naming [`default_method_marker`].
    fn grow_by(&mut self) {}
}

/// A const's doc naming [`const_marker`].
pub const WIDTH: u32 = 80;

/// A static's doc naming [`static_marker`].
pub static NAME: &str = "fixture";

/// An alias's doc naming [`alias_marker`].
pub type Width = u32;

/// A gated function's doc naming [`gated_marker`].
#[cfg(windows)]
pub fn gated() {}

/// An inline module's outer doc naming [`outer_module_marker`].
pub mod inner {
    //! An inline module's inner doc naming [`inner_module_marker`].

    /// A function of the inline module naming [`inner_fn_marker`].
    pub fn inside() {}
}

/// A macro's doc naming [`macro_doc_marker`].
macro_rules! make {
    () => {
        /// A doc inside a macro's tokens naming [`macro_tokens_marker`].
        pub struct Made;
    };
}

make!();

// A plain comment naming [`plain_comment_marker`].
pub fn after_comment() {}
