//! Constructors are calls in syntax and no code: resolved, never walked.

pub struct Wrap(u8);

pub enum Choice {
    One(u8),
    Two,
}

pub fn root() {
    let _ = Wrap(1);
    let _ = Choice::One(2);
    let _ = Some(3);
}
