// Associated types the census follows to no one implementation: bound to two
// types, bound to an impl's own generic, bound by no product impl, and a bound
// trait's provided method the bound type's impl does not write.
pub trait Speak {
    fn speak(&self) -> u32;
}

pub struct Loud;

pub struct Quiet;

impl Speak for Loud {
    fn speak(&self) -> u32 {
        2
    }
}

impl Speak for Quiet {
    fn speak(&self) -> u32 {
        1
    }
}

pub trait Owed {
    type Answer: Speak;
}

pub struct Asker;

pub struct Teller;

impl Owed for Asker {
    type Answer = Loud;
}

impl Owed for Teller {
    type Answer = Quiet;
}

pub fn ask<O: Owed>(answer: &O::Answer) -> u32 {
    answer.speak()
}

pub trait Carry {
    type Load: Speak;
}

pub struct Boxed<A>(pub A);

impl<A: Speak> Carry for Boxed<A> {
    type Load = A;
}

pub fn carry<C: Carry>(load: &C::Load) -> u32 {
    load.speak()
}

pub trait Lone {
    type Part: Speak;
}

pub fn lone<L: Lone>(part: &L::Part) -> u32 {
    part.speak()
}

pub trait Glow {
    fn glow(&self) -> u32 {
        1
    }
}

pub trait Lamp {
    type Bulb: Glow;
}

pub struct Bulb;

impl Glow for Bulb {}

impl Lamp for Asker {
    type Bulb = Bulb;
}

pub fn light<L: Lamp>(bulb: &L::Bulb) -> u32 {
    bulb.glow()
}

// A provided method no impl writes, called through an associated type the
// product impls bind to two types: refused, though no impl declares `hum`.
pub trait Hum {
    fn hum(&self) -> u32 {
        3
    }
}

impl Hum for Loud {}

impl Hum for Quiet {}

pub trait Choir {
    type Voice: Hum;
}

impl Choir for Asker {
    type Voice = Loud;
}

impl Choir for Teller {
    type Voice = Quiet;
}

pub fn choir<C: Choir>(voice: &C::Voice) -> u32 {
    voice.hum()
}
