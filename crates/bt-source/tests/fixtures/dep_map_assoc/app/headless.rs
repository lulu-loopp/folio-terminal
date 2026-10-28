// The tests' backend: no product build has it, so its bindings are not the
// product's.
use crate::seam::{Backend, Device, Glass};

pub struct Headless;

pub struct HeadlessGlass;

pub struct HeadlessGpu;

impl Backend for Headless {
    type Glass = HeadlessGlass;
    type Device = HeadlessGpu;
}

impl Glass for HeadlessGlass {
    type Device = HeadlessGpu;

    fn paint(&self) -> u32 {
        0
    }
}

impl Device for HeadlessGpu {
    fn dev(&self) -> u32 {
        0
    }
}
