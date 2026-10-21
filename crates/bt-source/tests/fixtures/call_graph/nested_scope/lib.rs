//! A `fn` written in a body sees the block's items and `use`s, never a same-named module function.

mod other {
    pub fn helper() {}
}

pub fn sibling() {}

pub fn root() {
    use other::helper as from_use;
    fn sibling() {}
    fn nested() {
        sibling();
        from_use();
    }
    nested();
}
