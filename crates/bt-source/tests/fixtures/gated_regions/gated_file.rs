//! The review of plan row 77 (F5): a file whose own inner `#![cfg]` keeps it out of the product.
#![cfg(test)]

pub fn in_a_gated_file() -> fn() { crate::needles::the_gated_needle } // gated
