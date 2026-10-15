//! **The window seam's guard** — its fixture.
//!
//! ```text
//! window_seam   the seam: `Backend`, the door `WindowHost`, the native backend and its door
//! (root)        `Runtime`, `Shell` and `Pair`, which hold a backend; `Plain`, which does not;
//!               `Indirect`, bound through a supertrait; the listed native method `create`, the
//!               listed function `listed_rebuild` and the listed `const`; a test module's native
//!               block; and the PLANTED impls below module level inside `create` and
//!               `listed_rebuild` (a door, a native holder block, a generic holder block and a
//!               non-holder block reaching a native-only method, a trait impl for a native holder)
//! runtime       the generic roads (named directly, through a glob, through a rename, in a
//!               `where` clause), the passing controls (a holder built by inference inside a
//!               generic function, an enum's variant spelled as the backend, a trait's default
//!               bodies written for its own scope), and every other PLANTED shape, each marked
//!               where it is written
//! elsewhere     the PLANTED door implemented outside the seam, and a renamed re-export of the
//!               door (`Host`) that bodies reach
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
        // PLANTED: a generic holder block below module level whose method reaches a native-only
        // method.
        impl<B: Backend> Runtime<'_, B> {
            fn again() {
                Runtime::create()
            }
        }
        // PLANTED: a trait impl for a reference to a native holder, below module level.
        impl Clone for &Runtime<'_, Native> {
            fn clone(&self) -> Self {
                self
            }
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
    // PLANTED: a non-holder's block below module level whose method reaches a native-only method.
    impl crate::Plain {
        fn road() {
            Runtime::create()
        }
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
