//! Plan row 122's GREEN fixture: a `cfg` inside an item whose other attributes rewrite nothing —
//! the language's own, a tool namespace, a `derive` of anything, and a derive helper beside the
//! derive that declares it — is a gate as written. A line that ends `// counted` is product; a
//! line that ends `// gated` is not.

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub struct Settings {
    #[cfg(test)]
    pub the_gated_needle: u8, // gated
    #[serde(default)]
    pub kept: u8,
}

#[derive(Debug, thiserror::Error)]
#[error("a failure")]
pub enum Failure {
    #[cfg(test)]
    #[error("planted")]
    the_gated_needle, // gated
    #[error("kept")]
    Kept,
}

#[allow(dead_code)]
#[inline]
#[must_use]
pub fn plain() -> u8 {
    #[cfg(test)]
    let the_gated_needle = 1; // gated
    let the_gated_needle = 0; // counted
    0
}

#[rustfmt::skip]
pub fn skipped() {
    #[cfg(test)]
    let the_gated_needle = 2; // gated
}

#[cfg_attr(test, allow(unused), clippy::inline_always)]
pub fn conditionally_allowed() {
    #[cfg(test)]
    let the_gated_needle = 3; // gated
}

impl Settings {
    #[inline]
    #[track_caller]
    pub fn method(&self) {
        #[cfg(test)]
        let the_gated_needle = 4; // gated
    }
}

pub trait Shown {
    #[must_use]
    fn shown(&self) -> u8 {
        #[cfg(test)]
        let the_gated_needle = 5; // gated
        0
    }
}

#[macro_use]
pub mod used {
    #[cfg(test)]
    pub fn the_gated_needle() {} // gated
}
