//! Plan row 77: a `cfg` whose argument is no predicate the reading reads, a `cfg_attr` that
//! writes a `cfg`, and a `cfg` on a position the gate walk does not read — each refused.

#[cfg(all(test windows))]
pub fn missing_comma() {}

#[cfg_attr(test, cfg(any()))]
pub fn conditional_gate() {}

pub fn closure_parameter() -> u8 {
    let read = |#[cfg(test)] inner: u8| inner;
    read(1)
}
