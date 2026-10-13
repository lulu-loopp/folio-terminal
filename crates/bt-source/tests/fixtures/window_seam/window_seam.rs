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
