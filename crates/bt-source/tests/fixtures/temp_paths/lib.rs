// The temp-path guard's fixture: the old shape in a test, in a feature-gated
// test helper, and in product code. Read by bt-source's `temp_paths` test;
// never compiled.

use std::time::{SystemTime, UNIX_EPOCH};

/// Product code names its own temporary files; the guard is about tests.
fn product_unique_suffix() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    format!("{}-{nanos}", std::process::id())
}

/// A test helper compiled behind a feature as well as under `test`.
#[cfg(any(test, feature = "test-helper"))]
pub fn helper_scratch(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("{tag}-{}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_named_by_pid_and_clock() {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let _ = format!("probe-{}-{unique}", std::process::id());
    }

    #[test]
    fn a_test_named_by_the_owner() {
        let _ = bt_testpath::temp_path("probe");
    }

    #[test]
    fn a_test_that_only_reports_its_pid() {
        println!("ready {}", std::process::id());
    }
}
