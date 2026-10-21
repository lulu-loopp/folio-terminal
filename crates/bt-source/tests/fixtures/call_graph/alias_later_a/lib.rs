//! An `impl` on an alias declared in a later package.

pub trait Speak {
    fn speak(&self);
}

impl Speak for cg_alias_later_b::Short {
    fn speak(&self) {}
}

pub fn root(value: &cg_alias_later_b::Real) {
    value.speak();
}
