// A second way to `shared.rs`, from inside test code.
#[path = "shared.rs"]
mod shared_again;

// Test code all the way down, with no gate of its own.
mod helper;
