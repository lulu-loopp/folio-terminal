// The same program as `rules`, with the gate's opener moved into a newly
// declared module.
#[path = "../shared/gate.rs"]
mod gate;
#[path = "../shared/model.rs"]
mod model;
#[path = "../shared/others.rs"]
mod others;
mod doors;
