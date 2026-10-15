//! **The window seam's guard** — its fixture.
//!
//! ```text
//! window_seam   the seam: `Backend`, the door `WindowHost`, the native backend and its door
//! (root)        `Runtime`, `Shell` and `Pair`, which hold a backend; `Plain`, which does not;
//!               `Indirect`, bound through a supertrait; the listed native method, function
//!               and `const`; a test module's native block
//! runtime       the generic roads (named directly, through a glob, through a rename, in a
//!               `where` clause), the passing controls (a holder built by inference inside a
//!               generic function, an enum's variant spelled as the backend), and every PLANTED
//!               shape the guard refuses, each marked where it is written
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

/// A holder whose bound is the backend's supertrait.
pub struct Indirect<B: window_seam::ShellBackend> {
    backend: B,
}

/// A holder whose fields are a tuple's.
pub struct Pair<B: Backend = Native>(B::Window);

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
    // PLANTED: a door of the seam implemented inside a listed native method.
    fn create() {
        struct Made;
        impl window_seam::WindowHost for Made {
            fn request_redraw(&self) {}
        }
    }
}

fn generic_over<B: Backend>(_runtime: &Runtime<'_, B>) {}

/// Listed: a native function over native holders.
fn listed_rebuild(_windows: Vec<&mut Runtime<'_>>) {
    // PLANTED: a native block of a holder written inside a listed function.
    impl Runtime<'_, Native> {
        fn hidden_in_a_listed_function() {}
    }
}

/// Listed: a compile-time check of a native holder.
const _: fn(Runtime<'_>) = |Runtime { window: _ }| ();

#[cfg(test)]
mod tests {
    impl crate::Runtime<'_, crate::window_seam::Native> {
        fn a_test_may_name_the_native_window() {}
    }
}
