// The early-return guard's fixture: each shape of a road out of a test and each way its callee
// is read, the child half and a read that is none, the roads in no shape, the roads R drops
// after each kind of assertion, and returns that are not the test's own. Read by bt-source's
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

/// An endpoint a test reads through `*`.
pub fn shared_endpoint() -> &'static Option<u16> {
    &None
}

/// Settings whose port a test reads through a field.
pub struct Settings {
    pub port: Option<u16>,
}

impl Settings {
    pub fn load() -> Self {
        Self { port: None }
    }
}

/// An assertion under a helper's `assert…` name.
pub fn assert_nothing_moved() {}

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

    /// The parent half's start of its child: the one variable this file sets on a command,
    /// which is what makes a read of it a child half (G4).
    fn start_the_child() {
        let _ = std::process::Command::new("this-test").env(PART, "1").status();
    }

    #[test]
    fn a_unit_return_is_a_road() {
        let Some(_) = endpoint() else {
            return ();
        };
        assert!(ready());
    }

    #[test]
    fn an_if_let_is_judged_by_its_expression() {
        if let Err(_) = TestRoot::new() {
            return;
        }
        assert!(ready());
    }

    #[test]
    fn a_road_in_an_async_block_is_not_the_tests() {
        let _later = async {
            return;
        };
        assert!(!ready());
    }

    #[test]
    fn a_callee_through_a_not() {
        if !ready() {
            return;
        }
        assert!(ready());
    }

    #[test]
    fn a_callee_through_a_reference() {
        let Some(_) = &endpoint() else {
            return;
        };
        assert!(ready());
    }

    #[test]
    fn a_callee_through_a_question_mark() -> Result<(), String> {
        let true = TestRoot::new()?.signs() else {
            return Ok(());
        };
        assert!(ready());
        Ok(())
    }

    #[test]
    fn a_callee_through_a_field() {
        let Some(_) = Settings::load().port else {
            return;
        };
        assert!(ready());
    }

    #[test]
    fn a_callee_through_a_deref() {
        let Some(_) = *shared_endpoint() else {
            return;
        };
        assert!(ready());
    }

    #[test]
    fn a_callee_through_parentheses() {
        if !(ready()) {
            return;
        }
        assert!(ready());
    }

    #[test]
    fn a_bare_name_in_one_test() {
        let names = ["one"];
        let Some(_) = names.iter().next() else {
            return;
        };
        assert!(ready());
    }

    #[test]
    fn a_bare_name_in_another_test() {
        let names = ["another"];
        let Some(_) = names.iter().next() else {
            return;
        };
        assert!(ready());
    }

    #[test]
    fn a_variable_is_judged_by_its_name() {
        let found = endpoint();
        let Some(_) = found else {
            return;
        };
        assert!(ready());
    }

    #[test]
    fn a_road_after_a_statement() {
        helper();
        return;
    }

    #[test]
    fn a_panic_first_drops_the_road() {
        if ready() {
            panic!("ready already");
        }
        let Some(_) = endpoint() else {
            return;
        };
    }

    #[test]
    fn an_expect_err_first_drops_the_road() {
        TestRoot::new().expect_err("no test root here");
        let Some(_) = endpoint() else {
            return;
        };
    }

    #[test]
    fn an_unreachable_first_drops_the_road() {
        if ready() {
            unreachable!("not ready yet");
        }
        let Some(_) = endpoint() else {
            return;
        };
    }

    #[test]
    fn an_unwrap_err_first_drops_the_road() {
        TestRoot::new().unwrap_err();
        let Some(_) = endpoint() else {
            return;
        };
    }

    #[test]
    fn an_assert_helper_first_drops_the_road() {
        assert_nothing_moved();
        let Some(_) = endpoint() else {
            return;
        };
    }

    #[test]
    fn an_else_if_last_arm_is_held_as_g2() {
        if TestRoot::new().is_ok() {
            helper();
        } else if !ready() {
            return;
        }
        assert!(ready());
    }

    #[test]
    fn a_child_half_by_a_match() {
        match std::env::var(PART) {
            Ok(_) => return,
            Err(_) => {}
        }
        start_the_child();
        assert!(ready());
    }

    #[test]
    fn a_child_half_by_a_let() {
        let Err(_) = std::env::var(PART) else {
            return;
        };
        start_the_child();
        assert!(ready());
    }

    #[test]
    fn an_opt_in_is_no_child_half() {
        if std::env::var_os("EARLY_RETURNS_OPT_IN").is_none() {
            return;
        }
        assert!(ready());
    }

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
