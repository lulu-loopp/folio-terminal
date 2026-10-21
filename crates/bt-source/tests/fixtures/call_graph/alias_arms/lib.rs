//! One alias written on two arms, naming a different type on each.

pub struct Wide;
pub struct Narrow;

impl Wide {
    pub fn show(&self) {}
}

impl Narrow {
    pub fn show(&self) {}
}

#[cfg(windows)]
pub type Native = Wide;

#[cfg(not(windows))]
pub type Native = Narrow;

pub fn root(value: &Native) {
    value.show();
}
