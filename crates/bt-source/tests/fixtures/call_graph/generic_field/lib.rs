//! A call on a bound generic in the shapes `bt_platform::install_txn` writes (plan row 155): a
//! method on a generic struct's field (`self.surface` in `impl<S: Surface> Rewrite<S>`) and on an
//! `impl Surface` argument is the method of every admitted impl of the trait — not of a
//! `#[cfg(test)]` one — and a concrete wrapper around the generic struct (`DurableWrite`'s
//! `Rewrite<Os>`) reaches that impl's effect.

use std::path::Path;

pub trait Surface {
    fn remove(&mut self, path: &Path);
}

pub struct Os;

impl Surface for Os {
    fn remove(&mut self, path: &Path) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
pub struct Recorder;

#[cfg(test)]
impl Surface for Recorder {
    fn remove(&mut self, _path: &Path) {}
}

pub struct Rewrite<S: Surface> {
    surface: S,
}

impl<S: Surface> Rewrite<S> {
    pub fn give_up(&mut self, path: &Path) {
        self.surface.remove(path);
    }
}

pub struct DurableWrite(Rewrite<Os>);

impl DurableWrite {
    pub fn abandon(&mut self, path: &Path) {
        self.0.give_up(path);
    }
}

pub fn handed(mut surface: impl Surface, path: &Path) {
    surface.remove(path);
}
