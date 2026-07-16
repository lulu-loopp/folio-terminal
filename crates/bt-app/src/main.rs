use std::{
    num::{NonZeroU32, NonZeroUsize},
    sync::Arc,
    time::Instant,
};

use anyhow::{Context, Result, anyhow};
use bt_doc::LayoutKey;
use bt_pty::{OutputWake, PtySession, PtySize};
use bt_render::{FrameSource, FrameTrigger, GridSize, LatestFrameSlot, PresentOutcome, Renderer};
use bt_term::DualPlaneSession;
use bt_transcript::DEFAULT_STAGING_QUOTA;
use bt_viewport::ViewportProjection;
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalSize},
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    keyboard::{Key, ModifiersState, NamedKey},
    window::{Window, WindowId},
};

const INITIAL_WIDTH: f64 = 960.0;
const INITIAL_HEIGHT: f64 = 600.0;
/// M0-alpha's single-session frozen-line budget; later configuration work may expose it.
const M0_FROZEN_LINE_QUOTA: NonZeroUsize = NonZeroUsize::new(100_000).unwrap();

#[derive(Clone, Copy, Debug)]
enum AppEvent {
    PtyOutput,
}

struct Runtime {
    renderer: Renderer,
    pty: PtySession,
    session: DualPlaneSession,
    projection: ViewportProjection,
    pending_frames: LatestFrameSlot,
    grid: GridSize,
    modifiers: ModifiersState,
    pending_keyboard_at: Option<Instant>,
    window: Arc<Window>,
}

impl Runtime {
    fn create(event_loop: &ActiveEventLoop, proxy: EventLoopProxy<AppEvent>) -> Result<Self> {
        let attributes = Window::default_attributes()
            .with_title("BetterTerminal M0-alpha")
            .with_inner_size(LogicalSize::new(INITIAL_WIDTH, INITIAL_HEIGHT));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .context("create native window")?,
        );
        let physical = window.inner_size();
        let renderer = pollster::block_on(Renderer::new(
            Arc::clone(&window),
            physical.width,
            physical.height,
            window.scale_factor(),
        ))
        .context("initialize wgpu renderer")?;
        let grid = renderer
            .metrics()
            .grid_for_pixels(physical.width, physical.height);
        let wake: OutputWake = Arc::new(move || {
            let _ = proxy.send_event(AppEvent::PtyOutput);
        });
        let pty = PtySession::spawn_default(pty_size(grid, physical), wake)
            .context("spawn default PowerShell in ConPTY")?;
        let columns = nonzero_u32(grid.columns.get());
        let rows = nonzero_u32(grid.rows.get());
        let mut session = DualPlaneSession::with_quotas_and_cell_height(
            columns,
            rows,
            DEFAULT_STAGING_QUOTA,
            M0_FROZEN_LINE_QUOTA,
            renderer.metrics().cell_height_subpixels(),
        );
        session.set_layout_key(LayoutKey {
            width_cells: columns,
            dpi_milli: renderer.metrics().dpi_milli(),
            font_rev: 1,
            theme_rev: 1,
        });
        let projection = session.new_projection(session.layout_key());
        let mut runtime = Self {
            renderer,
            pty,
            session,
            projection,
            pending_frames: LatestFrameSlot::default(),
            grid,
            modifiers: ModifiersState::default(),
            pending_keyboard_at: None,
            window,
        };
        runtime.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        Ok(runtime)
    }

    fn publish_frame(&mut self, trigger: FrameTrigger) -> Result<()> {
        self.session.refresh_projection(&mut self.projection);
        let frame = self
            .session
            .viewport_frame(&self.projection)
            .context("project terminal grid into viewport frame")?;
        self.pending_frames.publish(frame, trigger);
        self.window.request_redraw();
        Ok(())
    }

    fn drain_pty(&mut self) -> Result<()> {
        let mut changed = false;
        loop {
            let bytes = self.pty.read_output();
            if bytes.is_empty() {
                break;
            }
            debug_assert!(bytes.len() <= bt_pty::TERM_READ_QUANTUM.get());
            self.session.feed(&bytes).context("apply PTY output")?;
            for reply in self.session.take_pty_writes() {
                self.pty
                    .write(&reply)
                    .context("return terminal protocol reply to PTY")?;
            }
            changed = true;
        }
        if changed {
            let keyboard_at = self.pending_keyboard_at.take();
            self.publish_frame(FrameTrigger {
                occurred_at: keyboard_at.unwrap_or_else(Instant::now),
                source: if keyboard_at.is_some() {
                    FrameSource::Keyboard
                } else {
                    FrameSource::PtyOutput
                },
            })?;
        }
        Ok(())
    }

    fn keyboard_input(&mut self, event: &KeyEvent) -> Result<()> {
        if event.state != ElementState::Pressed {
            return Ok(());
        }
        let Some(bytes) = keyboard_bytes(&event.logical_key, self.modifiers) else {
            return Ok(());
        };
        self.pending_keyboard_at = Some(Instant::now());
        self.pty
            .write(&bytes)
            .context("write keyboard input to PTY")
    }

    fn resize(&mut self, physical: PhysicalSize<u32>) -> Result<()> {
        if physical.width == 0 || physical.height == 0 {
            return Ok(());
        }
        self.renderer.resize(physical.width, physical.height);
        let next_grid = self
            .renderer
            .metrics()
            .grid_for_pixels(physical.width, physical.height);
        if next_grid != self.grid {
            self.pty
                .resize(pty_size(next_grid, physical))
                .context("resize ConPTY")?;
            self.session
                .resize(
                    nonzero_u32(next_grid.columns.get()),
                    nonzero_u32(next_grid.rows.get()),
                )
                .context("resize terminal actor")?;
            self.grid = next_grid;
        }
        self.session.set_layout_key(LayoutKey {
            width_cells: nonzero_u32(self.grid.columns.get()),
            dpi_milli: self.renderer.metrics().dpi_milli(),
            font_rev: 1,
            theme_rev: 1,
        });
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Resize,
        })
    }

    fn scale_factor_changed(&mut self, scale_factor: f64) -> Result<()> {
        let metrics = self
            .renderer
            .update_scale_factor(scale_factor)
            .context("remeasure terminal font at new DPI")?;
        self.session
            .set_cell_height_subpixels(metrics.cell_height_subpixels());
        self.resize(self.window.inner_size())
    }

    fn redraw(&mut self) -> Result<()> {
        let Some((frame, trigger)) = self.pending_frames.take() else {
            return Ok(());
        };
        match self
            .renderer
            .present(&frame, trigger)
            .context("render terminal frame")?
        {
            PresentOutcome::Presented(receipt) => {
                let _ = receipt.latency();
            }
            PresentOutcome::Skipped | PresentOutcome::Reconfigure => {
                self.pending_frames.publish(frame, trigger);
                self.window.request_redraw();
            }
        }
        Ok(())
    }

    fn shutdown(&mut self) -> Result<()> {
        self.pty.shutdown().context("shut down child process")?;
        Ok(())
    }
}

