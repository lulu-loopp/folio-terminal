// A topic calling a method of another topic through its receiver's type, and
// a name two glob imports bring in.
use crate::model::*;
use crate::runtime::panes::*;

pub fn retab(runtime: &mut Runtime) {
    runtime.zoom();
}

// The grid's third writer: no door for the grid.
pub fn grid_b(runtime: &mut Runtime) {
    runtime.leaf.grid = 2;
}

// The second writer of `seen`, beside `settle`: both are doors.
pub fn mark(runtime: &mut Runtime) {
    runtime.leaf.seen = 3;
}

pub fn ambiguous() {
    helper();
}
