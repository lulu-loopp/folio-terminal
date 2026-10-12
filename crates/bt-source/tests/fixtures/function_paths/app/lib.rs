//! `FunctionPaths`' fixture, the application crate (`tests/function_paths.rs`): each module is
//! one case of the resolution matrix, and `here` marks where its path is written.

/// (a) A cross-crate re-export, the shape of `bt-app`'s `contained_panic`.
pub mod reexport {
    pub(crate) use fp_platform::boundary::contain;

    pub fn here() {}
}

/// (b) A glob that brings `f` in, unshadowed.
pub mod globbed {
    use crate::provider_one::*;

    pub fn here() {}
}

/// (c) A local `f` beside the same glob: the local one.
pub mod shadowed {
    use crate::provider_one::*;

    pub fn f() {}

    pub fn here() {}
}

/// (d) Two globs that both bring an `f`.
pub mod twice {
    use crate::provider_one::*;
    use crate::provider_two::*;

    pub fn here() {}
}

pub mod provider_one {
    pub fn f() {}
}

pub mod provider_two {
    pub fn f() {}
}

/// (e) A method and a trait function: no free function by those names.
pub mod methods {
    pub struct Thing;

    impl Thing {
        pub fn run(&self) {}
    }

    pub trait Act {
        fn act();
    }

    impl Act for Thing {
        fn act() {}
    }

    pub fn here() {}
}
