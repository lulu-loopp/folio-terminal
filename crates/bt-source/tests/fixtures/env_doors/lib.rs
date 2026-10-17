//! The `env_doors` fixture: one function per shape of an environment read, for
//! `tests/env_doors.rs`. Each function's name is the shape; the guard's table says
//! what the scan must answer for it.

use std::env;
use std::env::var_os as read_door;
use std::env::{var, var_os};
use std::ffi::OsString;

mod globbed;

const DOOR: &str = "BT_FIXTURE_CONSTANT";

mod one {
    pub const TWICE: &str = "BT_FIXTURE_ONE";
}

mod two {
    pub const TWICE: &str = "BT_FIXTURE_TWO";
}

use one::TWICE;

mod other {
    pub const ELSEWHERE: &str = "BT_FIXTURE_ELSEWHERE";
}

mod reexport {
    pub use std::env::var_os;
}

macro_rules! door {
    ($name:expr) => {
        std::env::var_os($name)
    };
}

pub fn literal_os() -> Option<OsString> {
    std::env::var_os("BT_FIXTURE_LITERAL_OS")
}

pub fn literal() -> bool {
    std::env::var("BT_FIXTURE_LITERAL").is_ok()
}

pub fn through_use() -> Option<OsString> {
    env::var_os("BT_FIXTURE_USE")
}

pub fn through_a_constant() -> Option<OsString> {
    std::env::var_os(DOOR)
}

pub fn bare() -> bool {
    var_os("BT_FIXTURE_BARE").is_some() && var("NOT_A_DOOR").is_ok()
}

pub fn not_a_door() -> Option<OsString> {
    ::std::env::var_os("PATH")
}

pub fn computed(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

pub fn formatted() -> bool {
    std::env::var(format!("BT_{}", 1)).is_ok()
}

pub fn ambiguous() -> Option<OsString> {
    std::env::var_os(TWICE)
}

pub fn through_another_module() -> Option<OsString> {
    std::env::var_os(other::ELSEWHERE)
}

pub fn other_module_path() -> Option<OsString> {
    reexport::var_os("BT_FIXTURE_REEXPORTED")
}

pub fn enumerated() -> usize {
    std::env::vars_os().count()
}

pub fn handed_on() -> [Option<OsString>; 1] {
    ["BT_FIXTURE_HANDED_ON"].map(std::env::var_os)
}

pub fn through_a_macro() -> Option<OsString> {
    door!("BT_FIXTURE_MACRO")
}

pub fn renamed() -> Option<OsString> {
    read_door("BT_FIXTURE_RENAMED")
}

pub struct Shell;

impl Shell {
    pub fn var_os(&self, _key: &str) -> Option<OsString> {
        None
    }
}

pub fn a_method(shell: &Shell) -> Option<OsString> {
    shell.var_os("BT_FIXTURE_METHOD")
}

pub fn a_local_named_vars() -> usize {
    let vars = [1, 2];
    vars.len()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_test_reads_its_own_marker() {
        let _ = std::env::var_os("BT_FIXTURE_TEST_CHILD");
    }
}
