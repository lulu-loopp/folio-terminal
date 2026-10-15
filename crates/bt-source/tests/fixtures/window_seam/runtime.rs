use crate::window_seam::prelude::*;
use crate::window_seam::Backend as Seam;
use crate::window_seam::Native;
use crate::window_seam::ShellBackend;
use crate::{Indirect, Pair, Runtime, Shell};

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

impl<B: Backend> Runtime<'_, B> {
    // PLANTED: a method of a generic block taking a holder whose backend defaults to the native
    // one.
    fn takes_the_native(&mut self, _other: &mut Runtime<'_>) {}
}

// PLANTED: a generic trait impl of a holder naming the native backend in its body.
impl<B: Backend> std::fmt::Debug for Shell<B> {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let _native: Option<Shell<Native>> = None;
        Ok(())
    }
}

pub trait Twin<T> {}

// PLANTED: a generic trait impl of a holder naming the native backend in its trait's arguments.
impl<B: Backend> Twin<Shell<Native>> for Shell<B> {}

// PLANTED: a native holder built in expression position, its backend chosen by inference.
fn builds_by_inference() {
    Runtime::create();
}

// PLANTED: a native holder taken apart in a struct pattern.
fn takes_apart_by_pattern() {
    let Shell { window: _ } = unimplemented!();
}

// PLANTED: a type whose field holds a native holder.
pub struct Keeper {
    runtime: Runtime<'static>,
}

// PLANTED: an associated const naming a native holder.
impl crate::Plain {
    const NATIVE: Option<Runtime<'static>> = None;
}

// PLANTED: an associated type of a generic holder block naming a native holder.
impl<B: Backend> Iterator for Shell<B> {
    type Item = Shell<Native>;

    fn next(&mut self) -> Option<Self::Item> {
        None
    }
}

/// Generic over the backend: a holder built by inference ties to the parameter.
fn builds_generically<B: Backend>() -> Runtime<'static, B> {
    Runtime { window: unimplemented!() }
}

// PLANTED: the native backend named by its path.
fn names_it_by_its_path() {
    let _ = crate::window_seam::Native;
}

/// Not the backend: an enum's variant spelled as it.
pub enum Kind {
    Native,
}

fn a_variant_spelled_as_the_backend() -> Kind {
    Kind::Native
}

// PLANTED: a holder named in a macro's tokens, in a body.
fn builds_in_a_macro() -> Vec<()> {
    vec![Runtime::create()]
}

// PLANTED: a holder named in a `macro_rules!` body.
macro_rules! builds_one {
    () => {
        crate::Runtime::create()
    };
}

// PLANTED: a native-only method called by inference inside a function generic over the backend.
fn generic_builds<B: Backend>(_runtime: &Runtime<'_, B>) {
    Runtime::create();
}

// PLANTED: an inner function sees none of the generic scope around it.
fn holds_an_inner_function<B: Backend>(_runtime: &Runtime<'_, B>) {
    fn inner() {
        let _ = Shell::settle;
    }
}

// PLANTED: `use` items renaming the native backend and the seam module.
use crate::window_seam::Native as N;
use crate::window_seam as ws;

// PLANTED: the native backend named through a module that brought it in.
fn through_a_module() {
    let _ = crate::runtime::Native;
}

// PLANTED: a trait whose associated const and associated type name native holders.
pub trait Keeps {
    const KEPT: Option<Runtime<'static>>;
    type Held: Into<Shell<Native>>;
}

// PLANTED: a trait whose supertrait and generic bound name native holders.
pub trait Bounded<T: Into<Runtime<'static>>>: Twin<Shell<Native>> {}

// PLANTED: a native holder built by a struct literal.
fn builds_by_a_literal() {
    let _ = Shell { window: () };
}

// PLANTED: a native holder taken apart by a tuple-struct pattern.
fn takes_a_pair_apart() {
    let Pair(_) = unimplemented!();
}

// PLANTED: a holder block's own `where` clause naming a native holder.
impl<B: Backend> Runtime<'_, B>
where
    Shell<Native>: Clone,
{
    fn constrained(&self) {}
}

// PLANTED: a generic parameter's default on a type that holds no backend.
pub struct Defaulted<T = Runtime<'static>>(T);

// PLANTED: a `use` renaming a holder, and a function written through the new name.
use crate::Runtime as Rt;

fn through_a_renamed_holder(_runtime: Rt<'_>) {}

// PLANTED: a `use` in a body renaming a holder.
fn renames_a_holder_inside() {
    use crate::Shell as Sh;
    let _: Option<Sh> = None;
}

// PLANTED: a module-level type alias of the native backend, and one whose generic default is it.
type NativeBackend = crate::window_seam::Native;
type Windowed<W = Native> = Option<W>;

// PLANTED (with `generic_builds` above): a generic trait impl holding a method of the native
// block's name; a path finds the inherent method first, so `Runtime::create()` in a generic scope
// is still the native one.
pub trait Creates {
    fn create();
}

impl<B: Backend> Creates for Runtime<'_, B> {
    fn create() {}
}

