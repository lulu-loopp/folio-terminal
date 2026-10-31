//! Plan row 109: a `cfg` inside a macro's tokens that this reading does not read is refused by
//! its line, never counted. The last block holds the cases that are not refused.

other! {
    #[cfg(test)]
    the_needle,
}

pub fn in_a_body() -> Vec<u8> {
    vec![#[cfg(test)] 1]
}

thread_local! {
    #[cfg(test)]
    static ONE: u8 = 1;
}

outer! {
    inner! {
        #[cfg(test)]
        deep
    }
}

inner_attribute! {
    #![cfg(test)]
}

conditional! {
    #[cfg_attr(test, cfg(any()))]
    the_needle
}

macro_rules! two_arms {
    () => {};
    ($name:ident) => {
        #[cfg(test)]
        fn $name() {}
    };
}

macro_rules! dropping {
    ($( $(#[$attribute:meta])* $name:ident, )*) => {
        pub enum Dropped {
            $( $name, )*
        }
    };
}

dropping! {
    #[cfg(test)]
    the_needle,
}

macro_rules! entries {
    ($( $(#[$attribute:meta])* $name:ident, )*) => {
        pub enum Entry {
            $( $(#[$attribute])* $name, )*
        }
    };
}

entries! {
    #[cfg(test)]
    the_needle another,
}

crate::entries! {
    #[cfg(test)]
    the_needle,
}

shadowing!();

entries! {
    #[cfg(test)]
    the_needle,
}

#[cfg(test)]
mod tests {
    other! {
        #[cfg(test)]
        the_needle,
    }
}

bitflags_like! {
    #[cfg_attr(feature = "serde", derive(Clone))]
    /// Documented.
    pub struct Flags: u8 {}
}

pub fn a_cfg_macro() -> bool {
    cfg!(test)
}

// The review's fold (2026-10-30): a definition a `cfg` keeps out of some build is not read.
#[cfg(windows)]
macro_rules! windows_entries {
    ($( $(#[$a:meta])* $v:ident, )*) => {
        pub enum Windows {
            $( $(#[$a])* $v, )*
        }
    };
}

windows_entries! {
    #[cfg(test)]
    the_needle,
}

// An attribute macro between the definition and the invocation may define another one.
macro_rules! attribute_entries {
    ($( $(#[$a:meta])* $v:ident, )*) => {
        pub enum Attributed {
            $( $(#[$a])* $v, )*
        }
    };
}

#[some_attribute]
pub fn between_an_attribute() {}

attribute_entries! {
    #[cfg(test)]
    the_needle,
}

// So may a derive of anything but the language's own derives.
macro_rules! derive_entries {
    ($( $(#[$a:meta])* $v:ident, )*) => {
        pub enum Derived {
            $( $(#[$a])* $v, )*
        }
    };
}

#[derive(Clone, Serialize)]
pub struct BetweenADerive;

derive_entries! {
    #[cfg(test)]
    the_needle,
}

// And a module a `cfg_attr` gives `macro_use`.
macro_rules! conditional_use_entries {
    ($( $(#[$a:meta])* $v:ident, )*) => {
        pub enum ConditionalUse {
            $( $(#[$a])* $v, )*
        }
    };
}

#[cfg_attr(test, macro_use)]
mod between_a_conditional_use {}

conditional_use_entries! {
    #[cfg(test)]
    the_needle,
}

// A paste into another macro's tokens: `stringify!` keeps the name in every build.
macro_rules! named_entries {
    ($( $(#[$a:meta])* $v:ident, )*) => {
        pub const NAMES: &str = stringify!($( $(#[$a])* $v, )*);
    };
}

named_entries! {
    #[cfg(test)]
    the_needle,
}

// A definition in an inline module is not visible after the module ends.
mod scoped {
    macro_rules! scoped_entries {
        ($( $(#[$a:meta])* $v:ident, )*) => {
            pub enum Scoped {
                $( $(#[$a])* $v, )*
            }
        };
    }
}

scoped_entries! {
    #[cfg(test)]
    the_needle,
}

// The nearest earlier definition wins, not the first.
macro_rules! twice {
    ($( $(#[$a:meta])* $v:ident, )*) => {
        pub enum Twice {
            $( $(#[$a])* $v, )*
        }
    };
}

macro_rules! twice {
    ($($tokens:tt)*) => {};
}

twice! {
    #[cfg(test)]
    the_needle,
}

// A statement macro between a block's definition and its invocation may define another one.
pub fn a_statement_between() {
    macro_rules! block_entries {
        ($( $(#[$a:meta])* $v:ident, )*) => {
            enum Block {
                $( $(#[$a])* $v, )*
            }
        };
    }
    statement_between!();
    block_entries! {
        #[cfg(test)]
        the_needle,
    }
}

// As may a `#[macro_use]` module.
macro_rules! used_entries {
    ($( $(#[$a:meta])* $v:ident, )*) => {
        pub enum Used {
            $( $(#[$a])* $v, )*
        }
    };
}

#[macro_use]
mod between_a_use {}

used_entries! {
    #[cfg(test)]
    the_needle,
}