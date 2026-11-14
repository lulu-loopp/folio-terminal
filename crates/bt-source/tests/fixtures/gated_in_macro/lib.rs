//! Plan row 109's fixture: a `cfg` written inside a macro's tokens, read where the macro is a
//! list of attribute-carrying names whose definition this file holds. A line that ends
//! `// counted` is product; a line that ends `// gated` is not.

macro_rules! entries {
    ($( $(#[$attribute:meta])* $name:ident, )*) => {
        #[derive(Clone, Copy)]
        pub enum Entry {
            $( $(#[$attribute])* $name, )*
        }

        #[cfg(test)]
        impl Entry {
            pub const ALL: [Self; 1] = [Self::the_gated_needle]; // gated
        }
    };
}

entries! {
    #[cfg(test)]
    the_gated_needle, // gated
    #[cfg(all(test, windows))]
    the_gated_needle, // gated
    #[cfg(not(test))]
    the_gated_needle, // counted
    #[cfg(any(test, windows))]
    the_gated_needle, // counted
    /// A documented entry with no gate.
    the_gated_needle, // counted
}

pub mod inner {
    entries! {
        #[cfg(test)]
        the_gated_needle, // gated
        the_gated_needle, // counted
    }
}

pub fn in_a_block() {
    macro_rules! local_entries {
        ($( $(#[$a:meta])* $v:ident, )*) => {
            enum Local {
                $( $(#[$a])* $v, )*
            }
        };
    }
    local_entries! {
        #[cfg(test)]
        the_gated_needle, // gated
    }
}
