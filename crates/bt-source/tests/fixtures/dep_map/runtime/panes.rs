// A topic calling the model's door by an imported name, the other topic by
// its path, and the crate root by `crate::`.
use crate::model::{Runtime, step_text_scale};

impl Runtime {
    pub fn zoom(&mut self) {
        step_text_scale(&mut self.leaf);
        super::tabs::retab(self);
        crate::settle(self);
        self.refresh();
    }

    pub fn grid_a(&mut self) {
        self.leaf.grid = 1;
    }
}

// A second `helper`: a glob over both modules makes the name ambiguous.
pub fn helper() {}
