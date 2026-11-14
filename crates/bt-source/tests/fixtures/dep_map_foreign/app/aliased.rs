// The subject's trait on the other package's `Session`, by a path through a
// renamed crate: `t::Session`. The census's rules do not read `use … as`, and
// `t` names no module, so the path is not placed by package (it would be the
// subject's own `Session` in `twin.rs`).
use term_fixture as t;

pub trait Aliased {
    fn aliased(&mut self);
}

impl Aliased for t::Session {
    fn aliased(&mut self) {
        self.set();
    }
}
