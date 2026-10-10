/// A window's physical position on the screen, distinct from a pointer position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowOrigin {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

impl From<WindowOrigin> for winit::dpi::Position {
    fn from(origin: WindowOrigin) -> Self {
        Self::Physical((origin.x, origin.y).into())
    }
}

#[cfg(test)]
mod tests {
    use super::WindowOrigin;
    use winit::dpi::{PhysicalPosition, Position};

    #[test]
    fn converts_to_a_physical_window_position() {
        let origin = WindowOrigin { x: 320, y: 200 };
        assert!(matches!(
            Position::from(origin),
            Position::Physical(position) if position == PhysicalPosition::new(320, 200)
        ));
    }

    #[test]
    fn preserves_a_negative_multimonitor_origin() {
        let origin = WindowOrigin { x: -1920, y: -1080 };
        assert!(matches!(
            Position::from(origin),
            Position::Physical(position) if position == PhysicalPosition::new(-1920, -1080)
        ));
    }
}