struct BetterTerminalApp {
    runtime: Option<Runtime>,
    proxy: EventLoopProxy<AppEvent>,
}

impl BetterTerminalApp {
    fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self {
            runtime: None,
            proxy,
        }
    }
}

impl BetterTerminalApp {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        eprintln!("BetterTerminal stopped: {error:#}");
        if let Some(runtime) = self.runtime.as_mut()
            && let Err(shutdown_error) = runtime.shutdown()
        {
            eprintln!("child shutdown also failed: {shutdown_error:#}");
        }
        self.runtime = None;
        event_loop.exit();
    }
}

impl ApplicationHandler<AppEvent> for BetterTerminalApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.runtime.is_some() {
            return;
        }
        match Runtime::create(event_loop, self.proxy.clone()) {
            Ok(runtime) => self.runtime = Some(runtime),
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::PtyOutput => {
                if let Some(runtime) = self.runtime.as_mut()
                    && let Err(error) = runtime.drain_pty()
                {
                    self.fail(event_loop, error);
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };
        if runtime.window.id() != window_id {
            return;
        }
        let result = match event {
            WindowEvent::CloseRequested => {
                let result = runtime.shutdown();
                self.runtime = None;
                event_loop.exit();
                result
            }
            WindowEvent::KeyboardInput { event, .. } => runtime.keyboard_input(&event),
            WindowEvent::ModifiersChanged(modifiers) => {
                runtime.modifiers = modifiers.state();
                Ok(())
            }
            WindowEvent::Resized(size) => runtime.resize(size),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                runtime.scale_factor_changed(scale_factor)
            }
            WindowEvent::RedrawRequested => runtime.redraw(),
            WindowEvent::Focused(true) | WindowEvent::Occluded(false) => {
                runtime.publish_frame(FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::Expose,
                })
            }
            _ => Ok(()),
        };
        if let Err(error) = result {
            self.fail(event_loop, error);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };
        match runtime.pty.try_wait() {
            Ok(Some(_)) => {
                if let Err(error) = runtime.drain_pty().and_then(|_| runtime.shutdown()) {
                    eprintln!("shell exit cleanup failed: {error:#}");
                }
                self.runtime = None;
                event_loop.exit();
            }
            Ok(None) => {}
            Err(error) => self.fail(event_loop, error.into()),
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(runtime) = self.runtime.as_mut()
            && let Err(error) = runtime.shutdown()
        {
            eprintln!("child shutdown failed: {error:#}");
        }
        self.runtime = None;
    }
}

