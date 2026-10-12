// The struct whose fields are counted, and its doors.
pub struct Leaf {
    pub text_scale: u32,
    pub grid: u32,
    pub seen: u32,
    pub label: String,
}

pub struct Runtime {
    pub leaf: Leaf,
}

impl Leaf {
    // The type's own method: never a door, though it is the grid's third writer.
    pub fn reset(&mut self) {
        self.grid = 0;
    }
}

// The text scale's only writer: a door. It writes and does not read.
pub fn step_text_scale(leaf: &mut Leaf) {
    leaf.text_scale += 1;
}

// A read of the text scale, by value.
pub fn scale_of(leaf: &Leaf) -> u32 {
    leaf.text_scale
}

// A read by a pattern, and a `&self` method on the label.
pub fn describe(leaf: &Leaf) -> usize {
    let Leaf { label, .. } = leaf;
    label.len() + leaf.label.len()
}

// A name a topic declares too.
pub fn helper() {}

// A trait method of the same name as `Runtime`'s inherent `refresh`: a call
// `runtime.refresh()` reaches the inherent one, as the language picks.
pub trait Refresh {
    fn refresh(&mut self) -> u32;
}

impl Refresh for Runtime {
    fn refresh(&mut self) -> u32 {
        0
    }
}
