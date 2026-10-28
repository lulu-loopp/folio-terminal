// A type of the subject named like the other package's `Session`, with its
// own `Ports` and its own `rename`: a call on the other package's `Session`
// never reaches either.
use crate::ports::Ports;

pub struct Session {
    pub n: u32,
}

impl Session {
    pub fn rename(&mut self) {
        self.n = 4;
    }
}

impl Ports for Session {
    fn port(&mut self) {
        self.n = 3;
    }
}
