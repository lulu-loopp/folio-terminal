// The other package: the types the subject implements its own traits on.
use std::ops::{Deref, DerefMut};

pub struct Session {
    pub n: u32,
    pub inner: Inner,
}

impl Session {
    // The type's own method: what the subject's forward calls.
    pub fn set(&mut self) {
        self.n = 1;
    }
}

// What a `Session` derefs to: `rename` is reached through it.
pub struct Inner {
    pub name: u32,
}

impl Inner {
    pub fn rename(&mut self) {
        self.name = 1;
    }
}

impl Deref for Session {
    type Target = Inner;

    fn deref(&self) -> &Inner {
        &self.inner
    }
}

impl DerefMut for Session {
    fn deref_mut(&mut self) -> &mut Inner {
        &mut self.inner
    }
}

// A type named like a standard one: written bare, `Cell` is both.
pub struct Cell {
    pub n: u32,
}
