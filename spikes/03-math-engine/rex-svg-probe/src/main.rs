use rex::cairo::CairoBackend;

fn main() {
    let _backend_type = std::any::TypeId::of::<CairoBackend>();
    let _ = rex::parser::parse(r"x = \frac{-b}{2a}");
}
