fn main() {
    let parsed = rex::parser::parse(r"x = \frac{-b}{2a}");
    println!("{parsed:?}");
}
