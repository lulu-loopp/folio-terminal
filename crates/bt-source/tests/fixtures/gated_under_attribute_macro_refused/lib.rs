//! Plan row 122's RED fixture: a `cfg` inside an item that carries an attribute macro is refused
//! by its line, because the macro receives the item's tokens and may emit anything. The item
//! itself is not refused. Without a `cfg` inside, nothing is.

#[proc_attr]
pub fn a_let() {
    #[cfg(test)]
    let x = 0;
    let v = vec![#[cfg(test)] 1];
}

#[proc_attr]
pub fn a_read_shape_inside() {
    macro_rules! entries {
        ($( $(#[$a:meta])* $v:ident, )*) => {
            pub enum Entry {
                $( $(#[$a])* $v, )*
            }
        };
    }
    entries! {
        #[cfg(test)]
        x,
    }
}

#[proc_attr]
pub mod a_module {
    #[cfg(test)]
    pub fn t() {}
}

#[cfg_attr(test, proc_attr)]
pub struct Conditional {
    #[cfg(test)]
    pub f: u8,
}

pub struct T;

impl T {
    #[proc_attr]
    pub fn h() {
        #[cfg(test)]
        let y = 0;
    }
}

#[derive(Clone)]
#[serde(rename_all = "lowercase")]
pub struct NoSerdeDerive {
    #[cfg(test)]
    pub f: u8,
}

#[tokio::main]
pub fn main() {
    #[cfg(test)]
    let z = 0;
}

pub trait Hooked {
    #[proc_attr]
    fn hooked(&self) {
        #[cfg(test)]
        let w = 0;
    }
}

extern "C" {
    #[proc_attr]
    #[cfg(test)]
    pub fn gated_before_the_macro();

    #[proc_attr]
    pub fn with_a_gated_parameter(#[cfg(test)] a: u8);
}

#[implement(IUnknown)]
pub struct WithoutAGate {
    pub f: u8,
}
