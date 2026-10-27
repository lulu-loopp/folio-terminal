// The nesting fixture, before: a leaf whose two view fields stand on it
// beside one field of its own. `nested_after` is the same program with the
// two fields moved under `Leaf.view` and nothing else changed.

pub struct Leaf {
    pub seen: u64,
    pub clock: u32,
    pub skip: usize,
}

// An assignment.
pub fn wake(leaf: &mut Leaf) {
    leaf.clock = 7;
}

// A compound assignment.
pub fn bump(leaf: &mut Leaf) {
    leaf.skip += 1;
}

// A `&mut` lend, which proves no write.
pub fn aim(leaf: &mut Leaf) {
    nudge(&mut leaf.skip);
}

fn nudge(skip: &mut usize) {
    *skip += 1;
}

// A read.
pub fn glance(leaf: &Leaf) -> u32 {
    leaf.clock
}

// A write of the leaf's own field from a read of a view field.
pub fn note(leaf: &mut Leaf) {
    leaf.seen = u64::from(leaf.clock);
}
