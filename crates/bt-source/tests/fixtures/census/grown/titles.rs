use crate::model::WindowRuntime;

// The new writer.
pub fn name_the_window(window: &mut WindowRuntime) {
    window.title.push_str("Folio");
}
