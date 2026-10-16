//! **The resolver's cycles and its hop limit** — the fixture of
//! `the_resolver_reads_a_cycle_as_naming_nothing_more_and_refuses_past_its_limit`.
//!
//! ```text
//! left, right   each opens the other with a glob; `left` declares `Declared`
//! named         imports `outside::outside`, a name under its own name
//! chain         `Link0` … `Link20`, each a renamed re-export of the one before
//! ```

mod left {
    pub use crate::right::*;

    pub trait Declared {}
}

mod right {
    pub use crate::left::*;
}

mod named {
    use outside::outside;
}

mod chain {
    pub use crate::left::Declared as Link0;
    pub use Link0 as Link1;
    pub use Link1 as Link2;
    pub use Link2 as Link3;
    pub use Link3 as Link4;
    pub use Link4 as Link5;
    pub use Link5 as Link6;
    pub use Link6 as Link7;
    pub use Link7 as Link8;
    pub use Link8 as Link9;
    pub use Link9 as Link10;
    pub use Link10 as Link11;
    pub use Link11 as Link12;
    pub use Link12 as Link13;
    pub use Link13 as Link14;
    pub use Link14 as Link15;
    pub use Link15 as Link16;
    pub use Link16 as Link17;
    pub use Link17 as Link18;
    pub use Link18 as Link19;
    pub use Link19 as Link20;
}