/// Passes: a trait's own backend parameter is in scope in its method's default body.
pub trait Hosts<B: Backend> {
    fn host(runtime: &Runtime<'_, B>) {
        let _ = runtime;
    }

    // PLANTED: the trait's scope is no exemption — a default body naming a native holder.
    fn native_default(_runtime: &Runtime<'_>) {}
}

/// Passes: a backend trait's `Self` is in scope in its method's default body.
pub trait SelfHosted: Backend {
    fn hosted(_runtime: &Runtime<'_, Self>) {}
}

// PLANTED: a door of the seam implemented inside a function's body.
fn hides_a_door() {
    struct Hidden;
    impl WindowHost for Hidden {
        fn request_redraw(&self) {}
    }
}

// PLANTED: a door of the seam, renamed in a body and implemented there.
fn hides_a_renamed_door() {
    use crate::window_seam::WindowHost as Door;
    struct Hidden;
    impl Door for Hidden {
        fn request_redraw(&self) {}
    }
}

// PLANTED: a native block of a holder written inside a function's body (a mention of the holder).
fn hides_a_native_block() {
    impl Runtime<'_, Native> {
        fn hidden() {}
    }
}

// PLANTED: a door of the seam implemented inside a `const`'s initializer.
const _: () = {
    struct Made;
    impl WindowHost for Made {
        fn request_redraw(&self) {}
    }
};

// PLANTED: a door reached by a body's `use` through a module's renamed re-export.
fn hides_a_reexported_door() {
    use crate::elsewhere::Host;
    struct Hidden;
    impl Host for Hidden {
        fn request_redraw(&self) {}
    }
}

// PLANTED: a door implemented inside an array length in a signature.
fn sized(
    _: [u8; {
        struct Made;
        impl WindowHost for Made {
            fn request_redraw(&self) {}
        }
        1
    }],
) {
}

// PLANTED: an enum discriminant naming a native holder.
pub enum Counted {
    One = {
        let _ = Runtime::create;
        1
    },
}

// PLANTED: a door reached through a body `use` glob.
fn hides_behind_a_glob() {
    use crate::elsewhere::*;
    struct Hidden;
    impl Host for Hidden {
        fn request_redraw(&self) {}
    }
}

// PLANTED: a door reached through a chain of body `use`s.
fn hides_behind_a_chain() {
    use crate::elsewhere;
    use elsewhere::Host as Chained;
    struct Hidden;
    impl Chained for Hidden {
        fn request_redraw(&self) {}
    }
}

// PLANTED: one local name for two traits in sibling blocks; the first block's is the door.
fn hides_in_a_sibling_block() {
    {
        use crate::elsewhere::Host as Either;
        struct First;
        impl Either for First {
            fn request_redraw(&self) {}
        }
    }
    {
        use std::fmt::Debug as Either;
        struct Second;
        impl Either for Second {
            fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                Ok(())
            }
        }
    }
}

// PLANTED: a door reached through a module brought in by a `{self}` leaf.
fn hides_behind_self() {
    use crate::elsewhere::{self};
    struct Hidden;
    impl elsewhere::Host for Hidden {
        fn request_redraw(&self) {}
    }
}

// PLANTED: a macro whose tokens implement a door wherever it is expanded.
macro_rules! makes_a_door {
    ($made:ident) => {
        impl WindowHost for $made {
            fn request_redraw(&self) {}
        }
    };
}

// PLANTED: a holder reached through a body alias, in a body the guard reads.
fn aliases_a_holder_inside() {
    type Aliased = Runtime<'static>;
    impl Aliased {
        fn sneaky() {}
    }
}

// PLANTED: a module below module level, holding a door.
fn hides_a_module() {
    mod hidden {
        use crate::elsewhere::Host;
        struct Hidden;
        impl Host for Hidden {
            fn request_redraw(&self) {}
        }
    }
}

// PLANTED: a nested impl of a single-segment trait the guard cannot resolve.
fn implements_a_mystery() {
    struct Hidden;
    impl Mystery for Hidden {}
}

// PLANTED: a macro implementing a door through a renamed re-export.
macro_rules! door_through_a_reexport {
    () => {
        struct Made;
        impl crate::elsewhere::Host for Made {
            fn request_redraw(&self) {}
        }
    };
}

// PLANTED: names built from pieces.
paste::paste! {
    fn [<built _ here>]() {}
}
