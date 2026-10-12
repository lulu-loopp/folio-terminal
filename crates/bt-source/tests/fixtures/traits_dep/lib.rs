//! The package the `traits` fixture re-exports a trait from.

pub mod seam {
    pub trait Door {
        fn open(&self);
    }
}
