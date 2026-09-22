//! The type a pin names, written in a module that is not the crate root.

use super::Edge;

/// The application, and the counter that numbers its tabs.
pub struct App {
    /// **The one counter that numbers tabs.** A pin says the counter lives
    /// here; moving this line to another struct has to turn that pin red.
    pub tab_ids: TabIds,
    pub(crate) edge: Edge,
    /// The needle `pane_seat` is written here and once outside every type, so a
    /// scope over this struct can be told from a reading of the whole file.
    pane_seat: u8,
}

pub struct TabIds {
    next: u32,
}

pub struct Wrapper(pub u32);

pub struct Nothing;

impl App {
    /// A method whose name is a field's name: two identities, one spelling.
    pub fn tab_ids(&self) -> &TabIds {
        &self.tab_ids
    }
}

/// The occurrence of the needle that stands outside every type.
pub fn pane_seat() -> u8 {
    1
}
