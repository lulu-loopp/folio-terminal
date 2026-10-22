//! **The native-labels guard** — its fixture.
//!
//! `Menu`'s blocks and the free function `labels` are the guarded items; `elsewhere` is not.
//! Each literal below says what the guard makes of it: PLANTED (refused) or PASSES.

pub struct Menu;

fn show(_title: &str) -> Result<(), String> {
    Ok(())
}

impl Menu {
    /// A capitalised literal handed to a surface a person reads. PLANTED.
    pub fn planted_label(&self) -> Result<(), String> {
        show("Copy LaTeX")
    }

    /// The error channel: inside `Err(…)`, a `.map_err` closure and a `.ok_or_else` closure.
    /// PASSES.
    pub fn error_texts(&self, wanted: Option<u32>) -> Result<u32, String> {
        show("lowercase words are no label's shape")
            .map_err(|error| format!("AppendMenuW: {error}"))?;
        let found = wanted.ok_or_else(|| "CreatePopupMenu answered nothing".to_owned())?;
        if found == 0 {
            return Err(format!("GetCursorPos failed: {found}"));
        }
        Ok(found)
    }

    /// An attribute's literal: a class or link name, not an expression. PASSES.
    #[doc = "Not a label"]
    #[inline]
    pub fn attributed(&self) {}

    /// An ABI's name after `extern`. PASSES.
    pub extern "C" fn called_from_c() {}
}

/// A literal on the allowlist (a format's name). PASSES.
pub fn labels() -> &'static str {
    "JSON"
}

/// A literal inside a `macro_rules!` definition lands in an expansion the reader cannot see.
/// PLANTED, by its line, whatever its case.
macro_rules! titled {
    () => {
        "Unseen"
    };
}

/// Not a guarded item: its literal is not read unless the whole file is. PASSES.
pub fn elsewhere() -> &'static str {
    "Folio"
}

#[cfg(test)]
mod tests {
    /// A test module's literal is not the product's. PASSES.
    #[test]
    fn a_test_says_anything() {
        assert_eq!(super::Menu.planted_label(), Ok(()), "Copy LaTeX");
    }
}
