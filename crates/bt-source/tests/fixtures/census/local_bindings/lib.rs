// Plan row 115: a one-segment call of a name bound in an enclosing scope is a binding called —
// untyped — never the package's function of that name. The package declares `grab`, which
// returns the window; every function below but `plain`, `scope_ended` and `outer` calls a
// binding named `grab` whose value is not that function, so the `.title` it writes through is
// an unknown site, never a write of `WindowRuntime.title`.
#[path = "../shared/gate.rs"]
mod gate;
#[path = "../shared/model.rs"]
mod model;
#[path = "../shared/others.rs"]
mod others;

use crate::model::WindowRuntime;

// Not a fact: a field of the same name as the window's.
pub struct Other {
    pub title: String,
}

pub fn grab(_: u32) -> &'static mut WindowRuntime {
    unimplemented!()
}

fn other() -> &'static mut Other {
    unimplemented!()
}

fn apply(_: impl FnOnce(&dyn Fn(u32) -> &'static mut Other)) {}

macro_rules! named {
    ($name:ident) => {
        Some($name)
    };
}

// Nothing is bound: the package's function, so a write of the window's title.
pub fn plain() {
    grab(1).title = String::new();
}

// A parameter, `impl Fn*`.
pub fn impl_param(grab: impl FnOnce(u32) -> &'static mut Other) {
    grab(1).title = String::new();
}

// A parameter, `dyn Fn*`.
pub fn dyn_param(grab: &dyn Fn(u32) -> &'static mut Other) {
    grab(1).title = String::new();
}

// A generic parameter bounded `Fn*`.
pub fn generic_param<F: Fn(u32) -> &'static mut Other>(grab: F) {
    grab(1).title = String::new();
}

// A generic parameter bounded `Fn*` by a `where` clause.
pub fn where_param<F>(grab: F)
where
    F: Fn(u32) -> &'static mut Other,
{
    grab(1).title = String::new();
}

// A `let` of a closure.
pub fn let_closure() {
    let grab = |_: u32| other();
    grab(1).title = String::new();
}

// A closure's parameter.
pub fn closure_param() {
    apply(|grab| {
        grab(1).title = String::new();
    });
}

// A tuple pattern's name.
pub fn tuple_binding(pair: (fn(u32) -> &'static mut Other, u32)) {
    let (grab, _) = pair;
    grab(1).title = String::new();
}

// An `if let` pattern's name.
pub fn if_let_binding(maybe: Option<fn(u32) -> &'static mut Other>) {
    if let Some(grab) = maybe {
        grab(1).title = String::new();
    }
}

// A name in a pattern a macro writes: the rules cannot see that it binds `grab`, so every name in
// it is bound untyped.
pub fn macro_pattern(maybe: Option<fn(u32) -> &'static mut Other>) {
    match maybe {
        named!(grab) => {
            grab(1).title = String::new();
        }
        None => {}
    }
}

// The binding's scope has ended: the package's function again.
pub fn scope_ended() {
    {
        let grab = |_: u32| other();
        grab(1).title = String::new();
    }
    grab(1).title = String::new();
}

// An `if let`'s binding ends with its `then` block: in the `else` and after the `if`, `grab` is
// the package's.
pub fn if_let_after(maybe: Option<fn(u32) -> &'static mut Other>) {
    if let Some(grab) = maybe {
        let _ = grab;
    } else {
        grab(1).title = String::new();
    }
    grab(1).title = String::new();
}

// A closure's parameter ends with the closure: after it, `grab` is the package's.
pub fn closure_after() {
    apply(|grab| {
        let _ = grab;
    });
    grab(1).title = String::new();
}

// A nested function sees none of the outer one's bindings: its `grab` is the package's.
pub fn outer(grab: impl Fn(u32) -> &'static mut Other) {
    fn inner() {
        grab(1).title = String::new();
    }
    inner();
    grab(1).title = String::new();
}
