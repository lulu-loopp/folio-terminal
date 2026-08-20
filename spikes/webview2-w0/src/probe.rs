//! The rig every gate runs on: one window, one visual tree, one wgpu overlay
//! above a WebView2 seat, and a loopback server to point it at.
//!
//! The chrome it paints is deliberately the product's shape — a top bar, a left
//! column, a seat inset from both — because the questions being asked are about
//! a *pane inside a window*, and a full-window WebView would answer easier
//! versions of all of them.

use crate::capture::{Image, WindowCapture};
use crate::dcomp::Tree;
use crate::gfx::{Overlay, Rect};
use crate::host::{BoundsOrigin, Evidence, Host, MouseEvent};
use crate::server::Server;
use crate::win::HostWindow;
use anyhow::{Context as _, Result};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use windows::Win32::Foundation::{POINT, RECT};
use windows::core::w;

// The palette. Every one of these is a landmark some gate looks for by colour,
// so they are chosen to be far apart in every channel.
pub const TOP_BAR: [f32; 4] = [0.16, 0.18, 0.22, 1.0];
pub const LEFT_COLUMN: [f32; 4] = [0.13, 0.15, 0.18, 1.0];
/// The frame the host paints immediately outside the seat. A scan line crosses
/// it on its way to the page, and any gap between the two is the tear.
pub const SEAT_BORDER: [f32; 4] = [1.0, 0.784, 0.0, 1.0];
pub const SEAT_BORDER_BGR: [u8; 3] = [0, 200, 255];
/// `#1b6ef3`, the probe page's background.
pub const PAGE_BGR: [u8; 3] = [0xf3, 0x6e, 0x1b];
/// The floating panel, opaque. `#e8552f`.
pub const PANEL: [f32; 4] = [0.91, 0.33, 0.18, 1.0];
pub const PANEL_BGR: [u8; 3] = [0x2f, 0x55, 0xe8];

pub const BORDER: i32 = 4;
pub const TOP_BAR_HEIGHT: i32 = 44;
pub const LEFT_COLUMN_WIDTH: i32 = 220;

pub struct Probe {
    pub window: HostWindow,
    pub tree: Tree,
    pub overlay: Overlay,
    pub host: Host,
    pub evidence: Rc<RefCell<Evidence>>,
    pub server: Server,
    pub seat: RECT,
    pub user_data_folder: PathBuf,
    pub shots: PathBuf,
    /// Extra rectangles the current gate wants painted over the seat.
    pub panels: Vec<Rect>,
    /// How many web messages have already been read out of the evidence table.
    consumed_messages: usize,
}

impl Probe {
    pub fn start(shots: PathBuf, origin: BoundsOrigin) -> Result<Self> {
        std::fs::create_dir_all(&shots).ok();
        let user_data_folder = shots.join("udf");
        std::fs::create_dir_all(&user_data_folder).ok();

        let window = HostWindow::create(w!("Folio W0 WebView2 probe"), 1400, 900)?;
        let (width, height) = window.client_size();
        let tree = Tree::new(window.hwnd)?;
        let overlay = Overlay::new(tree.gpu_visual_ptr(), width, height)?;
        crate::log::emit(
            1,
            "alpha-modes",
            serde_json::json!({
                "offered": overlay.alpha_offered,
                "chosen": overlay.alpha_chosen,
            }),
        );
        let server = Server::start()?;
        let evidence = Evidence::new();
        let (environment, created) = crate::host::environment(&user_data_folder)?;
        crate::log::emit(
            7,
            "environment",
            serde_json::json!({ "created_now": created, "udf": user_data_folder }),
        );
        let host = Host::create(&environment, window.hwnd, Rc::clone(&evidence))?;
        host.attach_environment_events()?;
        host.set_root_visual(&tree.web_visual())?;

        let mut probe = Self {
            window,
            tree,
            overlay,
            host,
            evidence,
            server,
            seat: RECT::default(),
            user_data_folder,
            shots,
            panels: Vec::new(),
            consumed_messages: 0,
        };
        probe.host.set_seat(RECT::default(), origin)?;
        probe.relayout()?;
        Ok(probe)
    }

