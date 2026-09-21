// `#[path]` makes the file it names a `mod.rs` to its own children, so the
// plain `mod child;` written in it is looked for beside THIS file.
#[path = "reached.rs"]
mod p;
