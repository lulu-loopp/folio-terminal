// Plan row 126: a one-segment call `f(..)` is typed only from the declarations its module
// reaches — the module's own product items, then its product `use` items (renames and the
// package's re-exports followed), then its globs, and across a `use` or a glob into another
// workspace crate, followed to that crate's declaration. Nothing else answers: not a test
// helper of the same name, not a function of another module or package the call does not reach.
// A call reaching a test-only declaration, a crate the census does not read, a `fn` or a `use`
// written in a body, or nothing at all is refused, and the `.title` written through it is an
// unknown site.
#[path = "../../shared/gate.rs"]
mod gate;
#[path = "../../shared/model.rs"]
mod model;
#[path = "../../shared/others.rs"]
mod others;

// Another crate's module, brought into the crate root under its name.
use census_reach_other::attention;

// The package's product functions that return the window.
pub mod windows {
    use crate::model::WindowRuntime;

    pub fn grab(_: u32) -> &'static mut WindowRuntime {
        unimplemented!()
    }

    // One item, two `cfg` variants: one answer.
    #[cfg(unix)]
    pub fn pick(_: u32) -> &'static mut WindowRuntime {
        unimplemented!()
    }

    #[cfg(windows)]
    pub fn pick(_: u32) -> &'static mut WindowRuntime {
        unimplemented!()
    }

    // GREEN: declared in the module the call is written in.
    pub fn here() {
        grab(1).title = String::new();
    }
}

// A re-export of the package's own function, under another name.
pub mod reexport {
    pub use crate::windows::grab as fetch_window;
}

// Test helpers named like the calls of `red` below: each returns the window, so a call typed from
// one of them would write the window's title.
#[cfg(test)]
mod tests {
    use crate::model::WindowRuntime;

    pub fn helper(_: u32) -> &'static mut WindowRuntime {
        unimplemented!()
    }

    pub fn fetch(_: u32) -> &'static mut WindowRuntime {
        unimplemented!()
    }

    pub fn spread(_: u32) -> &'static mut WindowRuntime {
        unimplemented!()
    }

    pub fn take(_: u32) -> &'static mut WindowRuntime {
        unimplemented!()
    }

    pub fn probe(_: u32) -> &'static mut WindowRuntime {
        unimplemented!()
    }

    pub fn solve(_: u32) -> &'static mut WindowRuntime {
        unimplemented!()
    }

    pub fn kind_mode(_: u32) -> &'static mut WindowRuntime {
        unimplemented!()
    }

    pub fn admitted(_: u32) -> &'static mut WindowRuntime {
        unimplemented!()
    }

    pub fn spin(_: u32) -> &'static mut WindowRuntime {
        unimplemented!()
    }
}

pub mod green {
    use crate::reexport::fetch_window;
    use crate::windows::grab;
    use crate::windows::grab as window_of;
    use crate::windows::pick;

    // GREEN: a `use` of the package's function.
    pub fn through_a_use() {
        grab(1).title = String::new();
    }

    // GREEN: a `use` renaming it.
    pub fn through_a_rename() {
        window_of(1).title = String::new();
    }

    // GREEN: the package's re-export, followed.
    pub fn through_a_reexport() {
        fetch_window(1).title = String::new();
    }

    // GREEN: the `cfg` variants of one item agree.
    pub fn through_cfg_variants() {
        pick(1).title = String::new();
    }
}

pub mod glob_user {
    use crate::windows::*;

    // GREEN: a glob of the package's module.
    pub fn through_a_glob() {
        grab(1).title = String::new();
    }
}

// Fold F1: a module `config` of the crate root, and a module that imports it and declares a
// function of the same name. A module and a function live in two namespaces, so both are there.
pub mod config {}

pub mod shadowed {
    #[allow(unused_imports)]
    use crate::config;

    pub fn config(_: u32) -> &'static mut crate::model::WindowRuntime {
        unimplemented!()
    }
}

// Fold F4: `self::` and `super::` paths.
pub mod relative {
    use self::deeper::grab_here;

    pub mod deeper {
        use super::super::windows::grab;

        pub use crate::windows::grab as grab_here;

        // GREEN: `super::super::windows::grab`.
        pub fn through_super() {
            grab(1).title = String::new();
        }
    }

    // GREEN: `self::deeper::grab_here`, a re-export there.
    pub fn through_self() {
        grab_here(1).title = String::new();
    }
}

// Fold F3: a test-only declaration beside a product `use` of the same name: a product build has
// only the `use`.
pub mod skips {
    use crate::windows::grab as now;

