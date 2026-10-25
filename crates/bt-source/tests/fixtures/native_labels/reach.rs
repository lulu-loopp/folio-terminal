//! What `Dialog` reaches outside the guarded scope: no list names this module.

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
