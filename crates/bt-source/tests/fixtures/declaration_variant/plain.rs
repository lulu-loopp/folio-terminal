// Reached twice: by an unconditional declaration in `lib.rs`, and through the
// gate by a `#[path]` one in `gate.rs`. One set of bytes, two identities, and
// only one of them stands on `test`.

pub fn reached_two_ways() -> u8 {
    2
}
