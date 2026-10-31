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
