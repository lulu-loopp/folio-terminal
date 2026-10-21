//! An untyped receiver: every first-party method of the name, never a pick.

pub struct A;
pub struct B;

impl A {
    pub fn shared_name(&self) {}
    pub fn only_a(&self) {}
}

impl B {
    pub fn shared_name(&self) {}
}

pub fn root() {
    let value = make();
    value.shared_name();
    value.only_a();
    value.no_such_method();
}
