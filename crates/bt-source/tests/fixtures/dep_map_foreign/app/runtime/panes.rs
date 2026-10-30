// A topic calling the subject's traits on the other package's types: by a
// method call and by the type's path.
use crate::Runtime;
use crate::aliased::Aliased;
use crate::imported::Other;
use crate::ports::{Nudge, Ports};

pub fn poke(runtime: &mut Runtime) {
    runtime.leaf.session.port();
    term_fixture::Session::port(&mut runtime.leaf.session);
}

pub fn prod(runtime: &mut Runtime) {
    runtime.leaf.cell.nudge();
}

pub fn elsewhere(runtime: &mut Runtime) {
    runtime.leaf.session.other();
}

// `rename` is the other package's `Inner`'s, through `Session`'s deref; only
// the subject's own `Session` declares a `rename` of its own.
pub fn renamed(runtime: &mut Runtime) {
    runtime.leaf.session.rename();
}

pub fn qualified(runtime: &mut Runtime) {
    <term_fixture::Session as Ports>::port(&mut runtime.leaf.session);
}

pub fn renamed_crate(runtime: &mut Runtime) {
    runtime.leaf.session.aliased();
}
