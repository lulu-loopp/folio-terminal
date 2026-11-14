// The subject's trait on a type outside the workspace, called by the type's
// path: the language picks the outside type's own `port` first when it has
// one, and the census does not read the outside type.
use outside_crate::Thing;

use crate::ports::Ports;

impl Ports for Thing {
    fn port(&mut self) {}
}

pub fn outside(thing: &mut Thing) {
    Thing::port(thing);
}
