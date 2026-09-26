// The four structs of the census, in miniature, and the two that stand beside
// them in the real tree: `Runtime`, whose own fields are looked up before its
// `Deref` target's, and `SessionStore`, which declares a field of the same name
// as one of the four.
use std::cell::Cell;
use std::collections::BTreeMap;

use crate::gate::DirtyGate;

pub struct App {
    pub gpu: u32,
    pub notes: Vec<String>,
    pub minimized: Cell<bool>,
}

pub struct WindowRuntime {
    pub dirty_gate: DirtyGate,
    pub tabs: Vec<TabState>,
    pub active_tab: usize,
    pub title: String,
    pub hover: Option<u32>,
}

pub struct TabState {
    pub pinned: bool,
    // The same name as `Runtime.window`: a reading that looked through the
    // `Deref` first would take this one.
    pub window: u32,
    pub sessions: BTreeMap<u32, LeafSession>,
}

pub struct LeafSession {
    pub session: u32,
}

pub struct SessionStore {
    pub session: u32,
}

pub struct Runtime<'a> {
    pub app: &'a mut App,
    pub window: &'a mut WindowRuntime,
}

impl std::ops::Deref for Runtime<'_> {
    type Target = TabState;

    fn deref(&self) -> &TabState {
        &self.window.tabs[self.window.active_tab]
    }
}

impl std::ops::DerefMut for Runtime<'_> {
    fn deref_mut(&mut self) -> &mut TabState {
        let at = self.window.active_tab;
        &mut self.window.tabs[at]
    }
}
