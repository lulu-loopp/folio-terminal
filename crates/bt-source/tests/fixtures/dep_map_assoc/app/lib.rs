// The associated-type fixture's subject: a backend seam whose one product impl
// binds each associated type, the bound traits' impls on the bound types (the
// forwards), a test-only backend, associated types the census cannot follow to
// one implementation, and a topic calling through them.
mod lanes;
mod runtime;
mod seam;

#[cfg(test)]
mod headless;

pub struct Leaf {
    pub n: u32,
}

pub struct Runtime {
    pub leaf: Leaf,
}
