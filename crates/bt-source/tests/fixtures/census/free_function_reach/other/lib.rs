// Plan row 126: another workspace crate. Its functions return its own `Other`, which no census
// struct is, so a call typed from them writes no fact.
pub struct Other {
    pub title: String,
}

mod inner {
    pub fn fetch(_: u32) -> &'static mut crate::Other {
        unimplemented!()
    }
}

// The crate's re-export: `census_reach_other::fetch` is `inner::fetch`.
pub use inner::fetch;

pub mod globbed {
    pub fn spread(_: u32) -> &'static mut crate::Other {
        unimplemented!()
    }
}

// Named like the body's own `fn lend` in the subject: not what that call reaches.
pub fn lend(_: u32) -> &'static mut crate::Other {
    unimplemented!()
}

// A module and a function of one name: `census_reach_other::solve` is the function the crate
// re-exports from its module `solve`.
pub mod solve {
    pub fn solve(_: u32) -> &'static mut crate::Other {
        unimplemented!()
    }
}

pub use solve::solve;

// A module the subject brings into its crate root under its own name.
pub mod attention {
    pub fn kind_mode(_: u32) -> &'static mut crate::Other {
        unimplemented!()
    }
}