    /// The seat, derived from the client area the way a pane is derived from a
    /// window: below the bar, right of the column, inset by its own border.
    pub fn seat_for(width: u32, height: u32) -> RECT {
        RECT {
            left: LEFT_COLUMN_WIDTH + BORDER,
            top: TOP_BAR_HEIGHT + BORDER,
            right: width as i32 - BORDER,
            bottom: height as i32 - BORDER,
        }
    }

    pub fn relayout(&mut self) -> Result<()> {
        let (width, height) = self.window.client_size();
        self.overlay.resize(width, height);
        let seat = Self::seat_for(width, height);
        self.move_seat(seat)
    }

    /// Move the seat. One call sets the WebView's bounds, the visual's offset
    /// and the overlay's idea of where not to paint, and the frame that follows
    /// publishes all three in the same commit.
    pub fn move_seat(&mut self, seat: RECT) -> Result<()> {
        self.seat = seat;
        let origin = self.host.origin;
        self.host.set_seat(seat, origin)?;
        match origin {
            BoundsOrigin::AtSeat => self.tree.set_web_offset(0.0, 0.0)?,
            BoundsOrigin::AtZero => {
                self.tree
                    .set_web_offset(seat.left as f32, seat.top as f32)?;
            }
        }
        self.present()
    }

    /// Paint one frame and commit. The seat's interior is left untouched, so it
    /// stays alpha 0 and the page below shows through it.
    pub fn present(&mut self) -> Result<()> {
        let (width, height) = self.window.client_size();
        let seat = self.seat;
        let mut rects = vec![
            Rect {
                x: 0.0,
                y: 0.0,
                width: width as f32,
                height: TOP_BAR_HEIGHT as f32,
                color: TOP_BAR,
            },
            Rect {
                x: 0.0,
                y: TOP_BAR_HEIGHT as f32,
                width: LEFT_COLUMN_WIDTH as f32,
                height: (height as i32 - TOP_BAR_HEIGHT) as f32,
                color: LEFT_COLUMN,
            },
        ];
        // Four strips hugging the seat from outside. Their inner edge is the
        // seat's outer edge exactly, so a scan line leaves the border and
        // arrives at the page with nothing in between — unless the web
        // rectangle is lagging, which is the whole measurement.
        rects.extend([
            Rect {
                x: (seat.left - BORDER) as f32,
                y: (seat.top - BORDER) as f32,
                width: (seat.right - seat.left + BORDER * 2) as f32,
                height: BORDER as f32,
                color: SEAT_BORDER,
            },
            Rect {
                x: (seat.left - BORDER) as f32,
                y: seat.bottom as f32,
                width: (seat.right - seat.left + BORDER * 2) as f32,
                height: BORDER as f32,
                color: SEAT_BORDER,
            },
            Rect {
                x: (seat.left - BORDER) as f32,
                y: seat.top as f32,
                width: BORDER as f32,
                height: (seat.bottom - seat.top) as f32,
                color: SEAT_BORDER,
            },
            Rect {
                x: seat.right as f32,
                y: seat.top as f32,
                width: BORDER as f32,
                height: (seat.bottom - seat.top) as f32,
                color: SEAT_BORDER,
            },
        ]);
        rects.extend(self.panels.iter().copied());
        self.overlay.draw(&rects)?;
        self.tree.commit()
    }

    pub fn pump(&mut self, duration: Duration) {
        crate::win::pump_for(duration, |_| {});
    }

    /// Everything the page has said since the last time this was asked.
    pub fn drain_messages(&mut self) -> Vec<serde_json::Value> {
        let evidence = self.evidence.borrow();
        let all = &evidence.web_messages;
        let fresh: Vec<serde_json::Value> = all[self.consumed_messages.min(all.len())..]
            .iter()
            .filter_map(|text| serde_json::from_str(text).ok())
            .collect();
        drop(evidence);
        self.consumed_messages = self.evidence.borrow().web_messages.len();
        fresh
    }

