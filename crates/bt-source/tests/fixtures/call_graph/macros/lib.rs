//! A macro invocation is an edge of its own; the arguments that parse as expressions are walked.

macro_rules! my_macro {
    ($value:expr) => {
        $value
    };
}

pub fn root() {
    my_macro!(helper());
    my_macro! { not an expression at all };
}

fn helper() {}
