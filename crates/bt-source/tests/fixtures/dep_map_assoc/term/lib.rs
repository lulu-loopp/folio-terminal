// The other package: the types the subject's backend binds, each with an
// inherent method named like the subject's trait method it implements.
pub struct Renderer {
    pub n: u32,
}

impl Renderer {
    pub fn paint(&self) -> u32 {
        self.n
    }
}

pub struct Gpu {
    pub n: u32,
}

impl Gpu {
    pub fn dev(&self) -> u32 {
        self.n
    }
}
