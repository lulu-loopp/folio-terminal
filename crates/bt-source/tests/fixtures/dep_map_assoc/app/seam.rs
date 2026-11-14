// The backend seam: the traits a window is written over, the product's one
// backend, and the forwards on the other package's types. `Device` is
// declared by two traits and bound by both impls, to one type.
use term_fixture::{Gpu, Renderer};

pub trait Device {
    fn dev(&self) -> u32;
}

pub trait Glass {
    type Device: Device;

    fn paint(&self) -> u32;
}

pub trait Backend {
    type Glass: Glass<Device = Self::Device>;
    type Device: Device;
}

pub struct Native;

impl Backend for Native {
    type Glass = Renderer;
    type Device = Gpu;
}

// The forward: `self.paint()` is the renderer's own inherent method, which
// the language picks first here; a call through `B::Glass` reaches this one.
impl Glass for Renderer {
    type Device = Gpu;

    fn paint(&self) -> u32 {
        self.paint()
    }
}

impl Device for Gpu {
    fn dev(&self) -> u32 {
        self.dev()
    }
}
