use crate::window_seam::WindowHost as Host;

pub struct Stray;

// PLANTED: a door of the seam implemented outside it.
impl Host for Stray {
    fn request_redraw(&self) {}
}
