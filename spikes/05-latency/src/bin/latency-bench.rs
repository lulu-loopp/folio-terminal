use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use bt_spike_latency::{
    Distribution, EchoEvent, LatencySample, SubmitReceipt, TARGET_RATES_HZ, distribution,
};
use serde::Serialize;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId};

const RECORDED_SAMPLES_PER_RATE: usize = 240;
const WARMUP_SAMPLES_PER_RATE: usize = 30;

#[derive(Clone, Copy, Debug)]
enum BenchEvent {
    Echo { event: EchoEvent, record: bool },
    Flood,
}

struct GpuState {
    _window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    adapter: AdapterReport,
    color_phase: u64,
}

impl GpuState {
    fn new(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            ..Default::default()
        }))?;
        let info = adapter.get_info();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("BetterTerminal latency spike"),
                ..Default::default()
            }))?;
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| anyhow!("adapter has no compatible surface configuration"))?;
        let capabilities = surface.get_capabilities(&adapter);
        if capabilities
            .present_modes
            .contains(&wgpu::PresentMode::Immediate)
        {
            // This probe measures the CPU submission boundary. Immediate mode prevents a
            // lower-refresh monitor from turning surface acquisition into an unrelated
            // frame-pacing queue. Photon/display timing is a separate calibration.
            config.present_mode = wgpu::PresentMode::Immediate;
        }
        config.desired_maximum_frame_latency = 1;
        surface.configure(&device, &config);
        Ok(Self {
            _window: window,
            surface,
            device,
            queue,
            config,
            adapter: AdapterReport {
                name: info.name,
                vendor: info.vendor,
                device: info.device,
                device_type: format!("{:?}", info.device_type),
                driver: info.driver,
                driver_info: info.driver_info,
                backend: format!("{:?}", info.backend),
            },
            color_phase: 0,
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    fn clear_and_present(&mut self) -> Result<SubmitReceipt> {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            status => return Err(anyhow!("acquire swapchain texture: {status:?}")),
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("latency clear encoder"),
            });
        self.color_phase = self.color_phase.wrapping_add(1);
        let bright = if self.color_phase.is_multiple_of(2) {
            0.92
        } else {
            0.08
        };
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("latency clear pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bright,
                            g: 0.15,
                            b: 1.0 - bright,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        self.queue.submit([encoder.finish()]);
        let submitted_at = Instant::now();
        self.queue.present(frame);
        Ok(SubmitReceipt {
            submitted_at,
            present_called_at: Instant::now(),
        })
    }
}

#[derive(Serialize)]
struct AdapterReport {
    name: String,
    vendor: u32,
    device: u32,
    device_type: String,
    driver: String,
    driver_info: String,
    backend: String,
}

#[derive(Serialize)]
struct MonitorReport {
    name: Option<String>,
    current_refresh_hz: Option<f64>,
    width_px: u32,
    height_px: u32,
    scale_factor: f64,
}

#[derive(Serialize)]
struct RateReport {
    target_injection_hz: u32,
    samples: usize,
    event_to_submit: Distribution,
    event_to_present_call: Distribution,
}

#[derive(Serialize)]
struct BenchmarkReport {
    schema: &'static str,
    timestamp_unix_seconds: u64,
    measurement_definition: &'static str,
    present_call_warning: &'static str,
    flood_definition: &'static str,
    wgpu_version: &'static str,
    winit_version: &'static str,
    present_mode: String,
    desired_maximum_frame_latency: u32,
    adapter: AdapterReport,
    monitor: MonitorReport,
    flood_events_processed: u64,
    rates: Vec<RateReport>,
    photon_baseline: PhotonReport,
}

