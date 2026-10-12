//! `FunctionPaths`' fixture, the platform crate: the item a re-export elsewhere names, and a
//! function of the same name in another module.

pub mod boundary {
    pub fn contain() {}
}

pub mod other {
    pub fn contain() {}
}
