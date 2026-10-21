//! A helper two calls deep: `root` -> `helper` -> `helper2` -> `std::fs::write`.

pub fn root() {
    helper();
}

fn helper() {
    helper2();
}

fn helper2() {
    let _ = std::fs::write("x", "y");
}