    /// Wait for the page to say something matching `wanted`, pumping meanwhile.
    pub fn wait_for_message(
        &mut self,
        timeout: Duration,
        mut wanted: impl FnMut(&serde_json::Value) -> bool,
    ) -> Option<serde_json::Value> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            for message in self.drain_messages() {
                if wanted(&message) {
                    return Some(message);
                }
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            self.pump(Duration::from_millis(10));
        }
    }

    /// Navigate and wait for the completion event, returning whether it
    /// succeeded and what URL the engine ended on.
    pub fn navigate(&mut self, url: &str, timeout: Duration) -> (bool, String) {
        let before = self.evidence.borrow().nav_completed.len();
        if let Err(error) = self.host.navigate(url) {
            crate::log::emit(
                0,
                "navigate-error",
                serde_json::json!({ "url": url, "error": error.to_string() }),
            );
            return (false, String::new());
        }
        let evidence = Rc::clone(&self.evidence);
        crate::win::pump_until(timeout, || evidence.borrow().nav_completed.len() > before);
        let evidence = self.evidence.borrow();
        match evidence.nav_completed.get(before) {
            Some(record) => (record.success, record.uri.clone()),
            None => (false, String::new()),
        }
    }

    /// Send the page a command it knows (`focus-field`, `clear`, `caret-rect`).
    pub fn tell_page(&self, command: &str) {
        let json = serde_json::to_string(command).unwrap_or_default();
        let _ = unsafe {
            self.host
                .webview
                .PostWebMessageAsJson(&windows::core::HSTRING::from(json))
        };
    }

    /// A point at a fraction across the seat, in the host's client coordinates.
    pub fn seat_point(&self, fraction_x: f32, fraction_y: f32) -> POINT {
        POINT {
            x: self.seat.left
                + ((self.seat.right - self.seat.left) as f32 * fraction_x).round() as i32,
            y: self.seat.top
                + ((self.seat.bottom - self.seat.top) as f32 * fraction_y).round() as i32,
        }
    }

    pub fn mouse(&mut self, event: MouseEvent, point: POINT, buttons: u32) -> Result<()> {
        self.host.send_mouse(event, point, buttons)?;
        self.pump(Duration::from_millis(30));
        Ok(())
    }

    /// Photograph the window through the OS, save it, and hand back the pixels.
    ///
    /// The waiting either side of the drain is not politeness: the queued frames
    /// are thrown away *after* the compositor has had time to publish the change,
    /// and only then is a frame taken, so what comes back cannot predate the
    /// present that prompted it.
    pub fn shoot(&mut self, capture: &WindowCapture, name: &str) -> Result<Image> {
        self.pump(Duration::from_millis(120));
        let dropped = capture.discard_queued();
        self.pump(Duration::from_millis(120));
        let image = capture.frame(Duration::from_millis(900))?;
        let _ = dropped;
        let path = self.shots.join(format!("{name}.png"));
        image.save_png(&path)?;
        crate::log::emit(
            0,
            "screenshot",
            serde_json::json!({ "name": name, "path": path, "size": [image.width, image.height] }),
        );
        Ok(image)
    }

    pub fn shot_path(&self, name: &str) -> PathBuf {
        self.shots.join(format!("{name}.png"))
    }
}

/// The seat's own rectangle in the capture, found by the border the host paints
/// around it. The host's own colours survive the capture byte for byte, so this
/// landmark needs no calibration.
pub fn border_box(image: &Image) -> Option<(u32, u32, u32, u32)> {
    image.bounding_box(SEAT_BORDER_BGR, 16)
}

/// What the screen actually shows for each landmark.
///
/// **The finding this type exists for**: the host's rectangles come back from
/// Windows.Graphics.Capture with exactly the bytes wgpu wrote, and the web
/// page's do not — `#1b6ef3` is captured as `(71, 114, 234)`. So the page's
/// colours are *read off the screen once* and every later comparison is made
/// against that reading rather than against the CSS the page was written with.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Calibration {
    /// The page's background as captured.
    pub page: [u8; 3],
    /// The page's background as the CSS declares it, for the record.
    pub page_nominal: [u8; 3],
    /// The window class brush, read from inside the seat with the WebView
    /// hidden — the only moment anything but a visual is showing there.
    pub class_background: [u8; 3],
    /// The host's own top bar, read back to prove the host half is exact.
    pub top_bar_captured: [u8; 3],
    pub top_bar_nominal: [u8; 3],
}

impl Calibration {
    pub fn host_colours_are_exact(&self) -> bool {
        crate::capture::near(self.top_bar_captured, self.top_bar_nominal, 2)
    }
}

