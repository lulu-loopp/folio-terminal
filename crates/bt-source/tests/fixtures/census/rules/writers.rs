use crate::model::Runtime;

impl Runtime<'_> {
    // The dirty gate's opener.
    pub fn raise(&mut self) {
        self.window.dirty_gate.open(1);
    }
}
