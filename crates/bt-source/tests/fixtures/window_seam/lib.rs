//! **The window seam's guard** — its fixture.
//!
//! ```text
//! window_seam   the seam: `Backend`, the door `WindowHost`, the native backend and its door
//! (root)        `Runtime` and `Shell`, which hold a backend; `Plain`, which does not
//! runtime       the generic road (named directly, through a glob and through a rename), the
//!               native-only road on the list, and the PLANTED native block with a method off it
//! elsewhere     the PLANTED door implemented outside the seam
//! ```

mod elsewhere;
mod runtime;
mod window_seam;

use window_seam::{Backend, Native};

pub struct Runtime<'a, B: Backend = Native> {
    window: &'a B::Window,
}

pub struct Shell<B>
where
    B: window_seam::Backend,
{
    window: B::Window,
}

pub struct Plain;

impl Plain {
    fn anything(&self) {}
}

impl<B: Backend> Runtime<'_, B> {
    fn redraw(&self) {
        self.window.request_redraw();
    }
}

impl Runtime<'_, Native> {
    fn create() {}
}

#[cfg(test)]
mod tests {
    impl crate::Runtime<'_, crate::window_seam::Native> {
        fn a_test_may_name_the_native_window() {}
    }
}
