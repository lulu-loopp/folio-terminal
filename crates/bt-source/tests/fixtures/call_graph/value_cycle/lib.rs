//! Function-pointer values whose initializers name the value again, and an associated one.

fn target() {}

pub const F: fn() = {
    fn g() {
        F()
    }
    g
};

pub static S: fn() = || S();

pub fn root() {
    F();
    S();
}

pub struct Holder;

impl Holder {
    pub const H: fn() = target;

    pub fn call(&self) {
        Self::H();
        Holder::H();
    }
}
