//! Foreign callees, by their full paths.

use std::sync::{Arc, Mutex};

pub struct Shared {
    state: Arc<Mutex<Vec<u8>>>,
}

impl Shared {
    pub fn touch(&self) {
        let _guard = self.state.lock();
    }
}

pub fn root() {
    let _ = std::fs::read("x");
}
