// A topic calling through the backend's associated types: by a method call on
// a field, on a parameter of a generic bounded by the other trait, and by the
// associated type's path.
use crate::seam::{Backend, Device, Glass};

pub struct Window<B: Backend> {
    pub glass: B::Glass,
    pub gpu: B::Device,
}

impl<B: Backend> Window<B> {
    pub fn draw(&self) -> u32 {
        self.glass.paint() + self.gpu.dev()
    }

    pub fn draw_by_path(&self) -> u32 {
        B::Glass::paint(&self.glass)
    }
}

pub fn measure<G: Glass>(gpu: &G::Device) -> u32 {
    gpu.dev()
}
