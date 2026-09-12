//! X-6 probe A. One type from each crate the macOS backend is planned to use,
//! so that `cargo check --target aarch64-apple-darwin` compiles something
//! rather than an empty crate — which is the trap §3 X-6 names.

#[cfg(target_os = "macos")]
pub mod mac {
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplication, NSPasteboard, NSWindow};
    use objc2_core_graphics::CGEventTapLocation as Objc2TapLocation;
    use objc2_foundation::NSString;
    use objc2_quartz_core::CAMetalLayer;
    use objc2_user_notifications::UNUserNotificationCenter;
    use objc2_web_kit::WKWebView;

    use core_graphics::event::CGEventTapLocation as CoreGraphicsTapLocation;

    /// `Retained<T>` makes the compiler look the class up rather than take the
    /// name on trust, so this is a real check and not a spelling test.
    pub fn one_type_from_each() -> usize {
        size_of::<Option<Retained<NSApplication>>>()
            + size_of::<Option<Retained<NSWindow>>>()
            + size_of::<Option<Retained<NSPasteboard>>>()
            + size_of::<Option<Retained<NSString>>>()
            + size_of::<Option<Retained<CAMetalLayer>>>()
            + size_of::<Option<Retained<WKWebView>>>()
            + size_of::<Option<Retained<UNUserNotificationCenter>>>()
            + size_of::<Objc2TapLocation>()
            + size_of::<CoreGraphicsTapLocation>()
    }

    /// block2 carries the completion handlers WebKit and UserNotifications take.
    pub fn takes_a_block(_handler: &RcBlock<dyn Fn()>) {}
}
