//! Callees a derive or `#[from]` generates.

#[derive(Clone, Debug)]
pub struct Copyable;

pub enum Failure {
    Io(#[from] std::io::Error),
}

pub fn root(value: &Copyable, error: std::io::Error) {
    let _ = value.clone();
    let _ = Failure::from(error);
}
