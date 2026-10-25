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

fn show_bytes(_title: &[u8]) -> Result<(), String> {
    Ok(())
}

fn show_c(_title: &std::ffi::CStr) -> Result<(), String> {
    Ok(())
}

fn show_char(_key: char) -> Result<(), String> {
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

    /// A raw, a byte and a C string, each piece of `concat!`, and a mnemonic `&` at the end.
    /// PLANTED, each. A character literal is skipped, a stated limit. PASSES.
    pub fn other_literal_shapes(&self) -> Result<(), String> {
        show(r"Raw label")?;
        show_bytes(b"Byte label")?;
        show_c(c"C label")?;
        show(concat!("Con", "cat"))?;
        show("Save &")?;
        show_char('Q')
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

/// One text written twice: one `ALLOWED` row allows one of them. PLANTED, the second, unless two
/// rows allow both.
pub fn twice() -> [&'static str; 2] {
    ["Twice", "Twice"]
}

/// A literal inside a `macro_rules!` definition lands in an expansion the reader cannot see.
/// PLANTED when the whole file is guarded, by its line, whatever its case.
macro_rules! titled {
    () => {
        "Unseen"
    };
}

/// An item-position macro whose tokens do not parse as items: what it writes cannot be read.
/// PLANTED when the whole file is guarded.
unreadable_items! { these are -> not items }

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

/// What runs with no call edge, what an item-position macro writes, and macro arguments the
/// call graph does not read.
pub struct Surface;

impl Surface {
    /// (4) A `Drop` that runs at the scope's end: `reach::Guard` is named here. PLANTED (in
    /// `reach`). `reach::Unnamed`'s, which no reached body names, PASSES.
    pub fn through_an_implicit_call(&self) {
        let _guard = reach::Guard;
    }

    /// (4) The second hop: `reach::Outer`'s `Drop` names `reach::Inner`, whose own `Drop` runs
    /// in turn. PLANTED (in `reach::Inner`).
    pub fn through_a_second_implicit_call(&self) {
        let _outer = reach::Outer;
    }

    /// (5) The functions of item-position macros: one by its selector, the ones of a type a path
    /// names, a `thread_local!`'s initializer. PLANTED (in `reach`), with the dynamic call and the
    /// unplaced path the type's functions write. The selector of no function of the package
    /// PASSES, and so does the class function nothing names.
    pub fn through_an_item_macro(&self, panel: &Object) {
        let _ = objc2::sel!(fixtureChosen:);
        let _ = objc2::sel!(foreignSelector:);
        let _ = reach::Panel::shown(panel);
        reach::SHOWN.with(|_| ());
    }

    /// (6) Macro arguments the call graph does not parse: with a call, PLANTED; `msg_send!`'s
    /// `part: argument` list, read — a call in a closure and in a nested macro there too (PLANTED
    /// in `reach`, each); with no call or path, PASSES. A
    /// `macro_rules!` of the package with no literal whose expansion calls a helper. PLANTED.
    pub fn through_macro_arguments(&self, target: &Object) {
        foreign::render!(rows => reach::macro_argument_label());
        let _: () = objc2::msg_send![target, setTitle: reach::message_label()];
        let _: () = objc2::msg_send![target, setAction: Some(1).map(|_| reach::closure_label())];
        let _: () = objc2::msg_send![target, setToolTip: format!("{}", reach::nested_label())];
        foreign::table!(rows => columns);
        calls_helper!();
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
