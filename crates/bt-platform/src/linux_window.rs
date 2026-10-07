//! Linux window-manager capabilities for winit-owned operations.

/// The window manager that owns a live Linux window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Backend {
    /// Xlib or XCB window managed by an X11 server.
    X11,
    /// Wayland surface managed by a compositor.
    Wayland,
}

impl Backend {
    /// Whether the active native IME context stays enabled while its window is blurred.
    #[must_use]
    pub const fn ime_activation_survives_focus_loss(self) -> bool {
        matches!(self, Self::X11)
    }
}

/// A window operation whose support depends on the backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    /// Ask the window manager to activate this window.
    RequestFocus,
    /// Minimize this window.
    Minimize,
    /// Restore this window from its minimized state.
    Restore,
    /// Set the window's absolute desktop position.
    SetGlobalPosition,
}

/// Explain why an operation cannot be requested from this backend.
///
/// `None` means the caller may use the corresponding winit operation. It does
/// not mean that the window manager accepted or completed that request. This
/// module holds no native handles and never performs the operation itself.
#[must_use]
pub const fn refusal(backend: Backend, operation: Operation) -> Option<&'static str> {
    match (backend, operation) {
        (Backend::X11, _) => None,
        (Backend::Wayland, Operation::RequestFocus) => {
            Some("Wayland requires a compositor activation token to request window focus")
        }
        (Backend::Wayland, Operation::Minimize) => None,
        (Backend::Wayland, Operation::Restore) => {
            Some("winit cannot restore a minimized window on Wayland")
        }
        (Backend::Wayland, Operation::SetGlobalPosition) => {
            Some("Wayland does not expose absolute window positioning")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Backend, Operation, refusal};

    #[test]
    fn wayland_refuses_focus_restore_and_absolute_position_only() {
        assert!(refusal(Backend::Wayland, Operation::RequestFocus).is_some());
        assert_eq!(refusal(Backend::Wayland, Operation::Minimize), None);
        assert!(refusal(Backend::Wayland, Operation::Restore).is_some());
        assert!(refusal(Backend::Wayland, Operation::SetGlobalPosition).is_some());
    }

    #[test]
    fn x11_has_no_policy_refusal_for_winit_window_operations() {
        for operation in [
            Operation::RequestFocus,
            Operation::Minimize,
            Operation::Restore,
            Operation::SetGlobalPosition,
        ] {
            assert_eq!(refusal(Backend::X11, operation), None);
        }
    }

    #[test]
    fn only_x11_keeps_the_native_ime_context_enabled_across_window_blur() {
        assert!(Backend::X11.ime_activation_survives_focus_loss());
        assert!(!Backend::Wayland.ime_activation_survives_focus_loss());
    }
}
