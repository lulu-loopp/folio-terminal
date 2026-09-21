// Reached by a product declaration and, through `gate`, by a test one.
mod shared;

// Everything under this declaration is test code, transitively.
#[cfg(test)]
mod gate;

// An inline module is a component of the logical module path, so `leaf` lives
// in a directory named after it and not beside this file.
mod outer {
    mod leaf;
}

// Both rules at once: an inline test gate whose child is out of line.
#[cfg(test)]
mod inline_tests {
    mod nested;
}
