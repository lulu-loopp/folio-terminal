//! **The native-labels guard** — its fixture.
//!
//! `Menu`'s and `Dialog`'s blocks and the free function `labels` are the guarded items (each test
//! names the ones it guards); `elsewhere` is not, and nothing guarded calls it. Each literal below
//! says what the guard makes of it: PLANTED (refused) or PASSES. `reach.rs` holds what `Dialog`
//! reaches outside the scope.

#[macro_use]
mod reach;

pub struct Menu;

fn show(_title: &str) -> Result<(), String> {
    Ok(())
}

impl Menu {
    /// A literal with a word handed to a surface a person reads. PLANTED.
    pub fn planted_label(&self) -> Result<(), String> {
        show("Copy LaTeX")
    }

    /// A mnemonic's ampersand and a leading space with a small letter. PLANTED, each.
    pub fn hidden_words(&self) -> Result<(), String> {
        show("&Copy")?;
        show(" copy")
    }

    /// The error channel: inside `Err(…)`, a `.map_err` closure and a `.ok_or_else` closure.
    /// PASSES.
    pub fn error_texts(&self, wanted: Option<u32>) -> Result<u32, String> {
        show("").map_err(|error| format!("AppendMenuW: {error}"))?;
        let found = wanted.ok_or_else(|| "CreatePopupMenu answered nothing".to_owned())?;
        if found == 0 {
            return Err(format!("GetCursorPos failed: {found}"));
        }
        Ok(found)
    }

    /// A programmer's text: `.expect`, `panic!`, `unreachable!`, `assert!`,
    /// `debug_assert_eq!`. PASSES. A literal with no word (`";"`, `"*.*"`). PASSES.
    pub fn programmer_texts(&self, wanted: Option<u32>) -> u32 {
        let found = wanted.expect("checked just above");
        assert!(found > 0, "a found row is never zero");
        debug_assert_eq!(found, found, "the same row");
        if found == 1 {
            panic!("one is not a row");
        }
        if found == 2 {
            unreachable!("two is taken above");
        }
        show(";").and(show("*.*")).map_or(found, |()| found)
    }

    /// A diagnostic-looking text in a place the closed list does not name. PLANTED.
    pub fn not_on_the_list(&self) {
        eprintln!("a diagnostic the list does not name");
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
/// PLANTED when the whole file is guarded, by its line, whatever its case.
macro_rules! titled {
    () => {
        "Unseen"
    };
}

/// Not a guarded item, and no guarded item calls it: its literal is not read unless the whole
/// file is. PASSES.
pub fn elsewhere() -> &'static str {
    "Folio"
}

/// A method on a receiver of no known type, as a guarded free function of its own: its
/// first-party candidate `reach::Stack::push` is read. PLANTED, unless a `BOUNDARY` row stops it.
pub fn ambiguous_root(start: &std::ffi::OsStr) {
    let mut units = start.encode_wide().collect::<Vec<_>>();
    units.push(0);
}

pub struct Dialog;

impl Dialog {
    /// (1) A `const` label defined outside the scope, named here. PLANTED. A wordless `const` and
    /// a number. PASS.
    pub fn from_a_const(&self) -> Result<(), String> {
        show(reach::OUTSIDE_LABEL)?;
        show(reach::SEPARATOR)?;
        show(&reach::OUTSIDE_ID.to_string())
    }

    /// (2) A crate-local macro whose definition holds a word. PLANTED. A macro on the `MACROS`
    /// list and a macro with no word. PASS.
    pub fn from_a_macro(&self) -> Result<(), String> {
        show(outside_label!())?;
        show(listed_label!())?;
        show(&no_words!().to_string())
    }

    /// (3) A helper on none of the lists that builds the label. PLANTED. A helper whose literal
    /// has a row. PASSES.
    pub fn from_a_helper(&self) -> Result<(), String> {
        show(reach::helper_label())?;
        show(reach::helper_entity())
    }

    /// (3) A method reached through a first-party trait object. PLANTED.
    pub fn through_a_trait_object(&self, named: &dyn reach::Named) -> Result<(), String> {
        show(named.name())
    }

    /// A call on an `Fn` parameter: what it runs cannot be read. PLANTED.
    pub fn through_a_parameter(&self, label: impl Fn() -> String) -> Result<(), String> {
        show(&label())
    }

    /// The same call, reviewed into the guard's `UNREAD` list. PASSES.
    pub fn through_a_reviewed_parameter(&self, label: impl Fn() -> String) -> Result<(), String> {
        show(&label())
    }

    /// A method on a receiver of no known type: the walk follows every first-party candidate
    /// (`reach::Stack::push`), which a `BOUNDARY` row stops at. PASSES with the row.
    pub fn through_an_ambiguous_method(&self, start: &std::ffi::OsStr) {
        let mut units = start.encode_wide().collect::<Vec<_>>();
        units.push(0);
    }

    /// A path no item of the package answers. PLANTED.
    pub fn through_a_lost_path(&self) -> Result<(), String> {
        show(crate::nowhere::label())
    }
}

#[cfg(test)]
mod tests {
    /// A test module's literal is not the product's. PASSES.
    #[test]
    fn a_test_says_anything() {
        assert_eq!(super::Menu.planted_label(), Ok(()), "Copy LaTeX");
    }
}
