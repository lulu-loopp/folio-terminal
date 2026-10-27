// The other package: the types the subject implements its own traits on.
pub struct Session {
    pub n: u32,
}

impl Session {
    // The type's own method: what the subject's forward calls.
    pub fn set(&mut self) {
        self.n = 1;
    }
}

// A type named like a standard one: written bare, `Cell` is both.
pub struct Cell {
    pub n: u32,
}
