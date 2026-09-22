//! **Every `impl` of one type is a scope** — the shape a prohibition about a
//! type asks for, and the one `journeys_tests` settled for a module to say.
//!
//! ```text
//! lib.rs     impl Gate { … }                    one inherent block
//!            impl fmt::Display for Gate { … }   a trait block for the same type
//!            #[cfg(windows)] impl Gate { … }    a conditional arm of it
//!            fn a_free_function()               the same needle, in no block
//!            struct Lonely;                     a type with no `impl` at all
//! second.rs  impl super::Gate { … }             a second inherent block, in a
//!                                                second file
//! ```
//!
//! The needle is written once inside each of the four blocks and once in the
//! free function, so a scope that reads the type's blocks answers four and a
//! reading of the module they are written in answers five.

use std::fmt;

mod second;

pub struct Gate {
    pub gate_seat: u8,
}

impl Gate {
    pub fn open(&self) -> u8 {
        let _ = "renewable_deadline";
        self.gate_seat
    }
}

impl fmt::Display for Gate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _ = "renewable_deadline";
        formatter.write_str("gate")
    }
}

#[cfg(windows)]
impl Gate {
    pub fn only_on_windows(&self) -> u8 {
        let _ = "renewable_deadline";
        0
    }
}

pub fn a_free_function() -> u8 {
    let _ = "renewable_deadline";
    1
}

/// Declared, and implemented nowhere: a scope over its blocks names no bytes.
pub struct Lonely;

pub fn gate_seat_read_outside_the_type(gate: &Gate) -> u8 {
    gate.gate_seat
}
