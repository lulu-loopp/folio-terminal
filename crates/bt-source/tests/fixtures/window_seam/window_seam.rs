pub trait WindowHost {
    fn request_redraw(&self);
}

pub trait Backend {
    type Window: WindowHost;
}

pub trait ShellBackend: Backend {}

pub struct Native;

pub struct NativeWindow;

impl WindowHost for NativeWindow {
    fn request_redraw(&self) {}
}

impl Backend for Native {
    type Window = NativeWindow;
}

pub mod prelude {
    pub use super::{Backend, WindowHost};
}

// PLANTED: a seam type implementing a trait reached past the resolver's hop limit — the native
// list's decision must refuse it, not read it as "not the backend".
pub struct Beyond;

impl crate::elsewhere::Bk20 for Beyond {
    type Window = NativeWindow;
}

// PLANTED: a door handing the platform handle out.
pub trait HandleDoor {
    fn handle(&self) -> bt_platform::NativeWindow;
}
