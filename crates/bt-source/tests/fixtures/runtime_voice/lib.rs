//! **The runtime voice's guard** — its fixture.
//!
//! `diagnostics::note` is the voice; `Runtime` is the holder whose `impl` blocks are the runtime
//! roads; `Window::note` is the window's door. Each road below says what the guard makes of it:
//! LISTED (a direct line the list names), PLANTED (a direct line the list does not name, or a
//! shape the guard does not read), or PASSES.

pub mod diagnostics {
    pub fn note(_text: &str) {}
}

/// A `use` that renames the voice: a call through `say` is a call the guard cannot see. PLANTED.
use crate::diagnostics::note as say;

/// A glob over the voice's module: a bare `note` anywhere under it is a call the guard cannot
/// see. PLANTED.
mod opened {
    use crate::diagnostics::*;
}

pub struct Window;

impl Window {
    /// The window's door. Not a runtime road: the guard reads `Runtime`'s blocks.
    pub fn note(&self, line: &str) {
        crate::diagnostics::note(line);
    }
}

pub struct Runtime {
    window: Window,
}

impl Runtime {
    /// LISTED: one direct call, spelled from the crate root.
    pub fn listed(&self) {
        crate::diagnostics::note("listed");
    }

    /// PASSES: the line said through the window's door.
    pub fn through_the_door(&self) {
        self.window.note("through the door");
    }

    /// PLANTED: a direct call the list does not name, inside a closure.
    pub fn planted_call(&self) {
        let speak = || diagnostics::note("planted");
        speak();
    }

    /// PLANTED: the voice handed on as a value, to be said by somebody else.
    pub fn planted_value(&self) {
        hand_on(diagnostics::note);
    }

    /// PLANTED: the voice brought in by a body's `use` and called by its bare name.
    pub fn planted_bare(&self) {
        use crate::diagnostics::note;
        note("bare");
    }

    /// PLANTED: the voice reached through a renamed module.
    pub fn planted_module_rename(&self) {
        use crate::diagnostics as d;
        d::note("renamed module");
    }

    /// PASSES (the guard's limit, said in its header): the rename the module-level `use` gave.
    /// The `use` itself is what is refused.
    pub fn through_the_rename(&self) {
        say("renamed");
    }
}

/// A trait block of the holder is a runtime road too. LISTED, twice.
impl Clone for Runtime {
    fn clone(&self) -> Self {
        crate::diagnostics::note("cloned");
        crate::diagnostics::note("cloned again");
        Self { window: Window }
    }
}

/// PASSES: a free function is no runtime road.
pub fn hand_on(say: fn(&str)) {
    say("handed on");
    diagnostics::note("a free function");
}

#[cfg(test)]
mod tests {
    /// PASSES: a test's block of the holder is not the product.
    impl super::Runtime {
        fn in_a_test(&self) {
            crate::diagnostics::note("a test");
        }
    }
}
