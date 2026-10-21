//! Qualified calls `<X as Trait>::m`.

pub trait Speak {
    fn speak(&self);
    fn hum(&self) {}
}

pub struct Dog;
pub struct Cat;

impl Speak for Dog {
    fn speak(&self) {}
}

impl Speak for Cat {
    fn speak(&self) {}
    fn hum(&self) {}
}

pub fn root(dog: &Dog) {
    <Dog as Speak>::speak(dog);
    <Dog as Speak>::hum(dog);
}

pub fn generic<T: Speak>(value: &T) {
    <T as Speak>::speak(value);
}
