// The test-order guard's fixture: each refused shape once in a test, its accepted twin, the same
// calls in product code, and a bin located by `CARGO_BIN_EXE_` inside an integration test
// (`tests/beside.rs`). Read by bt-source's `test_order` test; never compiled.

/// Product code names and places its own files; the guard is about tests.
pub fn product_writes_its_own_files() {
    let here = std::env::current_exe().unwrap();
    let beside = here.parent().unwrap().join("folio.exe");
    std::fs::write(std::env::temp_dir().join("fixed"), b"").unwrap();
    std::fs::copy(&here, beside).unwrap();
    std::env::set_current_dir("/").unwrap();
}

#[cfg(test)]
mod tests {
    /// Outside an integration target this is `None`, and the test below passes having tested
    /// nothing.
    const BIN: Option<&str> = option_env!("CARGO_BIN_EXE_folio");

    #[test]
    fn a_bin_that_is_never_there() {
        let Some(bin) = BIN else { return };
        let _ = std::fs::read(bin);
    }

    #[test]
    fn a_bin_located_outside_an_integration_test() {
        let _ = std::path::Path::new(env!("CARGO_BIN_EXE_folio"));
    }

    #[test]
    fn a_build_output_beside_the_test_executable() {
        let this = std::env::current_exe().unwrap();
        let built = this.parent().unwrap().join("folio.exe");
        let _ = std::fs::read(built);
    }

    #[test]
    fn a_scratch_folder_beside_the_test_executable() {
        let this = std::env::current_exe().unwrap();
        let folder = this
            .parent()
            .unwrap()
            .join(bt_testpath::unique_name("beside"));
        std::fs::create_dir_all(folder).unwrap();
    }

    #[test]
    fn the_process_directory_changed() {
        std::env::set_current_dir(std::env::temp_dir()).unwrap();
    }

    #[test]
    fn the_environment_changed() {
        unsafe { std::env::remove_var("PATH") };
    }

    #[test]
    fn a_fixed_fixture_in_the_temporary_folder() {
        let root = std::env::temp_dir().join("folio-fixed");
        std::fs::create_dir_all(&root).unwrap();
    }

    #[test]
    fn a_fixed_fixture_in_the_build_directory() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fixed");
        std::fs::write(root.join("settings.json"), b"{}").unwrap();
    }

    #[test]
    fn a_fixture_named_by_the_owner() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tb-tests")
            .join(bt_testpath::unique_name("owned"));
        std::fs::create_dir_all(&root).unwrap();
    }

    #[test]
    fn a_path_named_and_never_made() {
        let path = std::env::temp_dir().join("folio-panic.log");
        assert!(path.ends_with("folio-panic.log"));
    }

    #[test]
    fn a_helper_named_like_the_folder_is_not_it() {
        use std::io::Write as _;
        fn temp_dir(tag: &str) -> std::path::PathBuf {
            bt_testpath::temp_path(tag)
        }
        std::fs::create_dir_all(temp_dir("helper")).unwrap();
    }

    #[test]
    fn a_folder_imported_inside_the_body() {
        use std::env::{temp_dir, var};
        let _ = var("PATH");
        std::fs::create_dir_all(temp_dir().join("hidden")).unwrap();
    }

    #[test]
    fn a_folder_through_a_renamed_self() {
        use std::env::{self as e};
        std::fs::create_dir_all(e::temp_dir().join("hidden")).unwrap();
    }

    #[test]
    fn a_body_that_imports_something_else() {
        use std::io::Write;
        let mut sink = std::io::sink();
        sink.write_all(b"").unwrap();
    }

    mod aliased {
        use std::env as e;

        #[test]
        fn a_bare_folder_through_an_alias() {
            std::fs::create_dir_all(e::temp_dir().join("hidden")).unwrap();
        }
    }

    mod not_aliased {
        use std::env;

        #[test]
        fn the_folder_by_its_path() {
            let _ = env::var("PATH");
        }
    }

    mod imported {
        use std::env::temp_dir;

        #[test]
        fn a_bare_folder_after_an_import() {
            std::fs::create_dir_all(temp_dir().join("hidden")).unwrap();
        }
    }

    #[test]
    fn its_own_executable_run_again() {
        let _ = std::process::Command::new(std::env::current_exe().unwrap()).status();
    }
}