#[derive(Serialize)]
struct PhotonReport {
    measured: bool,
    reason: &'static str,
    recommended_alternatives: [&'static str; 2],
    expected_instrument_error: [&'static str; 2],
}

struct App {
    proxy: EventLoopProxy<BenchEvent>,
    output: PathBuf,
    window: Option<Arc<Window>>,
    gpu: Option<GpuState>,
    samples: Vec<LatencySample>,
    flood_events: u64,
    monitor: Option<MonitorReport>,
    stop: Arc<AtomicBool>,
    flood_generation: Arc<AtomicU64>,
    flood_queued: Arc<AtomicBool>,
    started_generators: bool,
    failure: Option<anyhow::Error>,
}

impl App {
    fn new(proxy: EventLoopProxy<BenchEvent>, output: PathBuf) -> Self {
        Self {
            proxy,
            output,
            window: None,
            gpu: None,
            samples: Vec::with_capacity(RECORDED_SAMPLES_PER_RATE * TARGET_RATES_HZ.len()),
            flood_events: 0,
            monitor: None,
            stop: Arc::new(AtomicBool::new(false)),
            flood_generation: Arc::new(AtomicU64::new(0)),
            flood_queued: Arc::new(AtomicBool::new(false)),
            started_generators: false,
            failure: None,
        }
    }

    fn start_generators(&mut self) {
        if self.started_generators {
            return;
        }
        self.started_generators = true;
        spawn_flood(
            self.proxy.clone(),
            self.stop.clone(),
            self.flood_generation.clone(),
            self.flood_queued.clone(),
        );
        spawn_echoes(self.proxy.clone());
    }

    fn record_echo(&mut self, event: EchoEvent, record: bool, event_loop: &ActiveEventLoop) {
        let result = self
            .gpu
            .as_mut()
            .ok_or_else(|| anyhow!("GPU not initialized"))
            .and_then(GpuState::clear_and_present);
        match result {
            Ok(receipt) if record => match LatencySample::from_receipt(event, receipt) {
                Ok(sample) => self.samples.push(sample),
                Err(error) => self.fail(anyhow!(error), event_loop),
            },
            Ok(_) => {}
            Err(error) => self.fail(error, event_loop),
        }
        if self.samples.len() == RECORDED_SAMPLES_PER_RATE * TARGET_RATES_HZ.len() {
            if let Err(error) = self.write_report() {
                self.failure = Some(error);
            }
            self.stop.store(true, Ordering::Release);
            event_loop.exit();
        }
    }

    fn fail(&mut self, error: anyhow::Error, event_loop: &ActiveEventLoop) {
        self.failure = Some(error);
        self.stop.store(true, Ordering::Release);
        event_loop.exit();
    }

    fn write_report(&mut self) -> Result<()> {
        let gpu = self.gpu.take().context("missing GPU report")?;
        let monitor = self.monitor.take().context("missing monitor report")?;
        let mut rates = Vec::new();
        for target_hz in TARGET_RATES_HZ {
            let group = self
                .samples
                .iter()
                .filter(|sample| sample.target_hz == target_hz)
                .collect::<Vec<_>>();
            if group.len() != RECORDED_SAMPLES_PER_RATE {
                return Err(anyhow!(
                    "rate {target_hz} produced {} samples, expected {RECORDED_SAMPLES_PER_RATE}",
                    group.len()
                ));
            }
            rates.push(RateReport {
                target_injection_hz: target_hz,
                samples: group.len(),
                event_to_submit: distribution(group.iter().map(|sample| sample.event_to_submit_us))
                    .context("empty submit distribution")?,
                event_to_present_call: distribution(
                    group.iter().map(|sample| sample.event_to_present_call_us),
                )
                .context("empty present-call distribution")?,
            });
        }
        let report = BenchmarkReport {
            schema: "bt-latency-spike/v1",
            timestamp_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("system clock precedes Unix epoch")?
                .as_secs(),
            measurement_definition: "t0 immediately before EventLoopProxy::send_event; t1 immediately after wgpu Queue::submit returns; includes winit user-event dispatch, clear-pass encoding, surface acquisition, and queue submission; excludes physical keyboard/TSF and scanout",
            present_call_warning: "SurfaceTexture::present call is a CPU enqueue boundary, not proof that a photon changed",
            flood_definition: "a concurrent producer advances output generation at 2000 Hz; at most one Flood event is queued (latest generation wins), each dispatch performs deterministic parser-like CPU work, and every fourth dispatch requests a coalesced competing clear/present",
            wgpu_version: "30.0.0",
            winit_version: "0.30.13",
            present_mode: format!("{:?}", gpu.config.present_mode),
            desired_maximum_frame_latency: gpu.config.desired_maximum_frame_latency,
            adapter: gpu.adapter,
            monitor,
            flood_events_processed: self.flood_events,
            rates,
            photon_baseline: PhotonReport {
                measured: false,
                reason: "no photodiode/high-speed-camera timing device was available in this session",
                recommended_alternatives: [
                    "1000 fps camera recording keyboard LED/physical key and display patch",
                    "photodiode plus microcontroller wired to a hardware input switch",
                ],
                expected_instrument_error: [
                    "1000 fps camera: approximately +/-1 ms quantization, plus frame exposure uncertainty",
                    "photodiode/microcontroller: approximately +/-0.1 ms after calibration; display scan position remains a variable",
                ],
            },
        };
        let json = serde_json::to_string_pretty(&report)?;
        std::fs::write(&self.output, format!("{json}\n"))
            .with_context(|| format!("write {}", self.output.display()))?;
        println!("{json}");
        Ok(())
    }
}

impl ApplicationHandler<BenchEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = WindowAttributes::default()
            .with_title("BetterTerminal latency spike")
            .with_inner_size(LogicalSize::new(640.0, 360.0));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                self.fail(error.into(), event_loop);
                return;
            }
        };
        let size = window.inner_size();
        let current_monitor = window.current_monitor();
        self.monitor = Some(MonitorReport {
            name: current_monitor.as_ref().and_then(|monitor| monitor.name()),
            current_refresh_hz: current_monitor
                .as_ref()
                .and_then(|monitor| monitor.refresh_rate_millihertz())
                .map(|millihertz| f64::from(millihertz) / 1000.0),
            width_px: size.width,
            height_px: size.height,
            scale_factor: window.scale_factor(),
        });
        match GpuState::new(window.clone()) {
            Ok(gpu) => {
                self.window = Some(window);
                self.gpu = Some(gpu);
                self.start_generators();
            }
            Err(error) => self.fail(error, event_loop),
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: BenchEvent) {
        match event {
            BenchEvent::Echo { event, record } => self.record_echo(event, record, event_loop),
            BenchEvent::Flood => {
                let mut value = self.flood_generation.load(Ordering::Acquire);
                self.flood_queued.store(false, Ordering::Release);
                for _ in 0..256 {
                    value = value.rotate_left(7).wrapping_mul(0x9E37_79B9_7F4A_7C15);
                }
                std::hint::black_box(value);
                self.flood_events += 1;
                if self.flood_events.is_multiple_of(4) {
                    self.window
                        .as_ref()
                        .expect("window initialized")
                        .request_redraw();
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.fail(anyhow!("benchmark window was closed"), event_loop)
            }
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(gpu) = &mut self.gpu
                    && let Err(error) = gpu.clear_and_present()
                {
                    self.fail(error, event_loop);
                }
            }
            _ => {}
        }
    }
}

