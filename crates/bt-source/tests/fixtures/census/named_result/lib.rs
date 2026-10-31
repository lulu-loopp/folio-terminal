// Plan row 101 (ruling R3): the census reads `?` and `Ok`/`Err` by what a type resolves to —
// the standard `Option`/`Result`, or a foreign `Result` alias on the fixed table — never by the
// bare name `Result`: a workspace enum that is called `Result` is that enum.
#[path = "../shared/gate.rs"]
mod gate;
#[path = "../shared/model.rs"]
mod model;
#[path = "../shared/others.rs"]
mod others;

use crate::model::WindowRuntime;

pub mod verdict {
    /// A workspace enum that happens to be called `Result`: its `Ok` holds a number, and only
    /// its `Err` holds the `T`.
    pub enum Result<T> {
        Ok(u32),
        Err(T),
    }
}

// The census reads no `use` item: in a package that declares a type called `Result`, a bare
// `Result` is that type (its module-placing rule), so the standard ones here are written with
// their crate.

// The standard `Result`: `Ok` holds the window, so this is a write of its title.
pub fn standard_ok(result: std::result::Result<&mut WindowRuntime, ()>) {
    if let Ok(window) = result {
        window.title = String::new();
    }
}

// A `Result` alias on the table, written with its crate: `?` gives the window.
pub fn spelled_try(result: std::io::Result<&mut WindowRuntime>) -> std::io::Result<()> {
    let window = result?;
    window.title = String::new();
    Ok(())
}

// The workspace `Result`'s `Ok` holds a `u32`: no window is written here, and `title` spelled
// on a number is an unknown site.
pub fn named_ok(verdict: verdict::Result<&mut WindowRuntime>) {
    if let verdict::Result::Ok(number) = verdict {
        number.title = String::new();
    }
}

// The workspace `Result`'s `Err` holds the `T`: a write of the window's title.
pub fn named_err(verdict: verdict::Result<&mut WindowRuntime>) {
    if let verdict::Result::Err(window) = verdict {
        window.title = String::new();
    }
}

// `?` on the workspace `Result` is no standard `?`: what it gives is untyped, and the field it
// spells is listed as unknown, never attributed.
pub fn named_try(verdict: verdict::Result<&mut WindowRuntime>) -> Option<()> {
    let window = verdict?;
    window.title = String::new();
    Some(())
}
