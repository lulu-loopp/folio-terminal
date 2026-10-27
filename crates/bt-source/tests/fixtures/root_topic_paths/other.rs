//! A module outside `runtime/`: T6's shapes, each PLANTED.

pub struct Field {
    // T6 — PLANTED: a field's type.
    pub press: crate::runtime::pointer::Press,
}

pub trait Shape {
    fn shape() -> u8;
}

// T6 — PLANTED: an `impl` header.
impl Shape for super::runtime::pointer::Press {
    fn shape() -> u8 {
        // T6 — PLANTED: UFCS.
        <crate::runtime::pointer::Press as Shape>::shape()
    }
}