fn spawn_flood(
    proxy: EventLoopProxy<BenchEvent>,
    stop: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    queued: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        while !stop.load(Ordering::Acquire) {
            generation.fetch_add(1, Ordering::Release);
            if !queued.swap(true, Ordering::AcqRel) && proxy.send_event(BenchEvent::Flood).is_err()
            {
                break;
            }
            thread::sleep(Duration::from_micros(500));
        }
    });
}

fn spawn_echoes(proxy: EventLoopProxy<BenchEvent>) {
    thread::spawn(move || {
        let mut sequence = 0_u64;
        for target_hz in TARGET_RATES_HZ {
            let interval = Duration::from_secs_f64(1.0 / f64::from(target_hz));
            let total = WARMUP_SAMPLES_PER_RATE + RECORDED_SAMPLES_PER_RATE;
            let mut deadline = Instant::now() + interval;
            for index in 0..total {
                let now = Instant::now();
                if deadline > now {
                    thread::sleep(deadline - now);
                }
                let event = EchoEvent {
                    sequence,
                    target_hz,
                    injected_at: Instant::now(),
                };
                if proxy
                    .send_event(BenchEvent::Echo {
                        event,
                        record: index >= WARMUP_SAMPLES_PER_RATE,
                    })
                    .is_err()
                {
                    return;
                }
                sequence += 1;
                deadline += interval;
            }
        }
    });
}

fn main() -> Result<()> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("latency-results.json"));
    let event_loop = EventLoop::<BenchEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let mut app = App::new(proxy, output);
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.failure {
        return Err(error);
    }
    Ok(())
}
