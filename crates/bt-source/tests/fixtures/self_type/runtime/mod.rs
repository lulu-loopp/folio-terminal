//! A method cut out of the root into a newly declared `runtime/` module, which
//! is where the self type has to be written `crate::Runtime`.

impl crate::Runtime<'_> {
    pub fn file_peek_promotes(&self) -> bool {
        !self.name.is_empty()
    }
}
