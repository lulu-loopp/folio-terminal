//! What `Dialog` reaches outside the guarded scope: no list names this module.

use crate::made_by_a_macro;
use std::mem::swap;

/// (1) A label defined outside the scope. PLANTED where `Dialog::from_a_const` names it.
pub const OUTSIDE_LABEL: &str = "Copy from a const";

/// No word. PASSES.
pub const SEPARATOR: &str = ";";

/// A number. PASSES.
pub const OUTSIDE_ID: u32 = 7;

/// (2) A crate-local macro whose definition holds a word: PLANTED at its call.
macro_rules! outside_label {
    () => {
        "Copy from a macro"
    };
}

/// The same shape, on the guard's `MACROS` list. PASSES.
macro_rules! listed_label {
    () => {
        "Copy from a listed macro"
    };
}

/// A macro with no word. PASSES.
macro_rules! no_words {
    () => {
        1 + 1
    };
}

/// A macro with no literal whose expansion calls a helper: what it runs is unread. PLANTED at its
/// call.
macro_rules! calls_helper {
    () => {
        $crate::reach::helper_label()
    };
}

/// (3) A helper on none of the lists that builds the label. PLANTED.
pub fn helper_label() -> &'static str {
    "Copy from a helper"
}

/// A helper whose literal has a row (an entity, written `&amp;`). PASSES.
pub fn helper_entity() -> &'static str {
    "&amp;"
}

/// A first-party trait a guarded item calls through a trait object.
pub trait Named {
    fn name(&self) -> &'static str;
}

pub struct Formula;

impl Named for Formula {
    /// (3) Reached through `&dyn Named`. PLANTED.
    fn name(&self) -> &'static str {
        "Copy through a trait object"
    }
}

/// A first-party `push`: a candidate of every `.push` on a receiver of no known type.
pub struct Stack;

impl Stack {
    /// Reached only as an ambiguous candidate. PLANTED unless a `BOUNDARY` row stops the walk.
    pub fn push(&mut self, _row: u32) -> &'static str {
        "Copy from an ambiguous candidate"
    }
}

/// (4) A `Drop` a guarded item's scope runs. PLANTED.
pub struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = "Copy from a drop";
    }
}

/// (4) A `Drop` no reached body names. PASSES.
pub struct Unnamed;

impl Drop for Unnamed {
    fn drop(&mut self) {
        let _ = "Copy from an unnamed drop";
    }
}

objc2::define_class!(
    #[unsafe(super(NSObject))]
    #[name = "FixtureTarget"]
    struct Target;

    impl Target {
        /// (5) Reached by its selector. PLANTED (in `class_label`).
        #[unsafe(method(fixtureChosen:))]
        fn chosen(&self) -> &'static str {
            class_label()
        }

        /// (5) Neither its selector nor its type is named. PASSES.
        #[unsafe(method(fixtureUnused:))]
        fn unused(&self) -> &'static str {
            "Copy from an unused method"
        }
    }
);

objc2::define_class!(
    #[unsafe(super(NSObject))]
    #[name = "FixturePanel"]
    struct Panel;

    impl Panel {
        /// (5) Reached by a path naming it. PLANTED.
        fn shown(&self) -> &'static str {
            "Copy from a panel"
        }

        /// (5) Reached with its type: a call of a binding, and a path the package names that no
        /// function answers. PLANTED, each.
        fn unreadable(&self, label: fn() -> String) -> String {
            let _ = crate::Dialog::missing();
            label()
        }

        /// (5) A free call a package `use` brings in that no function of the package answers (a
        /// macro's, say). PLANTED. A foreign one a `use` brings in. PASSES.
        fn generated(&self) {
            made_by_a_macro();
            swap(&mut 1, &mut 2);
        }

        /// (5) A `const` whose initializer names another: the other is not read. PLANTED. A
        /// `const` whose initializer is a literal. PASSES.
        fn from_consts(&self) -> [&'static str; 2] {
            [CHAINED, PLAIN]
        }
    }
);

thread_local! {
    /// (5) Its initializer is read where a guarded item names it.
    static SHOWN: std::cell::Cell<&'static str> = std::cell::Cell::new(thread_label());
}

/// (5) Called from the `thread_local!`'s initializer. PLANTED.
pub fn thread_label() -> &'static str {
    "Copy from a thread-local"
}

/// (5) Called from the selector's function. PLANTED.
pub fn class_label() -> &'static str {
    "Copy from a class method"
}

/// (6) Called from arguments the call graph does not parse, which are refused instead. PASSES.
pub fn macro_argument_label() -> &'static str {
    "Copy from macro arguments"
}

/// (6) Called from `msg_send!`'s argument list, which is read. PLANTED.
pub fn message_label() -> &'static str {
    "Copy from a message"
}

/// (5) Named from `Panel::from_consts`: its initializer names `CHAINED_LABEL`. PLANTED there.
pub const CHAINED: &str = CHAINED_LABEL;

/// Not read: `CHAINED`'s refusal stands for it.
pub const CHAINED_LABEL: &str = "Copy from a chained const";

/// (5) A literal initializer with no word. PASSES.
pub const PLAIN: &str = "*.*";

/// (4) The first hop: its `Drop` names `Inner`.
pub struct Outer;

impl Drop for Outer {
    fn drop(&mut self) {
        let _inner = Inner;
    }
}

/// (4) The second hop. PLANTED.
pub struct Inner;

impl Drop for Inner {
    fn drop(&mut self) {
        let _ = "Copy from a second drop";
    }
}

/// (6) Called from a closure inside `msg_send!`'s arguments. PLANTED.
pub fn closure_label() -> &'static str {
    "Copy from a closure"
}

/// (6) Called from a macro nested inside `msg_send!`'s arguments. PLANTED.
pub fn nested_label() -> &'static str {
    "Copy from a nested macro"
}
