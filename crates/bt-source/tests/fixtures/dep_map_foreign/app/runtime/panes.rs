// A topic calling the subject's traits on the other package's types: by a
// method call and by the type's path.
use crate::Runtime;
use crate::ports::{Nudge, Ports};

pub fn poke(runtime: &mut Runtime) {
    runtime.leaf.session.port();
    term_fixture::Session::port(&mut runtime.leaf.session);
}

pub fn prod(runtime: &mut Runtime) {
    runtime.leaf.cell.nudge();
}