fn keyboard_bytes(key: &Key, modifiers: ModifiersState) -> Option<Vec<u8>> {
    if modifiers.control_key()
        && matches!(key, Key::Character(text) if text.eq_ignore_ascii_case("c"))
    {
        return Some(vec![0x03]);
    }
    match key {
        Key::Character(text) if text.is_ascii() && !modifiers.control_key() => {
            Some(text.as_bytes().to_vec())
        }
        Key::Named(NamedKey::Enter) => Some(vec![b'\r']),
        Key::Named(NamedKey::Backspace) => Some(vec![0x7f]),
        Key::Named(NamedKey::Tab) => Some(vec![b'\t']),
        Key::Named(NamedKey::Escape) => Some(vec![0x1b]),
        _ => None,
    }
}

fn nonzero_u32(value: u16) -> NonZeroU32 {
    NonZeroU32::new(u32::from(value)).expect("grid dimensions originate from NonZeroU16")
}

fn pty_size(grid: GridSize, physical: PhysicalSize<u32>) -> PtySize {
    PtySize {
        columns: grid.columns,
        rows: grid.rows,
        pixel_width: physical.width.min(u32::from(u16::MAX)) as u16,
        pixel_height: physical.height.min(u32::from(u16::MAX)) as u16,
    }
}

fn main() -> Result<()> {
    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .context("create winit event loop")?;
    let mut application = BetterTerminalApp::new(event_loop.create_proxy());
    event_loop
        .run_app(&mut application)
        .map_err(|error| anyhow!(error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn keyboard_mapping_is_ascii_only_and_preserves_terminal_controls() {
        assert_eq!(
            keyboard_bytes(&Key::Character("hello".into()), ModifiersState::empty()),
            Some(b"hello".to_vec())
        );
        assert_eq!(
            keyboard_bytes(&Key::Named(NamedKey::Enter), ModifiersState::empty()),
            Some(vec![b'\r'])
        );
        assert_eq!(
            keyboard_bytes(&Key::Named(NamedKey::Backspace), ModifiersState::empty()),
            Some(vec![0x7f])
        );
        assert_eq!(
            keyboard_bytes(&Key::Character("c".into()), ModifiersState::CONTROL),
            Some(vec![0x03])
        );
        assert_eq!(
            keyboard_bytes(&Key::Character("中".into()), ModifiersState::empty()),
            None
        );
    }

    #[test]
    fn pty_pixel_size_is_clamped_to_backend_width() {
        let size = pty_size(
            GridSize {
                columns: std::num::NonZeroU16::new(80).unwrap(),
                rows: std::num::NonZeroU16::new(24).unwrap(),
            },
            PhysicalSize::new(100_000, 80_000),
        );
        assert_eq!((size.pixel_width, size.pixel_height), (u16::MAX, u16::MAX));
    }

    #[test]
    fn real_powershell_input_reaches_a_viewport_owned_frame() {
        let columns = std::num::NonZeroU16::new(48).unwrap();
        let rows = std::num::NonZeroU16::new(10).unwrap();
        let mut pty =
            PtySession::spawn_default(PtySize::cells(columns, rows), Arc::new(|| {})).unwrap();
        let mut session = DualPlaneSession::with_quotas_and_cell_height(
            nonzero_u32(columns.get()),
            nonzero_u32(rows.get()),
            DEFAULT_STAGING_QUOTA,
            M0_FROZEN_LINE_QUOTA,
            std::num::NonZeroI64::new(22 * bt_viewport::SUBPIXELS_PER_PX).unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut command_sent = false;
        let mut output_seen = false;
        while Instant::now() < deadline {
            let bytes = pty.read_output();
            if !bytes.is_empty() {
                session.feed(&bytes).unwrap();
                let replies = session.take_pty_writes();
                for reply in &replies {
                    pty.write(reply).unwrap();
                }
                if !command_sent && !replies.is_empty() {
                    pty.write(b"Write-Output ('BT_APP_' + 'INPUT_OK')\r")
                        .unwrap();
                    command_sent = true;
                }
                output_seen = session
                    .terminal()
                    .visible_text()
                    .iter()
                    .any(|line| line.contains("BT_APP_INPUT_OK"));
                if output_seen {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        assert!(
            command_sent,
            "PowerShell never completed its terminal handshake"
        );
        assert!(output_seen, "PowerShell command output never reached Term");
        let projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&projection).unwrap();
        let rendered_text = frame
            .cells
            .iter()
            .map(|cell| cell.text.as_str())
            .collect::<String>();
        assert!(rendered_text.contains("BT_APP_INPUT_OK"));
        pty.write(b"exit\r").unwrap();
        pty.shutdown().unwrap();
    }
}
