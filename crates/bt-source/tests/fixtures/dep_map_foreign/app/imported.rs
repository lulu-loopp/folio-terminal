// The subject's trait on the other package's `Session`, by the name the `use`
// below brings in, while the subject declares a `Session` of its own
// (`twin.rs`). The census's rules place a bare type name without reading `use`
// items, and `Session` is not declared in this module: placed by package, it
// would be the twin, so this self type is not placed.
use term_fixture::Session;

pub trait Other {
    fn other(&mut self);
}

impl Other for Session {
    fn other(&mut self) {
        self.set();
    }
}
