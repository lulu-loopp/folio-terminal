// Plan row 126, fold F4: a third workspace crate, two crates from the subject. `other` re-exports
// this crate's `admission` by a glob, as bt-platform re-exports bt-effects' — the real tree's
// `admitted` comes this way. Its function returns its own `Deep`, which no census struct is.
pub struct Deep {
    pub title: String,
}

pub mod admission {
    pub fn admitted(_: u32) -> &'static mut crate::Deep {
        unimplemented!()
    }
}
