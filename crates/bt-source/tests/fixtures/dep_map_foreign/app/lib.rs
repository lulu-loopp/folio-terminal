// The foreign-trait fixture's subject: its own traits implemented on the
// other package's types, a type named like one of them, a backend seam, and
// a topic that calls them.
mod glass;
mod imported;
mod ports;
mod runtime;
mod twin;

pub struct Leaf {
    pub session: term_fixture::Session,
    pub cell: term_fixture::Cell,
    pub seen: u32,
}

pub struct Runtime {
    pub leaf: Leaf,
}
