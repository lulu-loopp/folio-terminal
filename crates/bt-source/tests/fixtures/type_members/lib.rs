//! **Types and their members** — §2.4's identity rule past the callables, with
//! a case for every way a member query is answered and every way it is refused.
//!
//! ```text
//! lib.rs     enum Edge { … }             variants: fieldless, named, tuple
//!            enum Numbered { … = 3 }     a discriminant list
//!            union Word { … }            a union's fields are fields
//! panes.rs   struct App { … }            the type a query finds by name in a
//!                                        submodule, and a method of the same
//!                                        name as one of its fields
//!            struct Wrapper(pub u32)     a tuple field, named by its position
//! gated.rs   #[cfg] struct Split { … }   two arms, one field in one of them
//! ```

mod gated;
mod panes;

pub enum Edge {
    Top,
    Bottom { inset: u8 },
    Corner(u8, u8),
}

#[repr(u8)]
pub enum Numbered {
    First = 1,
    Third = 3,
}

pub union Word {
    pub bytes: [u8; 4],
    pub number: u32,
}
