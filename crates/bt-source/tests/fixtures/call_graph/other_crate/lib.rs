//! The second crate the `paths` fixture calls into.

pub mod tools {
    pub fn far() {}
}

pub use tools::far as again;
