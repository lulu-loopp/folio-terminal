// The nesting fixture, after: `nested_before` with its two view fields moved
// under `Leaf.view` (the ownership contract's M1 shape) and nothing else
// changed.

pub struct Leaf {
    pub seen: u64,
    pub view: View,
}

pub struct View {
    pub clock: u32,
    pub skip: usize,
}

// An assignment.
pub fn wake(leaf: &mut Leaf) {
    leaf.view.clock = 7;
}

// A compound assignment.
pub fn bump(leaf: &mut Leaf) {
    leaf.view.skip += 1;
}

// A `&mut` lend, which proves no write.
pub fn aim(leaf: &mut Leaf) {
    nudge(&mut leaf.view.skip);
}

fn nudge(skip: &mut usize) {
    *skip += 1;
}

// A read.
pub fn glance(leaf: &Leaf) -> u32 {
    leaf.view.clock
}

// A write of the leaf's own field from a read of a view field.
pub fn note(leaf: &mut Leaf) {
    leaf.seen = u64::from(leaf.view.clock);
}
