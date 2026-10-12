//! **Which trait a path names** — the `TraitResolver` fixture.
//!
//! ```text
//! platform_a, platform_b   two traits named `Point`, each implemented beside it
//! job                      `Driver: Named` (a supertrait), `Named`
//! drivers                  `impl Run for Mac` under `use … Driver as Run`;
//!                          `impl super::job::Driver for Win`
//! globbed                  `impl Driver for Glob` under `use crate::job::*`
//! reexport, user           `pub use traits_dep::seam::Door`, and an impl of it
//! foreign                  `impl std::fmt::Debug`: a trait no package declares
//! ```

mod platform_a {
    pub trait Point {
        fn prepare(&self);
    }

    pub struct Ours;

    impl Point for Ours {
        fn prepare(&self) {}
    }
}

mod platform_b {
    pub trait Point {
        fn prepare(&self);
    }

    pub struct Ours;

    impl Point for Ours {
        fn prepare(&self) {}
    }
}

mod job {
    pub trait Driver: Named + Send
    where
        Self: Sync,
    {
        fn prepare(&self);

        fn say(&self) {}
    }

    pub trait Named {
        fn name(&self) -> &str;
    }
}

mod drivers {
    use crate::job::{Driver as Run, Named};

    pub struct Mac;

    impl Run for Mac {
        fn prepare(&self) {}
    }

    impl Named for Mac {
        fn name(&self) -> &str {
            "mac"
        }
    }

    pub struct Win;

    impl super::job::Driver for Win {
        fn prepare(&self) {}
    }
}

mod globbed {
    use crate::job::*;

    struct Glob;

    impl Driver for Glob {
        fn prepare(&self) {}
    }
}

mod reexport {
    pub use traits_dep::seam::Door;
}

mod user {
    use crate::reexport::Door;

    struct Opened;

    impl Door for Opened {
        fn open(&self) {}
    }
}

mod foreign {
    struct Sent;

    impl std::fmt::Debug for Sent {
        fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            Ok(())
        }
    }
}
