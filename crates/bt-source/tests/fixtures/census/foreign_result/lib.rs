// Plan row 101 (ruling R3; the review's F4): in a package that declares no type called `Result`,
// a foreign `Result` reaches the census's alias table, and only its rows are a `Result` — a
// foreign path off the table (`syn::Result`) is not one, whatever its last segment.
#[path = "../shared/gate.rs"]
mod gate;
#[path = "../shared/model.rs"]
mod model;
#[path = "../shared/others.rs"]
mod others;

use crate::model::WindowRuntime;

// On the table: `?` gives the window, so this is a write of its title.
pub fn on_table_try(result: anyhow::Result<&mut WindowRuntime>) -> anyhow::Result<()> {
    let window = result?;
    window.title = String::new();
    Ok(())
}

// Off the table, through a standard method's name: `syn::Result`'s `map_err` is not the
// standard `Result`'s, so the `?` after it gives nothing the rules type (an unknown site).
pub fn off_table_map_err(result: syn::Result<&mut WindowRuntime>) -> syn::Result<()> {
    let window = result.map_err(|e| e)?;
    window.title = String::new();
    Ok(())
}

// Off the table: `?` on `syn::Result` gives nothing the rules type, and the field it spells is
// an unknown site, never attributed.
pub fn off_table_try(result: syn::Result<&mut WindowRuntime>) -> syn::Result<()> {
    let window = result?;
    window.title = String::new();
    Ok(())
}
