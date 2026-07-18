use std::{
    num::NonZeroU32,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Result, bail};
use bt_spike_ime_cjk::{JsonlLogger, LINE_HEIGHT_PX, candidate_grapheme_cells};
use cosmic_text::{Attrs, Buffer, Color, FontSystem, Metrics, Shaping, SwashCache};
use serde_json::json;
use softbuffer::{Context, Surface};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    event::{ElementState, Ime, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, ModifiersState, NamedKey},
    window::{Window, WindowId},
};

const LOGICAL_CELL_WIDTH: f64 = 14.0;
const LOGICAL_LINE_HEIGHT: f64 = 28.0;
const BACKGROUND: u32 = 0x0010_141b;

#[derive(Debug)]
struct Config {
    ime_name: String,
    log_path: PathBuf,
    smoke_duration: Option<Duration>,
}

impl Config {
    fn parse() -> Result<Self> {
        let mut args = std::env::args().skip(1);
        let mut ime_name = None;
        let mut log_path = None;
        let mut smoke_duration = None;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--ime-name" => ime_name = args.next(),
                "--log" => log_path = args.next().map(PathBuf::from),
                "--smoke-ms" => {
                    let value = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--smoke-ms needs a value"))?;
                    smoke_duration = Some(Duration::from_millis(value.parse()?));
                }
                _ => bail!("unknown argument {arg:?}"),
            }
        }
        Ok(Self {
            ime_name: ime_name.unwrap_or_else(|| "UNDECLARED-IME".to_owned()),
            log_path: log_path.unwrap_or_else(|| PathBuf::from("logs/ime-probe.jsonl")),
            smoke_duration,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct CandidateArea {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl CandidateArea {
    fn payload(self) -> serde_json::Value {
        json!({"x": self.x, "y": self.y, "width": self.width, "height": self.height})
    }
}

struct ProbeWindow {
    surface: Surface<Arc<Window>, Arc<Window>>,
    _context: Context<Arc<Window>>,
    window: Arc<Window>,
    committed: String,
    preedit: String,
    cursor_range: Option<(usize, usize)>,
    anchor: PhysicalPosition<i32>,
    last_pointer: PhysicalPosition<i32>,
    candidate_area: CandidateArea,
    track_mouse: bool,
    modifiers: ModifiersState,
    frame_number: u64,
    next_checklist_item: u8,
}

impl ProbeWindow {
    fn new(event_loop: &ActiveEventLoop) -> Result<Self> {
        let attributes = Window::default_attributes()
            .with_title("BetterTerminal M-1 IME probe")
            .with_inner_size(LogicalSize::new(960.0, 620.0));
        let window = Arc::new(event_loop.create_window(attributes)?);
        let context = Context::new(Arc::clone(&window)).map_err(softbuffer_error)?;
        let surface = Surface::new(&context, Arc::clone(&window)).map_err(softbuffer_error)?;
        let scale = window.scale_factor();
        let anchor = PhysicalPosition::new((80.0 * scale) as i32, (430.0 * scale) as i32);
        let mut state = Self {
            surface,
            _context: context,
            window,
            committed: String::new(),
            preedit: String::new(),
            cursor_range: None,
            anchor,
            last_pointer: anchor,
            candidate_area: CandidateArea {
                x: anchor.x,
                y: anchor.y,
                width: (LOGICAL_CELL_WIDTH * scale).ceil() as u32,
                height: (LOGICAL_LINE_HEIGHT * scale).ceil() as u32,
            },
            track_mouse: false,
            modifiers: ModifiersState::empty(),
            frame_number: 0,
            next_checklist_item: 1,
        };
        state.window.set_ime_allowed(true);
        state.recompute_candidate_area();
        Ok(state)
    }

    fn recompute_candidate_area(&mut self) {
        let scale = self.window.scale_factor();
        let cursor_byte = self
            .cursor_range
            .map_or(self.preedit.len(), |range| range.0.min(self.preedit.len()));
        let prefix = self.preedit.get(..cursor_byte).unwrap_or(&self.preedit);
        let prefix_cells = candidate_grapheme_cells(prefix) as f64;
        self.candidate_area = CandidateArea {
            x: self.anchor.x + (prefix_cells * LOGICAL_CELL_WIDTH * scale).round() as i32,
            y: self.anchor.y,
            width: (LOGICAL_CELL_WIDTH * scale).ceil() as u32,
            height: (LOGICAL_LINE_HEIGHT * scale).ceil() as u32,
        };
    }

    fn issue_ime_area(&self, logger: &mut JsonlLogger, reason: &str) -> Result<()> {
        self.window.set_ime_cursor_area(
            PhysicalPosition::new(self.candidate_area.x, self.candidate_area.y),
            PhysicalSize::new(self.candidate_area.width, self.candidate_area.height),
        );
        logger.emit(
            "set_ime_cursor_area",
            json!({"reason": reason, "area": self.candidate_area.payload()}),
        )
    }

    fn frame_payload(&self) -> serde_json::Value {
        json!({
            "frame_number": self.frame_number,
            "committed": self.committed,
            "preedit": self.preedit,
            "cursor_begin": self.cursor_range.map(|range| range.0),
            "cursor_end": self.cursor_range.map(|range| range.1),
            "candidate_area": self.candidate_area.payload(),
            "track_mouse": self.track_mouse,
            "next_checklist_item": self.next_checklist_item,
            "scale_factor": self.window.scale_factor()
        })
    }

    fn draw(
        &mut self,
        font_system: &mut FontSystem,
        swash_cache: &mut SwashCache,
        ime_name: &str,
    ) -> Result<()> {
        let size = self.window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return Ok(());
        };
        self.surface
            .resize(width, height)
            .map_err(softbuffer_error)?;
        let mut pixels = self.surface.buffer_mut().map_err(softbuffer_error)?;
        pixels.fill(BACKGROUND);
        draw_rect(
            &mut pixels,
            width.get(),
            height.get(),
            self.candidate_area,
            0x003c_8cff,
        );
        let scale = self.window.scale_factor() as f32;
        let checklist_status = if self.next_checklist_item <= 10 {
            format!("item {} pending", self.next_checklist_item)
        } else {
            "all 10 items marked; visual table still required".to_owned()
        };
        let status = format!(
            "BetterTerminal M-1 IME probe\n\
             Declared IME: {ime_name}\n\
             Ctrl+Q exit | F2 move caret | F3 mouse tracking | F4 mark item complete\n\
             Checklist: {checklist_status}\n\
             Mouse tracking: {}\n\
             Committed: {}\n\
             Preedit: {}\n\
             Cursor byte range: {:?}\n\
             Candidate area (physical px): x={} y={} w={} h={}\n\n\
             Blue rectangle is exactly the area passed to set_ime_cursor_area.\n\
             Preedit glyphs use natural advance; compare blue-box/candidate relative geometry.\n\
             Type here with a real IME; do not use synthetic SendInput.",
            self.track_mouse,
            self.committed,
            self.preedit,
            self.cursor_range,
            self.candidate_area.x,
            self.candidate_area.y,
            self.candidate_area.width,
            self.candidate_area.height
        );
        draw_text(
            &mut pixels,
            width.get(),
            height.get(),
            font_system,
            swash_cache,
            28,
            24,
            &status,
            17.0 * scale,
            24.0 * scale,
            Color::rgb(225, 232, 240),
        );
        draw_text(
            &mut pixels,
            width.get(),
            height.get(),
            font_system,
            swash_cache,
            self.anchor.x,
            self.anchor.y,
            if self.preedit.is_empty() {
                "<preedit appears here>"
            } else {
                &self.preedit
            },
            20.0 * scale,
            LINE_HEIGHT_PX * scale,
            Color::rgb(255, 211, 105),
        );
        self.window.pre_present_notify();
        pixels.present().map_err(softbuffer_error)?;
        self.frame_number += 1;
        Ok(())
    }
}

struct App {
    config: Config,
    logger: JsonlLogger,
    state: Option<ProbeWindow>,
    font_system: FontSystem,
    swash_cache: SwashCache,
    font_initialization_micros: u128,
    exit_deadline: Option<Instant>,
    shutdown_logged: bool,
    failure: Option<anyhow::Error>,
}

impl App {
    fn new(config: Config) -> Result<Self> {
        let logger = JsonlLogger::create(&config.log_path, config.ime_name.clone())?;
        let started = Instant::now();
        let font_system = FontSystem::new();
        let font_initialization_micros = started.elapsed().as_micros();
        Ok(Self {
            exit_deadline: config
                .smoke_duration
                .map(|duration| Instant::now() + duration),
            config,
            logger,
            state: None,
            font_system,
            swash_cache: SwashCache::new(),
            font_initialization_micros,
            shutdown_logged: false,
            failure: None,
        })
    }

    fn emit(&mut self, event: &str, payload: serde_json::Value) {
        if self.failure.is_none()
            && let Err(error) = self.logger.emit(event, payload)
        {
            self.failure = Some(error);
        }
    }

    fn shutdown(&mut self, reason: &str) {
        if !self.shutdown_logged {
            self.emit("shutdown", json!({"reason": reason}));
            self.shutdown_logged = true;
        }
    }

    fn finish(mut self) -> Result<()> {
        if !self.shutdown_logged {
            self.shutdown("event_loop_returned");
        }
        if let Some(error) = self.failure {
            return Err(error);
        }
        Ok(())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        match ProbeWindow::new(event_loop) {
            Ok(state) => {
                self.emit(
                    "boot",
                    json!({
                        "winit": "0.30.13",
                        "cosmic_text": "0.19.0",
                        "font_initialization_micros": self.font_initialization_micros,
                        "log_path": self.config.log_path,
                        "note": "Windows winit backend uses IMM32 compatibility APIs"
                    }),
                );
                self.state = Some(state);
                let mut area_error = None;
                if let Some(state) = self.state.as_ref() {
                    if let Err(error) = state.issue_ime_area(&mut self.logger, "initial") {
                        area_error = Some(error);
                    }
                    state.window.request_redraw();
                }
                if let Some(error) = area_error {
                    self.failure = Some(error);
                    event_loop.exit();
                }
            }
            Err(error) => {
                self.failure = Some(error.context("create IME probe window"));
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        if state.window.id() != window_id {
            return;
        }
        let mut area_reason = None;
        let mut redraw = false;
        let mut exit_reason = None;
        match event {
            WindowEvent::CloseRequested => exit_reason = Some("close_requested"),
            WindowEvent::RedrawRequested => {
                if let Err(error) = state.draw(
                    &mut self.font_system,
                    &mut self.swash_cache,
                    &self.config.ime_name,
                ) {
                    self.failure = Some(error.context("draw IME probe frame"));
                    exit_reason = Some("draw_failed");
                } else {
                    let payload = state.frame_payload();
                    if let Err(error) = self.logger.emit("frame", payload) {
                        self.failure = Some(error);
                        exit_reason = Some("log_failed");
                    }
                }
            }
            WindowEvent::Ime(ime) => {
                match ime {
                    Ime::Enabled => {
                        if let Err(error) = self.logger.emit("ime_enabled", json!({})) {
                            self.failure = Some(error);
                        }
                        state.recompute_candidate_area();
                        area_reason = Some("ime_enabled");
                    }
                    Ime::Disabled => {
                        state.preedit.clear();
                        state.cursor_range = None;
                        if let Err(error) = self.logger.emit("ime_disabled", json!({})) {
                            self.failure = Some(error);
                        }
                        state.recompute_candidate_area();
                        area_reason = Some("ime_disabled");
                    }
                    Ime::Preedit(text, cursor_range) => {
                        state.preedit = text.clone();
                        state.cursor_range = cursor_range;
                        if let Err(error) = self.logger.emit(
                            "ime_preedit",
                            json!({
                                "text": text,
                                "cursor_begin": cursor_range.map(|range| range.0),
                                "cursor_end": cursor_range.map(|range| range.1)
                            }),
                        ) {
                            self.failure = Some(error);
                        }
                        state.recompute_candidate_area();
                        area_reason = Some("preedit");
                    }
                    Ime::Commit(text) => {
                        state.committed.push_str(&text);
                        state.preedit.clear();
                        state.cursor_range = None;
                        if let Err(error) = self.logger.emit("ime_commit", json!({"text": text})) {
                            self.failure = Some(error);
                        }
                        state.recompute_candidate_area();
                        area_reason = Some("commit");
                    }
                }
                redraw = true;
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                state.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if let Err(error) = handle_key(
                    state,
                    &event,
                    &mut self.logger,
                    &mut area_reason,
                    &mut redraw,
                ) {
                    self.failure = Some(error);
                    exit_reason = Some("keyboard_log_failed");
                }
                if state.modifiers.control_key() && event.logical_key == Key::Character("q".into())
                {
                    exit_reason = Some("ctrl_q");
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                state.last_pointer = PhysicalPosition::new(position.x as i32, position.y as i32);
                if state.track_mouse {
                    state.anchor = state.last_pointer;
                    state.recompute_candidate_area();
                    area_reason = Some("mouse_tracking");
                    redraw = true;
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                state.recompute_candidate_area();
                if let Err(error) = self.logger.emit(
                    "scale_factor_changed",
                    json!({"scale_factor": scale_factor}),
                ) {
                    self.failure = Some(error);
                }
                area_reason = Some("scale_factor");
                redraw = true;
            }
            WindowEvent::Resized(size) => {
                if let Err(error) = self.logger.emit(
                    "resized",
                    json!({"width": size.width, "height": size.height}),
                ) {
                    self.failure = Some(error);
                }
                redraw = true;
            }
            /* Checklist item 8's failure criterion literally reads "丢焦点", and
               the log had no way to say it: losing focus and switching IME both
               surface as ime_disabled plus a commit of whatever was pending, and
               nothing distinguished them. So a raw pinyin string committing on
               alt-tab — real IMM32 behaviour, and worth knowing for a terminal
               where a stray commit lands on a shell prompt — arrived in the
               evidence with no recorded cause. */
            WindowEvent::Focused(focused) => {
                if let Err(error) = self.logger.emit("focus", json!({"focused": focused})) {
                    self.failure = Some(error);
                }
            }
            _ => {}
        }
        if let Some(reason) = area_reason
            && let Err(error) = state.issue_ime_area(&mut self.logger, reason)
        {
            self.failure = Some(error);
            exit_reason = Some("set_ime_cursor_area_failed");
        }
        if redraw {
            state.window.request_redraw();
        }
        if let Some(reason) = exit_reason {
            self.shutdown(reason);
            event_loop.exit();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.failure.is_some() {
            self.shutdown("failure");
            event_loop.exit();
        } else if self
            .exit_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.shutdown("smoke_timeout");
            event_loop.exit();
        } else if let Some(deadline) = self.exit_deadline {
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.shutdown("loop_exiting");
    }
}

fn handle_key(
    state: &mut ProbeWindow,
    event: &KeyEvent,
    logger: &mut JsonlLogger,
    area_reason: &mut Option<&'static str>,
    redraw: &mut bool,
) -> Result<()> {
    /* physical_key is not decoration — without it this log cannot tell one
       IME-consumed key from another. Windows sends VK_PROCESSKEY for anything the
       IME eats, so Backspace and Esc during composition BOTH arrive as
       `logical_key: Named(Process)` and were logged identically. Item 4's evidence
       had to be reconstructed by behavioural signature (preedit cleared with no
       commit) — that is detective work, not evidence.
       It is also the measurement winit #4508 needs (open, filed against 0.30.13 on
       this exact Windows build): the app still gets KeyboardInput during
       composition, and the rule for M0's keyboard layer is **filter on
       logical_key == Process, never dispatch on physical_key while composing** —
       or the shell eats a real Backspace while the IME deletes a pinyin letter.
       That rule cannot be shown to hold from a log that omits the field it is
       about.
       `state` and `repeat` are here for the same reason: an unfalsifiable log is
       one you have to trust. */
    logger.emit(
        "keyboard_input",
        json!({
            "logical_key": format!("{:?}", event.logical_key),
            "physical_key": format!("{:?}", event.physical_key),
            "state": format!("{:?}", event.state),
            "repeat": event.repeat,
            "text": event.text.as_deref(),
            "preedit_active": !state.preedit.is_empty()
        }),
    )?;
    match &event.logical_key {
        Key::Named(NamedKey::F2) => {
            let size = state.window.inner_size();
            let scale = state.window.scale_factor();
            let left = PhysicalPosition::new((80.0 * scale) as i32, (430.0 * scale) as i32);
            let right = PhysicalPosition::new(
                size.width.saturating_sub((240.0 * scale) as u32) as i32,
                (240.0 * scale) as i32,
            );
            state.anchor = if state.anchor.x < size.width as i32 / 2 {
                right
            } else {
                left
            };
            state.recompute_candidate_area();
            *area_reason = Some("f2_move");
            *redraw = true;
        }
        Key::Named(NamedKey::F3) => {
            state.track_mouse = !state.track_mouse;
            if state.track_mouse {
                state.anchor = state.last_pointer;
                state.recompute_candidate_area();
                *area_reason = Some("mouse_tracking_enabled");
            }
            *redraw = true;
        }
        Key::Named(NamedKey::F4) if state.next_checklist_item <= 10 => {
            let item = state.next_checklist_item;
            logger.emit(
                "checklist_item",
                json!({
                    "item": item,
                    "meaning": "operator marked this checklist item complete; visual PASS/FAIL remains in the manual table"
                }),
            )?;
            state.next_checklist_item += 1;
            *redraw = true;
        }
        Key::Named(NamedKey::Backspace) if state.preedit.is_empty() => {
            state.committed.pop();
            *redraw = true;
        }
        Key::Character(character)
            if state.modifiers.control_key() && character.eq_ignore_ascii_case("l") =>
        {
            state.committed.clear();
            *redraw = true;
        }
        _ if state.preedit.is_empty() && !state.modifiers.control_key() => {
            if let Some(text) = &event.text {
                state.committed.push_str(text);
                *redraw = true;
            }
        }
        _ => {}
    }
    Ok(())
}

fn softbuffer_error(error: softbuffer::SoftBufferError) -> anyhow::Error {
    anyhow::anyhow!(error.to_string())
}

fn draw_rect(pixels: &mut [u32], width: u32, height: u32, area: CandidateArea, color: u32) {
    let left = area.x.max(0) as u32;
    let top = area.y.max(0) as u32;
    let right = left.saturating_add(area.width).min(width);
    let bottom = top.saturating_add(area.height).min(height);
    for y in top..bottom {
        for x in left..right {
            if (x == left || x + 1 == right || y == top || y + 1 == bottom)
                && let Some(pixel) = pixels.get_mut((y * width + x) as usize)
            {
                *pixel = color;
            }
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the software text raster helper keeps all target geometry explicit"
)]
fn draw_text(
    pixels: &mut [u32],
    width: u32,
    height: u32,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    origin_x: i32,
    origin_y: i32,
    text: &str,
    font_size: f32,
    line_height: f32,
    color: Color,
) {
    let mut buffer = Buffer::new(font_system, Metrics::new(font_size, line_height));
    buffer.set_size(Some(width as f32), Some(height as f32));
    buffer.set_text(text, &Attrs::new(), Shaping::Advanced, None);
    buffer.draw(
        font_system,
        swash_cache,
        color,
        |x, y, glyph_w, glyph_h, pixel| {
            let (red, green, blue, alpha) = pixel.as_rgba_tuple();
            for dy in 0..glyph_h {
                for dx in 0..glyph_w {
                    let px = origin_x + x + dx as i32;
                    let py = origin_y + y + dy as i32;
                    if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                        continue;
                    }
                    let index = py as usize * width as usize + px as usize;
                    if let Some(destination) = pixels.get_mut(index) {
                        *destination = blend(*destination, red, green, blue, alpha);
                    }
                }
            }
        },
    );
}

fn blend(background: u32, red: u8, green: u8, blue: u8, alpha: u8) -> u32 {
    let alpha = u32::from(alpha);
    let inverse = 255 - alpha;
    let bg_red = (background >> 16) & 0xff;
    let bg_green = (background >> 8) & 0xff;
    let bg_blue = background & 0xff;
    let out_red = (u32::from(red) * alpha + bg_red * inverse) / 255;
    let out_green = (u32::from(green) * alpha + bg_green * inverse) / 255;
    let out_blue = (u32::from(blue) * alpha + bg_blue * inverse) / 255;
    (out_red << 16) | (out_green << 8) | out_blue
}

fn main() -> Result<()> {
    let config = Config::parse()?;
    let event_loop = EventLoop::new()?;
    let mut app = App::new(config)?;
    event_loop.run_app(&mut app)?;
    app.finish()
}
