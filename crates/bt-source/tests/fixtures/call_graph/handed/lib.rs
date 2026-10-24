//! Closures handed to a call, and closures that are not: an argument of a path call and of a
//! method call (its second argument), one nested inside the other, one bound and called, one called
//! in place.

pub struct Pool;

impl Pool {
    pub fn submit<F: FnOnce()>(&self, tag: u8, work: F) {
        let _ = tag;
        work();
    }
}

pub fn spawn<F: FnOnce()>(work: F) {
    work();
}

fn worker() {}
fn inner() {}
fn bound() {}
fn in_place() {}

pub fn root(pool: &Pool) {
    spawn(move || {
        worker();
        pool.submit(1, || inner());
    });
    let later = || bound();
    later();
    (|| in_place())();
}
