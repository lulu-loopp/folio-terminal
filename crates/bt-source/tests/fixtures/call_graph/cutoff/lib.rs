//! A path chained past the hop limit.

pub fn target() {}

pub mod chain {
    pub use crate::target as link0;
    pub use link0 as link1;
    pub use link1 as link2;
    pub use link2 as link3;
    pub use link3 as link4;
    pub use link4 as link5;
    pub use link5 as link6;
    pub use link6 as link7;
    pub use link7 as link8;
    pub use link8 as link9;
    pub use link9 as link10;
    pub use link10 as link11;
    pub use link11 as link12;
    pub use link12 as link13;
    pub use link13 as link14;
    pub use link14 as link15;
    pub use link15 as link16;
    pub use link16 as link17;
    pub use link17 as link18;
    pub use link18 as link19;
    pub use link19 as link20;
}

pub fn near() {
    chain::link3();
}

pub fn far() {
    chain::link20();
}

pub fn body_glob() {
    use chain::*;
    link20();
}
