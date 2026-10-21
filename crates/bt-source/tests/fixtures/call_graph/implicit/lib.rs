//! First-party impls only implicit calls run.

pub struct Lane;

impl Drop for Lane {
    fn drop(&mut self) {
        stop();
    }
}

fn stop() {}

pub fn root() {
    let _lane = Lane;
}
