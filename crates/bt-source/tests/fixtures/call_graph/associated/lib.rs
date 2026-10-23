//! Associated functions: inherent, a trait impl's, a trait default, `Self::`.

pub struct Thing;

impl Thing {
    pub fn new() -> Self {
        Self::helper();
        Thing
    }

    fn helper() {}
}

pub trait Make {
    fn make() -> Self;
    fn made() {}
}

impl Make for Thing {
    fn make() -> Self {
        Thing
    }
}

pub fn root() {
    let _ = Thing::new();
    let _ = Thing::make();
    Thing::made();
}
