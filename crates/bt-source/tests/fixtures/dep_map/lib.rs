// The dependency-map fixture: a crate root with free functions and `Runtime`
// methods still in it, a model, and two runtime topics.
mod model;
mod runtime;

use model::Runtime;

// Called from nowhere; calls into a topic.
pub fn wire(runtime: &mut Runtime) {
    runtime::tabs::retab(runtime);
    settle(runtime);
}

// Called by `wire` and by `Runtime::zoom`.
pub fn settle(runtime: &mut Runtime) {
    runtime.leaf.seen = 0;
}

impl Runtime {
    // A `Runtime` method still in the crate root, called by a topic.
    pub fn refresh(&mut self) -> u32 {
        self.leaf.text_scale
    }
}
