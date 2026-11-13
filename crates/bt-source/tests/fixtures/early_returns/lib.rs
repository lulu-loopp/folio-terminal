// The early-return guard's fixture: each shape of a road out of a test, the child half, the
// roads in no shape, and returns that are not the test's own. Read by bt-source's
// `early_returns` test; never compiled.

/// A test root that only some hosts can make.
pub struct TestRoot;

impl TestRoot {
    pub fn new() -> Result<Self, String> {
        Err("no test root here".to_owned())
    }

    pub fn signs(&self) -> bool {
        true
    }
}

pub fn endpoint() -> Option<u16> {
    None
}

pub fn ready() -> bool {
    false
}

/// A helper's own road out is no test's: not read.
pub fn helper() {
    let Some(_) = endpoint() else {
        return;
    };
}

macro_rules! twice {
    ($($body:tt)*) => {
        $($body)*
        $($body)*
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    const PART: &str = "EARLY_RETURNS_PART";

    #[test]
    fn a_fixture_answering_none() {
        let Some(root) = TestRoot::new().ok() else {
            return;
        };
        assert!(root.signs());
    }

    #[test]
    fn a_fixture_answering_none_in_a_result_test() -> Result<(), String> {
        let Some(root) = TestRoot::new().ok() else {
            return Ok(());
        };
        assert!(root.signs());
        Ok(())
    }

    #[test]
    fn a_host_by_its_cfg() {
        if cfg!(not(windows)) {
            return;
        }
        assert!(ready());
    }

    #[test]
    fn an_endpoint_by_a_match() {
        let at = match endpoint() {
            Some(at) => at,
            None => return,
        };
        assert_eq!(at, 1);
    }

    #[test]
    fn a_child_half_reads_its_part() {
        if std::env::var_os(PART).is_some() {
            helper();
            return;
        }
        assert!(ready());
    }

    #[test]
    fn a_road_at_the_end_of_a_loop() {
        for _step in 0..3 {
            return;
        }
        assert!(ready());
    }

    #[test]
    fn an_if_with_an_else() {
        if ready() {
            return;
        } else {
            assert!(!ready());
        }
    }

    #[test]
    fn a_road_among_a_macros_tokens() {
        twice! { return; }
    }

    #[test]
    fn a_return_in_a_closure_is_not_the_tests() {
        let check = || {
            if !ready() {
                return;
            }
            assert!(ready());
        };
        check();
        assert!(Some(1).map(|one| { return one + 1; }) == Some(2));
    }

    #[test]
    fn a_nested_function_returns_its_own() {
        fn inner() {
            let Some(_) = endpoint() else {
                return;
            };
        }
        inner();
        assert!(!ready());
    }

    #[test]
    fn a_road_after_an_assertion_has_checked_something() {
        assert!(!ready());
        let Some(at) = endpoint() else {
            return;
        };
        assert_eq!(at, 1);
    }

    #[test]
    fn an_error_return_is_red_not_silent() -> Result<(), String> {
        let Some(at) = endpoint() else {
            return Err("no endpoint".to_owned());
        };
        assert_eq!(at, 1);
        Ok(())
    }
}
