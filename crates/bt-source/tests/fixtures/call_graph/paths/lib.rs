//! Path calls through the module tree.

pub mod inner;

mod renamed {
    pub fn original() {}
}

use renamed::original as alias_name;

pub mod reexport {
    pub use super::inner::deep;
}

mod opened {
    pub fn by_glob() {}
}

use opened::*;

mod two_foreign {
    use alpha::*;
    use beta::*;

    pub fn caller() {
        mystery();
    }
}

mod one_foreign {
    use rayon::prelude::*;

    pub fn caller() {
        par_bridge();
    }
}

pub fn local() {}

pub fn root() {
    local();
    self::local();
    crate::local();
    inner::deep();
    inner::nested::deeper();
    alias_name();
    reexport::deep();
    by_glob();
    cg_other::tools::far();
    cg_other::again();
    drop(1);
    std::fs::remove_file("x");
    {
        use inner::nested::deeper as here;
        here();
    }
    nowhere();
}
