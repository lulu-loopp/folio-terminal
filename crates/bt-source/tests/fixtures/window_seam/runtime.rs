use crate::window_seam::prelude::*;
use crate::window_seam::Backend as Seam;
use crate::window_seam::Native;
use crate::window_seam::ShellBackend;
use crate::{Indirect, Runtime, Shell};

impl<B: Backend> Runtime<'_, B> {
    fn present(&self) {
        self.window.request_redraw();
    }
}

impl<S> Shell<S>
where
    S: Seam,
{
    fn settle(&self) {
        self.window.request_redraw();
    }
}

impl<B: Backend> Clone for Shell<B> {
    fn clone(&self) -> Self {
        unimplemented!()
    }
}

// PLANTED: a native-only block holding a method the list does not name.
impl Runtime<'_, Native> {
    fn sneaks_past_the_seam(&self) {}
}

// PLANTED: a trait implemented for the native backend's holder alone.
impl Default for Shell<Native> {
    fn default() -> Self {
        unimplemented!()
    }
}

impl<B: ShellBackend> Indirect<B> {
    fn fine(&self) {}
}

// PLANTED: a native block of a holder bound through the backend's supertrait.
impl Indirect<Native> {
    fn sneaks_through_a_supertrait(&self) {}
}

// PLANTED: a type alias of a holder, and a block written through it.
type NativeRuntime<'a> = Runtime<'a, Native>;

impl NativeRuntime<'_> {
    fn through_an_alias(&self) {}
}

// PLANTED: an impl for a reference to a native holder.
impl<'a> Clone for &'a Runtime<'a, Native> {
    fn clone(&self) -> Self {
        self
    }
}

// PLANTED: a free function over native holders.
fn rebuild_every_window(_windows: Vec<&mut Runtime<'_>>) {}

// PLANTED: a `const` naming a native holder.
const PLANTED: Option<Runtime<'static>> = None;
