use std::{
    num::{NonZeroU32, NonZeroUsize},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use bt_doc::LayoutKey;
use bt_pty::{OutputWake, PtySession, PtySize};
use bt_render::{
    DEFAULT_BACKGROUND_RGB, FrameSource, FrameTrigger, GridSize, LatestFrameSlot, PresentOutcome,
    Renderer,
};
use bt_term::DualPlaneSession;
use bt_transcript::DEFAULT_STAGING_QUOTA;
use bt_viewport::{ViewportFrame, ViewportProjection};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalSize},
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    keyboard::{Key, ModifiersState, NamedKey},
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::{Window, WindowId},
};

const INITIAL_WIDTH: f64 = 960.0;
const INITIAL_HEIGHT: f64 = 600.0;
const WINDOW_TITLE: &str = "BetterTerminal M0-alpha";
const STARTUP_PTY_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);
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
    startup_started: Instant,
    trace_startup: bool,
    background_visible: Option<Duration>,
    first_text_presented: bool,
    last_presented_frame: Option<ViewportFrame>,
}

impl Runtime {
    fn create(
        event_loop: &ActiveEventLoop,
        proxy: EventLoopProxy<AppEvent>,
        startup_started: Instant,
    ) -> Result<Self> {
        let trace_startup = std::env::var_os("BT_STARTUP_TRACE").is_some();
        let phase_started = Instant::now();
        let attributes = Window::default_attributes()
            .with_title(WINDOW_TITLE)
            .with_inner_size(LogicalSize::new(INITIAL_WIDTH, INITIAL_HEIGHT))
            // Do not expose the system class brush while the first swapchain image is pending.
            .with_visible(false);
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .context("create native window")?,
        );
        install_theme_class_background(&window)?;
        let window_time = phase_started.elapsed();
        let physical = window.inner_size();
        // Keeping one display-sized back buffer avoids ResizeBuffers while a normal window is
        // dragged larger. DXGI source-size cropping exposes only the current client area.
        let surface_capacity = surface_capacity(
            physical,
            event_loop
                .available_monitors()
                .map(|monitor| monitor.size()),
        );
        let phase_started = Instant::now();
        let renderer = pollster::block_on(Renderer::new(
            Arc::clone(&window),
            physical.width,
            physical.height,
            surface_capacity.width,
            surface_capacity.height,
            window.scale_factor(),
        ))
        .context("initialize wgpu renderer")?;
        let renderer_time = phase_started.elapsed();
        let grid = renderer
            .metrics()
            .grid_for_pixels(physical.width, physical.height);
        let wake: OutputWake = Arc::new(move || {
            let _ = proxy.send_event(AppEvent::PtyOutput);
        });
        let phase_started = Instant::now();
        let pty = PtySession::spawn_default(pty_size(grid, physical), wake)
            .context("spawn default PowerShell in ConPTY")?;
        let pty_time = phase_started.elapsed();
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
            startup_started,
            trace_startup,
            background_visible: None,
            first_text_presented: false,
            last_presented_frame: None,
        };
        if trace_startup {
            let renderer_phases = runtime.renderer.init_timings();
            eprintln!(
                "BT_STARTUP window={}ms adapter={}ms device={}ms surface={}ms fonts={}ms metrics={}ms render_resources={}ms renderer_total={}ms pty_spawn={}ms runtime_ready={}ms",
                window_time.as_millis(),
                renderer_phases.adapter.as_millis(),
                renderer_phases.device.as_millis(),
                renderer_phases.surface_configure.as_millis(),
                renderer_phases.font_system.as_millis(),
                renderer_phases.font_metrics.as_millis(),
                renderer_phases.render_resources.as_millis(),
                renderer_time.as_millis(),
                pty_time.as_millis(),
                startup_started.elapsed().as_millis(),
            );
        }
        runtime.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        // A hidden surface can either accept the clear or report Occluded. In the latter case
        // redraw() republishes the frame; the second call presents immediately after ShowWindow.
        runtime.redraw()?;
        runtime.window.set_visible(true);
        runtime.redraw()?;
        let background_visible = startup_started.elapsed();
        runtime.background_visible = Some(background_visible);
        if trace_startup {
            eprintln!(
                "BT_STARTUP background_visible={}ms",
                background_visible.as_millis()
            );
        }
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
        let resize_trigger = FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Resize,
        };
        let next_grid = self
            .renderer
            .metrics()
            .grid_for_pixels(physical.width, physical.height);
        self.renderer
            .resize(physical.width, physical.height)
            .context("resize renderer presentation source")?;
        // ResizeBuffers discards the DXGI back buffers. Re-present the last complete frame before
        // ConPTY/grid reflow so the replacement swapchain is immediately theme-filled and the
        // existing text stays at its old top-left pixel origin during the resize transaction.
        let stabilized = if let Some(frame) = self.last_presented_frame.as_ref() {
            matches!(
                self.renderer
                    .present(frame, resize_trigger)
                    .context("stabilize resized swapchain with the last complete frame")?,
                PresentOutcome::Presented(_)
            )
        } else {
            false
        };
        if next_grid == self.grid && stabilized {
            return Ok(());
        }
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
        self.publish_frame(resize_trigger)?;
        // Windows dispatches Resized from its modal move/size loop. Present before returning so
        // the compositor spends the shortest possible interval stretching an old grid or showing
        // a just-resized swapchain before its theme clear.
        self.redraw()
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
        let has_text = frame.cells.iter().any(|cell| !cell.text.trim().is_empty());
        match self
            .renderer
            .present(&frame, trigger)
            .context("render terminal frame")?
        {
            PresentOutcome::Presented(receipt) => {
                let latency = receipt.latency();
                if self.trace_startup
                    && matches!(trigger.source, FrameSource::Resize)
                    && let Ok(latency) = latency
                {
                    eprintln!(
                        "BT_RESIZE present={}us columns={} rows={}",
                        latency.event_to_present_call.as_micros(),
                        frame.columns,
                        frame.rows
                    );
                }
                if has_text && !self.first_text_presented {
                    self.first_text_presented = true;
                    if self.trace_startup {
                        let text_visible = self.startup_started.elapsed();
                        eprintln!(
                            "BT_STARTUP first_text_present={}ms",
                            text_visible.as_millis()
                        );
                        if let Some(background_visible) = self.background_visible {
                            self.window
                                .set_title(&startup_trace_title(background_visible, text_visible));
                        }
                    }
                }
                self.last_presented_frame = Some(frame);
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
    startup_started: Instant,
}

impl BetterTerminalApp {
    fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self {
            runtime: None,
            proxy,
            startup_started: Instant::now(),
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
        match Runtime::create(event_loop, self.proxy.clone(), self.startup_started) {
            Ok(runtime) => {
                self.runtime = Some(runtime);
                // Output can already be buffered after the long GPU initialization path. Drain
                // once after installing Runtime instead of waiting for another event-loop turn.
                if let Some(runtime) = self.runtime.as_mut()
                    && let Err(error) = runtime.drain_pty()
                {
                    self.fail(event_loop, error);
                    return;
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + STARTUP_PTY_POLL_INTERVAL,
                ));
            }
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
        if let Err(error) = runtime.drain_pty() {
            self.fail(event_loop, error);
            return;
        }
        event_loop.set_control_flow(match startup_poll_delay(runtime.first_text_presented) {
            Some(delay) => ControlFlow::WaitUntil(Instant::now() + delay),
            None => ControlFlow::Wait,
        });
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
    // M0 intentionally implements only Ctrl+C. Other Ctrl+letter chords are consumed here until
    // the later terminal-keybinding slice defines their byte and command semantics.
    match key {
        Key::Character(text) if text.is_ascii() && !modifiers.control_key() => {
            Some(text.as_bytes().to_vec())
        }
        Key::Named(NamedKey::Enter) => Some(vec![b'\r']),
        Key::Named(NamedKey::Backspace) => Some(vec![0x7f]),
        Key::Named(NamedKey::Tab) => Some(vec![b'\t']),
        Key::Named(NamedKey::Escape) => Some(vec![0x1b]),
        // winit reports the text-producing space key as Named rather than Character.
        Key::Named(NamedKey::Space) if !modifiers.control_key() => Some(vec![b' ']),
        _ => None,
    }
}

fn startup_poll_delay(first_text_presented: bool) -> Option<std::time::Duration> {
    (!first_text_presented).then_some(STARTUP_PTY_POLL_INTERVAL)
}

fn startup_trace_title(background_visible: Duration, first_text_visible: Duration) -> String {
    format!(
        "{WINDOW_TITLE} — bg {}ms · text {}ms",
        background_visible.as_millis(),
        first_text_visible.as_millis()
    )
}

fn install_theme_class_background(window: &Window) -> Result<()> {
    let handle = window.window_handle().context("get Win32 window handle")?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err(anyhow!("bt-app requires a Win32 window handle"));
    };
    bt_platform::install_window_class_background(handle.hwnd, DEFAULT_BACKGROUND_RGB)
        .map_err(|error| anyhow!(error))
        .context("install theme-colored winit class background brush")
}

fn surface_capacity(
    initial: PhysicalSize<u32>,
    monitor_sizes: impl IntoIterator<Item = PhysicalSize<u32>>,
) -> PhysicalSize<u32> {
    monitor_sizes.into_iter().fold(initial, |capacity, size| {
        PhysicalSize::new(
            capacity.width.max(size.width),
            capacity.height.max(size.height),
        )
    })
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
            keyboard_bytes(&Key::Named(NamedKey::Space), ModifiersState::empty()),
            Some(vec![b' '])
        );
        assert_eq!(
            keyboard_bytes(&Key::Character("c".into()), ModifiersState::CONTROL),
            Some(vec![0x03])
        );
        assert_eq!(
            keyboard_bytes(&Key::Character("x".into()), ModifiersState::CONTROL),
            None
        );
        assert_eq!(
            keyboard_bytes(&Key::Character("中".into()), ModifiersState::empty()),
            None
        );
    }

    #[test]
    fn startup_polls_pty_until_the_first_text_frame_is_presented() {
        assert_eq!(startup_poll_delay(false), Some(STARTUP_PTY_POLL_INTERVAL));
        assert_eq!(startup_poll_delay(true), None);
    }

    #[test]
    fn startup_trace_title_is_human_readable_without_console_output() {
        assert_eq!(
            startup_trace_title(Duration::from_millis(682), Duration::from_millis(1089)),
            "BetterTerminal M0-alpha — bg 682ms · text 1089ms"
        );
    }

    #[test]
    fn surface_capacity_covers_every_single_monitor_without_spanning_the_desktop() {
        assert_eq!(
            surface_capacity(
                PhysicalSize::new(960, 600),
                [PhysicalSize::new(1920, 1080), PhysicalSize::new(1440, 2560)],
            ),
            PhysicalSize::new(1920, 2560)
        );
        assert_eq!(
            surface_capacity(PhysicalSize::new(960, 600), []),
            PhysicalSize::new(960, 600)
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
