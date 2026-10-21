//! Closures: an inline call, a body walked as its definer's, a stored closure called.

fn inside() {}
fn handed() {}

pub fn spawn<F: FnOnce()>(work: F) {
    work();
}

pub fn root() {
    (|| inside())();
    spawn(move || handed());
    let stored = || inside();
    stored();
}
