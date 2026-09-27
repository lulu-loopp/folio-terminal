// Two types with a method called `open`: one writes, one only looks. A rule
// that decided by the method's name alone could not tell them apart.
pub struct DirtyGate {
    pub request: Option<u32>,
}

impl DirtyGate {
    pub fn open(&mut self, request: u32) {
        self.request = Some(request);
    }

    pub fn is_up(&self) -> bool {
        self.request.is_some()
    }
}

pub struct Door;

impl Door {
    pub fn open(&self) -> bool {
        true
    }
}

// A second module writing `App.gpu`, so it has proven writers in two.
pub fn wake(app: &mut crate::model::App) {
    app.gpu = 1;
}
