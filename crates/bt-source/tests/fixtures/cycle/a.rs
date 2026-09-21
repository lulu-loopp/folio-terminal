// `#[path]` at the top of a file is relative to the directory the file is in,
// so this names `a.rs` — itself.
#[path = "a.rs"]
mod again;