impl Probe {
    /// Read every landmark off the screen. Must run with the probe page loaded.
    pub fn calibrate(&mut self, capture: &WindowCapture) -> Result<Calibration> {
        // With the WebView hidden the seat shows the window class brush and
        // nothing else, which is the only way to learn what "a hole" looks like.
        self.host.set_visible(false)?;
        self.pump(Duration::from_millis(350));
        let empty = self.shoot(capture, "g1-00-calibration-seat-empty")?;
        let seat = border_box(&empty).context("the seat border was not found in the capture")?;
        let inner = (
            seat.0 + BORDER as u32 + 2,
            seat.1 + BORDER as u32 + 2,
            seat.2 - BORDER as u32 - 2,
            seat.3 - BORDER as u32 - 2,
        );
        let class_background = empty
            .modal_colour(inner.0, inner.1, inner.2, inner.3)
            .context("no modal colour inside the empty seat")?;

        self.host.set_visible(true)?;
        self.pump(Duration::from_millis(450));
        let filled = self.shoot(capture, "g1-00-calibration-seat-filled")?;
        let page = filled
            .modal_colour(inner.0, inner.1, inner.2, inner.3)
            .context("no modal colour inside the filled seat")?;
        // The host's top bar, sampled where it actually is: the seat border's
        // top edge in the capture is client y = TOP_BAR_HEIGHT, so the bar is
        // the band immediately above it. Sampling by capture row instead would
        // land in the window's title bar, which belongs to the shell.
        let bar_row = seat.1.saturating_sub(TOP_BAR_HEIGHT as u32 / 2);
        let top_bar_captured = filled
            .modal_colour(seat.2 / 2, bar_row, seat.2 / 2 + 60, bar_row + 6)
            .unwrap_or([0, 0, 0]);
        let calibration = Calibration {
            page,
            page_nominal: PAGE_BGR,
            class_background,
            top_bar_captured,
            top_bar_nominal: [
                (TOP_BAR[2] * 255.0).round() as u8,
                (TOP_BAR[1] * 255.0).round() as u8,
                (TOP_BAR[0] * 255.0).round() as u8,
            ],
        };
        crate::log::emit(1, "calibration", serde_json::to_value(calibration)?);
        Ok(calibration)
    }

    /// The area of the seat's interior in the capture, in pixels.
    pub fn seat_area(&self, image: &Image) -> Option<u64> {
        let (left, top, right, bottom) = border_box(image)?;
        let width = u64::from(right.saturating_sub(left).saturating_sub(BORDER as u32 * 2));
        let height = u64::from(bottom.saturating_sub(top).saturating_sub(BORDER as u32 * 2));
        Some(width * height)
    }

    /// The class background counted inside the seat: a hole nobody filled.
    ///
    /// This is a count and not a verdict on purpose. A page is entitled to
    /// contain pixels the same colour as the hole — this probe's own page has
    /// a translucent black log panel, which is why the first honest run showed
    /// 107 "holes" in a seat that was completely covered. The gate reads the
    /// count as a *fraction* of the seat.
    pub fn holes_in_seat(&self, image: &Image, calibration: &Calibration) -> Option<u64> {
        let (left, top, right, bottom) = border_box(image)?;
        let mut holes = 0;
        for y in top + BORDER as u32..=bottom.saturating_sub(BORDER as u32) {
            for x in left + BORDER as u32..=right.saturating_sub(BORDER as u32) {
                if crate::capture::near(image.bgr(x, y), calibration.class_background, 6) {
                    holes += 1;
                }
            }
        }
        Some(holes)
    }
}

impl Probe {
    /// Best-effort tidy-up: close the controller, wait for the browser to say it
    /// exited, then remove the user data folder. §4's ordering, exercised on
    /// every run rather than only in the gate that measures it.
    pub fn shutdown(self) -> Result<()> {
        let before = self.evidence.borrow().browser_exited.len();
        self.host.close();
        let evidence = Rc::clone(&self.evidence);
        let exited = crate::win::pump_until(Duration::from_secs(10), || {
            evidence.borrow().browser_exited.len() > before
        });
        crate::log::emit(
            7,
            "shutdown",
            serde_json::json!({ "browser_process_exited": exited }),
        );
        if exited {
            std::fs::remove_dir_all(&self.user_data_folder)
                .context("remove user data folder")
                .ok();
        }
        Ok(())
    }
}
