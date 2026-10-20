//! A method reached through two first-party `Deref` steps: `Outer` -> `Middle` -> `Inner`.

use std::ops::Deref;

pub struct Inner;

impl Inner {
    pub fn only_on_inner(&self) {}
}

pub struct Middle(Inner);

impl Deref for Middle {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

pub struct Outer(Middle);

impl Deref for Outer {
    type Target = Middle;
    fn deref(&self) -> &Middle {
        &self.0
    }
}

pub fn root(outer: &Outer) {
    outer.only_on_inner();
}
