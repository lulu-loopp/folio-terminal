// A type of the subject named like the other package's `Session`, with its
// own `Ports`: a call on the other package's `Session` never reaches it.
use crate::ports::Ports;

pub struct Session {
    pub n: u32,
}

impl Ports for Session {
    fn port(&mut self) {
        self.n = 3;
    }
}
