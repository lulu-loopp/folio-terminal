//! A type written through an alias, and an alias of an alias.

pub struct Real;

impl Real {
    pub fn act(&self) {}
}

pub type Short = Real;

#[cfg(windows)]
pub type Shorter = Short;

pub fn root(value: &Shorter) {
    value.act();
}

pub trait Assoc {
    type Out;
}

impl Real {
    fn inside() {
        type Local = u8;
    }
}
