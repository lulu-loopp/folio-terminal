//! The data types a document's root reaches (0.4.10 plan row 68d): a field's type, a type
//! argument, a tuple variant's and a struct variant's payload, a type of another module named
//! through a `use`, an alias followed to what it names, and a cycle back to the root. A foreign
//! type is a leaf, and a type nothing reaches is not reached.

pub mod document {
    use crate::elsewhere::Imported;
    use std::path::PathBuf;

    pub struct Root {
        pub nodes: Vec<Node>,
        pub imported: Option<Imported>,
        pub path: PathBuf,
        pub aliased: Aliased,
    }

    pub enum Node {
        Leaf,
        Again(Box<Root>),
        Wrapped { inner: Wrapper<Payload> },
    }

    pub struct Wrapper<T> {
        pub value: T,
    }

    pub struct Payload(pub u32);

    pub type Aliased = Target;

    pub struct Target {
        pub n: u8,
    }

    pub struct NotReached {
        pub root: Root,
    }
}

pub mod elsewhere {
    pub struct Imported {
        pub n: u8,
    }
}

/// A callable, which is no data type.
pub fn nothing() {}
