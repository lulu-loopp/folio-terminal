use crate::window_seam::prelude::*;
use crate::window_seam::Backend as Seam;
use crate::window_seam::Native;
use crate::{Runtime, Shell};

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
