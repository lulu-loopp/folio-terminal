//! Method calls on stated receivers.

use std::rc::Rc;
use std::sync::Arc;

pub struct Engine {
    part: Part,
}

pub struct Part;

impl Part {
    pub fn spin(&self) {}
}

impl Engine {
    pub fn new() -> Engine {
        Engine { part: Part }
    }

    pub fn run(&self) {
        self.part.spin();
        self.idle();
    }

    pub fn idle(&self) {}
}

pub struct Other;

impl Other {
    pub fn spin(&self) {}
}

pub fn parameter(engine: &Engine) {
    engine.run();
}

pub fn annotated() {
    let engine: Engine = make();
    engine.run();
}

pub fn carried() {
    let engine = Engine::new();
    engine.run();
}

pub fn pointers(boxed: Box<Part>, counted: Rc<Part>, shared: Arc<Part>) {
    boxed.spin();
    counted.spin();
    shared.spin();
}

pub fn shadowed(part: &Part, other: &Other) {
    let part = other;
    part.spin();
}
