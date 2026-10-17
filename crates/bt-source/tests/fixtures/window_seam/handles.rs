//! The window's platform handle (RULE C): code written for the window names neither
//! `bt_platform::NativeWindow` nor `bt_platform::Compositor`; the platform's own glue may.

use crate::window_seam::prelude::*;
use crate::Runtime;
use bt_platform as platform;
use bt_platform::NativeWindow;

// PLANTED: a function over a door, naming the window's handle in its signature.
pub fn hands_out_the_handle(window: &impl WindowHost) -> bt_platform::NativeWindow {
    window.request_redraw();
    loop {}
}

// PLANTED: a generic holder block's method, naming the composition tree through a renamed crate.
impl<B: Backend> Runtime<'_, B> {
    fn reaches_the_tree(&self) {
        let _ = platform::Compositor::new;
    }
}

// PLANTED: a function generic over the backend, naming the handle through the module's `use`.
pub fn holds_a_handle<B: Backend>(window: &B::Window) -> Option<NativeWindow> {
    window.request_redraw();
    None
}

// PLANTED: a body `use` renaming the handle, in a `where`-bounded function.
pub fn renames_the_handle<W>(window: &W)
where
    W: WindowHost,
{
    use bt_platform::NativeWindow as Handle;
    window.request_redraw();
    let _: Option<Handle> = None;
}

// PLANTED: the tree in a macro's tokens, in a function over a door.
pub fn says_it_in_a_macro(window: &impl WindowHost) {
    window.request_redraw();
    let _ = vec![bt_platform::Compositor::default()];
}

// PLANTED: a holder whose field is the handle.
pub struct Keeps<B: Backend> {
    window: B::Window,
    handle: bt_platform::NativeWindow,
}

// PLANTED: the handle through a glob of the platform crate, inside a nested module.
pub mod globbed {
    use crate::window_seam::WindowHost;
    use bt_platform::*;

    pub fn through_the_glob(window: &dyn WindowHost) -> Option<Compositor> {
        window.request_redraw();
        None
    }
}

/// Passes: the platform's own glue, written for no window, names both.
pub fn glue(handle: NativeWindow, tree: &platform::Compositor) {
    let _ = (handle, tree);
}

/// Passes: code written for the window naming the backend's own associated types, and the
/// fixture's own `NativeWindow`, which is not the platform's.
pub fn associated<B: Backend>(window: &B::Window, own: &crate::window_seam::NativeWindow) {
    window.request_redraw();
    own.request_redraw();
}

// PLANTED: a module-level type alias of the handle — the walk does not follow an alias, so the
// alias itself is refused; the function written through it is the road it would open.
pub type Handle = bt_platform::NativeWindow;

pub fn through_an_alias(window: &impl WindowHost) -> Option<Handle> {
    window.request_redraw();
    None
}

// PLANTED: a module-level `macro_rules!` whose body spells the tree.
macro_rules! hands_it_over {
    () => {
        bt_platform::Compositor
    };
}

// PLANTED: a chain of renames past the walk's hop limit — refused, never passed.
use bt_platform::NativeWindow as Far0;
use Far0 as Far1;
use Far1 as Far2;
use Far2 as Far3;
use Far3 as Far4;
use Far4 as Far5;
use Far5 as Far6;
use Far6 as Far7;
use Far7 as Far8;
use Far8 as Far9;

pub fn too_far(window: &impl WindowHost) -> Option<Far9> {
    window.request_redraw();
    None
}
