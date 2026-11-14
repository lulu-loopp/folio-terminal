//! Plan row 77's fixture: one needle in every position a `cfg` can stand on, gated and not. A
//! line that ends `// counted` is product; a line that ends `// gated` is not.

pub mod needles {
    pub fn the_gated_needle() {} // counted
    pub trait Seen {}
}

use needles::the_gated_needle as product_use; // counted

#[cfg(test)]
use needles::the_gated_needle; // gated

#[cfg(not(test))]
use needles::the_gated_needle as not_test; // counted

#[cfg(any(test, windows))]
use needles::the_gated_needle as any_test; // counted

#[cfg(all(test, windows))]
const ALL_TEST: fn() = needles::the_gated_needle; // gated

#[cfg(test)]
static GATED_STATIC: fn() = needles::the_gated_needle; // gated

#[cfg(test)]
type GatedAlias = needles::the_gated_needle; // gated

pub struct Holder;

#[cfg(test)]
impl needles::Seen for Holder where Holder: the_gated_needle {} // gated

#[cfg(test)]
macro_rules! gated_macro {
    () => {
        the_gated_needle() // gated
    };
}

pub fn product_body(value: u8) {
    #[cfg(test)]
    let gated_let = needles::the_gated_needle; // gated
    #[cfg(test)]
    needles::the_gated_needle(); // gated
    let product_let = needles::the_gated_needle; // counted
    match value {
        #[cfg(test)]
        1 => needles::the_gated_needle(), // gated
        _ => {}
    }
}

// The review of plan row 77 (F5): the positions the walk reads that had no line.
pub struct WithFields {
    #[cfg(test)]
    gated_field: the_gated_needle, // gated
    product_field: the_gated_needle, // counted
}

pub enum WithVariants {
    #[cfg(test)]
    Gated(the_gated_needle), // gated
    Product,
}

pub fn with_parameter(#[cfg(test)] gated: the_gated_needle, value: u8) {} // gated

pub fn with_generic<#[cfg(test)] T: the_gated_needle, U>() {} // gated

#[cfg(test)]
extern crate the_gated_needle; // gated

unsafe extern "C" {
    #[cfg(test)]
    fn the_gated_needle(); // gated
}

impl Holder {
    #[cfg(test)]
    fn gated_method() -> fn() { needles::the_gated_needle } // gated
}

pub trait WithItems {
    #[cfg(test)]
    fn gated_item() -> fn() { needles::the_gated_needle } // gated
}

pub mod gated_file;

// 2026-10-30 (plan row 109): the macro-token line went to `gated_in_macro` and
// `gated_in_macro_refused` — a `cfg` inside a macro's tokens is now read or refused.
