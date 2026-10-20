//! Code on an arm the walk does not take.

#[cfg(test)]
fn test_only() {}

#[cfg(windows)]
fn on_windows() {}

#[cfg(target_os = "macos")]
fn on_macos() {}

pub fn root() {
    test_only();
    #[cfg(test)]
    {
        on_windows();
    }
    on_windows();
    on_macos();
}
