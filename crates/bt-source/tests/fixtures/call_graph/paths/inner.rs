pub fn deep() {}

pub mod nested {
    pub fn deeper() {
        super::deep();
    }
}
