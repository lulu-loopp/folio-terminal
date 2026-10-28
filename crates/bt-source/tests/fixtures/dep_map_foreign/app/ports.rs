// The subject's traits on the other package's types: the forwards live here.
use term_fixture::Cell;

pub trait Ports {
    fn port(&mut self);
}

// On the other package's `Session`, by its path.
impl Ports for term_fixture::Session {
    fn port(&mut self) {
        self.set();
    }
}

pub trait Nudge {
    fn nudge(&mut self);
}

// On the other package's `Cell`, by the name the `use` above brings in. The
// census's rules place a bare type name without reading `use` items, and
// `Cell` is a standard type's name too: this self type is not placed.
impl Nudge for Cell {
    fn nudge(&mut self) {
        self.n = 2;
    }
}
