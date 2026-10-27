// A backend seam: a field typed by a generic's associated type, and a call
// through it, which reaches whichever implementation the backend names.
pub trait Shine {
    fn shine(&self) -> u32;
}

pub trait Backend {
    type Glass: Shine;
}

pub struct Window<B: Backend> {
    pub glass: B::Glass,
}

impl<B: Backend> Window<B> {
    pub fn paint(&self) -> u32 {
        self.glass.shine()
    }
}

pub struct Pane;

impl Shine for Pane {
    fn shine(&self) -> u32 {
        1
    }
}
