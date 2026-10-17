//! A file that globs `std::env`: a bare reader here may be the standard library's,
//! and a name alone cannot say.

use std::env::*;

pub fn under_a_glob() -> bool {
    var_os("BT_FIXTURE_GLOB").is_some()
}
