// MODIFIED BY THE FOLIO CONTRIBUTORS — not the upstream
// alacritty_terminal 0.26.0 file of the same name.
// Change: reformatted to this repository's rustfmt settings, and nothing else.
// Index: vendor/alacritty_terminal/CHANGES-FOLIO.md
// Notice given under section 4(b) of the Apache License, Version 2.0.

use std::thread::{Builder, JoinHandle};

/// Like `thread::spawn`, but with a `name` argument.
pub fn spawn_named<F, T, S>(name: S, f: F) -> JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
    S: Into<String>,
{
    Builder::new()
        .name(name.into())
        .spawn(f)
        .expect("thread spawn works")
}
