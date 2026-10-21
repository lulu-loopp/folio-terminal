//! A function named as a value, and a `fn` written inside a body.

fn worker() {}

pub fn spawn(work: fn()) {
    work();
}

pub fn root() {
    spawn(worker);
    fn local_helper() {
        worker();
    }
    local_helper();
}
