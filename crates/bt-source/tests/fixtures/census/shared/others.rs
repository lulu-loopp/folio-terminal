// One function per receiver rule of the census note's revision (b)2 §3.
use crate::gate::Door;
use crate::model::{App, Runtime, SessionStore, TabState, WindowRuntime};

impl Runtime<'_> {
    // `window` is the runtime's own field, though `TabState` declares one too.
    pub fn hover(&mut self) {
        self.window.hover = Some(1);
    }

    // `pinned` is not a field of `Runtime`: it is the front tab's, by `Deref`.
    pub fn pin(&mut self) {
        self.pinned = true;
    }

    // A local alias of the gate, then a write through it.
    pub fn alias(&mut self) {
        let gate = &mut self.window.dirty_gate;
        gate.open(2);
    }
}

// `Door::open` takes `&self`: nothing is written.
pub fn knock(door: &Door) -> bool {
    door.open()
}

// Mutable access, and nothing done with it.
pub fn peek(window: &mut WindowRuntime) -> bool {
    window.tabs.get_mut(0).is_some()
}

// An explicit receiver: a typed parameter.
pub fn retitle(window: &mut WindowRuntime) {
    window.title = String::new();
}

// An indexed element: the tab's field is written, the list is only reached.
pub fn pin_first(window: &mut WindowRuntime) {
    window.tabs[0].pinned = true;
}

// A field of the same name on another struct: not a fact at all.
pub fn file(store: &mut SessionStore) {
    store.session = 1;
}

// A hub's membership.
pub fn adopt(window: &mut WindowRuntime, tab: TabState) {
    window.tabs.push(tab);
}

// Inner mutability.
pub fn minimize(app: &App) {
    app.minimized.set(true);
}

// `map`'s closure returns what the rules cannot type, so the window the second
// closure receives is unknown, and the write through it is listed.
pub fn untyped(windows: &mut Vec<WindowRuntime>) {
    windows
        .iter_mut()
        .map(|window| window)
        .for_each(|window| window.title.clear());
}

pub fn reset(app: &mut App) {
    app.gpu = 0;
}