    #[cfg(test)]
    fn now(_: u32) -> &'static mut crate::model::WindowRuntime {
        unimplemented!()
    }

    // GREEN: the test-only `now` is skipped; the `use` answers.
    pub fn a_test_only_declaration_beside_a_product_use() {
        now(1).title = String::new();
    }
}

// Fold F4: a `pub use` cycle.
pub mod cycle_a {
    pub use crate::cycle_b::spin;
}

pub mod cycle_b {
    pub use crate::cycle_a::spin;
}

// Fold F2: a local enum's glob brings in no function.
pub mod enum_glob {
    pub enum Kind {
        One,
    }

    use Kind::*;

    pub fn a_type_glob_brings_no_function() {
        let _ = One;
        missing(1).title = String::new();
    }
}

pub mod green_fold {
    use crate::shadowed::config;

    // GREEN (fold F1): `shadowed`'s own `config`, not the module it imports.
    pub fn through_a_function_named_like_an_imported_module() {
        config(1).title = String::new();
    }
}

pub mod red {
    use super::super::lost;
    use crate::attention::kind_mode;
    use crate::cycle_a::spin;
    use census_reach_other::admission::admitted;
    use outside_the_workspace::thing;
    #[cfg(test)]
    use crate::tests::probe;
    use census_reach_other::fetch;
    use census_reach_other::globbed::*;
    use census_reach_other::solve;
    use std::mem::take;

    // RED 1: no product item of the module reaches `helper`; only a test helper elsewhere has the
    // name. Refused.
    pub fn a_test_helper_elsewhere() {
        helper(1).title = String::new();
    }

    // RED 2: a `use` into another workspace crate, through that crate's re-export: its `fetch`
    // returns its own `Other`, not the window. Typed, so nothing of the window is written.
    pub fn through_a_foreign_use() {
        fetch(1).title = String::new();
    }

    // RED 2b: another crate's re-export of its module's function of the same name as the
    // module (`pub use solve::solve;`): the module is looked up before the `use` name. Typed.
    pub fn through_a_reexport_named_like_its_module() {
        solve(1).title = String::new();
    }

    // RED 2c: a path through a module the crate root brings in from another crate
    // (`crate::attention`, which is `census_reach_other::attention`). Typed.
    pub fn through_an_aliased_module() {
        kind_mode(1).title = String::new();
    }

    // RED 3: a function written in the body. The census reads no declaration of it, and another
    // crate's `lend` is not what the call reaches. Refused.
    pub fn a_function_of_the_body() {
        fn lend(_: u32) -> &'static mut crate::model::WindowRuntime {
            unimplemented!()
        }
        lend(1).title = String::new();
    }

    // RED 4: a glob into another workspace crate: its `spread` returns its own `Other`. Typed.
    pub fn through_a_foreign_glob() {
        spread(1).title = String::new();
    }

    // RED 5: a `use` into a crate the census does not read. Refused.
    pub fn through_the_standard_library() {
        take(1).title = String::new();
    }

    // RED 6: declared in this module only for a test build. Refused.
    #[cfg(test)]
    fn only_in_tests(_: u32) -> &'static mut crate::model::WindowRuntime {
        unimplemented!()
    }

    pub fn a_test_only_declaration_here() {
        only_in_tests(1).title = String::new();
    }

    // RED 7: brought in only by a test build's `use`. Refused.
    pub fn a_test_only_use() {
        probe(1).title = String::new();
    }

    // RED 9 (fold F4): a re-export chain two crates deep — `other`'s `admission` globs
    // `deep`'s. Typed from `deep`'s `admitted`, so nothing of the window is written.
    pub fn through_two_crates() {
        admitted(1).title = String::new();
    }

    // RED 10 (fold F4/F2): a `pub use` cycle. Refused, naming the chain.
    pub fn a_use_cycle() {
        spin(1).title = String::new();
    }

    // Fold F2: `super` past the crate root. Refused, naming it.
    pub fn past_the_root() {
        lost(1).title = String::new();
    }

    // Fold F2: a first segment the rules cannot place. Refused, naming it.
    pub fn an_unknown_first_segment() {
        thing(1).title = String::new();
    }

    // RED 8: brought in by a `use` written in the body, under the name of the package's `grab`.
    // The census reads no `use` of a body. Refused.
    pub fn a_use_of_the_body() {
        use census_reach_other::lend as grab;
        grab(1).title = String::new();
    }
}
