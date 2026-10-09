//! A software-composited Chromium page hosted in Folio's existing preview pane.
//!
//! The window thread owns the policy closures and the host's small local
//! state. A named platform worker owns Chromium, its private CDP pipe, targets,
//! request interception and screencast frames. The worker only posts plain
//! values back; `drain` calls the app's closures on the window thread and sends
//! their verdicts back to the paused browser request.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use super::{
    NativeWindow, PageVisual, RehostCompensation, RehostOutcome, RehostSide, RehostStep, WebChord,
    WebColorScheme, WebDpiOwnership, WebEvent, WebFrame, WebImeEvent, WebInstallReport,
    WebKeyEvent, WebMouseEvent, WebNavigationVerdict, WebRequestVerdict,
};
use crate::admission::{WaitToken, doors};
use crate::{Compositor, EnvironmentAnswer, WebWarmUp};

use self::actor::{ActorCommand, ActorHandle, ActorNotice, GateKey, GateReply, HostId, HostInbox};

#[path = "linux_webview_actor.rs"]
mod actor;

static NEXT_HOST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

struct Shared {
    id: HostId,
    events: RefCell<VecDeque<WebEvent>>,
    gate: Box<dyn Fn(&str) -> WebNavigationVerdict>,
    request_gate: Box<dyn Fn(&str) -> WebRequestVerdict>,
    wake: Arc<dyn Fn() + Send + Sync>,
    inbox: Arc<HostInbox>,
    sender: Mutex<Option<ActorHandle>>,
    request_rules: RefCell<String>,
    page: Cell<Option<PageVisual>>,
    generation: Cell<Option<u64>>,
    report: RefCell<WebInstallReport>,
    frame: RefCell<Option<WebFrame>>,
    frame_sequence: Cell<u64>,
    bounds: Cell<(i32, i32, u32, u32)>,
    visible: Cell<bool>,
    scale: Cell<f64>,
    zoom: Cell<f64>,
    color_scheme: Cell<Option<WebColorScheme>>,
    controller: Cell<bool>,
    closed: Cell<bool>,
    browser_process_id: Cell<u32>,
}

/// One page seat backed by the Linux Chromium actor.
pub struct WebHost {
    shared: Rc<Shared>,
}

impl WebHost {
    /// Create the host without starting a process. The two policy closures stay
    /// on the caller's thread; the wake callback may be called by the browser
    /// actor thread.
    #[must_use]
    pub fn new(
        gate: Box<dyn Fn(&str) -> WebNavigationVerdict>,
        request_gate: Box<dyn Fn(&str) -> WebRequestVerdict>,
        wake: Box<dyn Fn() + Send + Sync>,
    ) -> Self {
        let id = NEXT_HOST.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self {
            shared: Rc::new(Shared {
                id,
                events: RefCell::new(VecDeque::new()),
                gate,
                request_gate,
                wake: Arc::from(wake),
                inbox: Arc::new(HostInbox::default()),
                sender: Mutex::new(None),
                request_rules: RefCell::new(String::new()),
                page: Cell::new(None),
                generation: Cell::new(None),
                report: RefCell::new(WebInstallReport {
                    guards: super::WebGuards::none(),
                    unapplied: Vec::new(),
                }),
                frame: RefCell::new(None),
                frame_sequence: Cell::new(0),
                bounds: Cell::new((0, 0, 0, 0)),
                visible: Cell::new(false),
                scale: Cell::new(1.0),
                zoom: Cell::new(1.0),
                color_scheme: Cell::new(None),
                controller: Cell::new(false),
                closed: Cell::new(false),
                browser_process_id: Cell::new(0),
            }),
        }
    }

    /// Drain actor notices and apply the app-owned gates on the window thread.
    #[must_use]
    pub fn drain(&self) -> Vec<WebEvent> {
        let notices = self.shared.inbox.take();
        for notice in notices {
            match notice {
                ActorNotice::Event { generation, event } => {
                    if self.accepts(generation) {
                        self.shared.events.borrow_mut().push_back(event);
                    }
                }
                ActorNotice::Environment {
                    generation,
                    error,
                    browser_process_id,
                } => {
                    if self.accepts(generation) {
                        self.shared
                            .browser_process_id
                            .set(browser_process_id.unwrap_or(0));
                        self.shared
                            .events
                            .borrow_mut()
                            .push_back(WebEvent::Environment { generation, error });
                    }
                }
                ActorNotice::Controller { generation, error } => {
                    if self.accepts(generation) {
                        self.shared.controller.set(error.is_none());
                        self.shared
                            .events
                            .borrow_mut()
                            .push_back(WebEvent::Controller { generation, error });
                    }
                }
                ActorNotice::Installed { generation, report } => {
                    if self.accepts(generation) {
                        *self.shared.report.borrow_mut() = report;
                    }
                }
                ActorNotice::Frame(frame) => {
                    if self.accepts(frame.generation)
                        && self.shared.page.get() == Some(frame.page)
                        && frame.sequence > self.shared.frame_sequence.get()
                    {
                        self.shared.frame_sequence.set(frame.sequence);
                        *self.shared.frame.borrow_mut() = Some(frame);
                    }
                }
                ActorNotice::NavigationRequest {
                    key,
                    generation,
                    uri,
                } => {
                    if !self.accepts(generation) {
                        self.reply(
                            key,
                            generation,
                            GateReply::Navigation(WebNavigationVerdict::Cancel),
                        );
                        continue;
                    }
                    let verdict = (self.shared.gate)(&uri);
                    let cancelled = !matches!(verdict, WebNavigationVerdict::Proceed);
                    self.shared
                        .events
                        .borrow_mut()
                        .push_back(WebEvent::NavigationStarting { uri, cancelled });
                    self.reply(key, generation, GateReply::Navigation(verdict));
                }
                ActorNotice::ResourceRequest {
                    key,
                    generation,
                    uri,
                } => {
                    if !self.accepts(generation) {
                        self.reply(
                            key,
                            generation,
                            GateReply::Resource(WebRequestVerdict::Refuse),
                        );
                        continue;
                    }
                    let verdict = (self.shared.request_gate)(&uri);
                    if verdict == WebRequestVerdict::Refuse {
                        self.shared
                            .events
                            .borrow_mut()
                            .push_back(WebEvent::RequestRefused { uri });
                    }
                    self.reply(key, generation, GateReply::Resource(verdict));
                }
            }
        }
        self.shared.events.borrow_mut().drain(..).collect()
    }

    fn accepts(&self, generation: u64) -> bool {
        !self.shared.closed.get() && self.shared.generation.get() == Some(generation)
    }

    fn reply(&self, key: GateKey, generation: u64, reply: GateReply) {
        if let Some(sender) = self.actor_sender() {
            let _ = sender.send(ActorCommand::GateVerdict {
                host: self.shared.id,
                generation,
                key,
                reply,
            });
        }
    }

    fn actor_sender(&self) -> Option<ActorHandle> {
        self.shared
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn send(&self, command: ActorCommand) -> Result<(), String> {
        let sender = self
            .actor_sender()
            .ok_or_else(|| String::from("the Chromium actor has not been started"))?;
        sender
            .send(command)
            .map_err(|_| String::from("the Chromium actor has stopped"))
    }

    /// The colour scheme this host was last told.
    #[must_use]
    pub fn color_scheme(&self) -> Option<WebColorScheme> {
        self.shared.color_scheme.get()
    }

    pub fn set_color_scheme(&self, scheme: WebColorScheme) -> Result<(), String> {
        self.shared.color_scheme.set(Some(scheme));
        if self.shared.controller.get() {
            self.send(ActorCommand::ColorScheme {
                host: self.shared.id,
                scheme,
            })?;
        }
        Ok(())
    }

    pub fn set_claimed_chords(&self, chords: Vec<WebChord>) {
        let _ = chords;
    }

    /// Keep the Linux DNR socket rule aligned with the same content rule list
    /// that Windows and macOS receive.
    pub fn set_request_rules(&self, rules: &str) -> Result<(), String> {
        *self.shared.request_rules.borrow_mut() = rules.to_owned();
        if self.shared.controller.get() {
            self.send(ActorCommand::RequestRules {
                host: self.shared.id,
                rules: rules.to_owned(),
            })?;
        }
        Ok(())
    }

    #[must_use]
    pub fn has_controller(&self) -> bool {
        self.shared.controller.get()
    }

    pub fn request_environment(
        &mut self,
        token: WaitToken<'_, doors::WebEnvironment>,
        folder: &Path,
        generation: u64,
    ) -> Result<(), String> {
        let _ = token;
        let mut sender = self
            .shared
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if sender.is_none() {
            *sender = Some(actor::start()?);
        }
        self.shared.generation.set(Some(generation));
        self.shared.closed.set(false);
        sender
            .as_ref()
            .ok_or_else(|| String::from("the Chromium actor did not start"))?
            .send(ActorCommand::Boot {
                host: self.shared.id,
                folder: folder.to_path_buf(),
                generation,
                rules: self.shared.request_rules.borrow().clone(),
                color_scheme: self.shared.color_scheme.get(),
                inbox: Arc::clone(&self.shared.inbox),
                wake: Arc::clone(&self.shared.wake),
            })
            .map_err(|_| String::from("the Chromium actor has stopped"))
    }

    pub fn request_controller(
        &mut self,
        token: WaitToken<'_, doors::WebController>,
        window: NativeWindow,
        generation: u64,
    ) -> Result<(), String> {
        let _ = (token, window);
        self.shared.generation.set(Some(generation));
        self.shared.frame_sequence.set(0);
        self.send(ActorCommand::CreatePage {
            host: self.shared.id,
            generation,
            bounds: self.shared.bounds.get(),
            scale: self.shared.scale.get(),
            visible: self.shared.visible.get(),
            color_scheme: self.shared.color_scheme.get(),
            rules: self.shared.request_rules.borrow().clone(),
        })
    }

    pub fn install(
        &mut self,
        compositor: &Compositor,
        page: PageVisual,
        generation: u64,
    ) -> Result<WebInstallReport, String> {
        let _ = compositor;
        if !self.accepts(generation) || !self.shared.controller.get() {
            return Err(String::from("the Chromium page is not ready to install"));
        }
        self.shared.page.set(Some(page));
        self.send(ActorCommand::Install {
            host: self.shared.id,
            page,
            generation,
        })?;
        Ok(self.shared.report.borrow().clone())
    }

    pub fn rehost(
        &mut self,
        token: WaitToken<'_, doors::WebRehost>,
        from: &RehostSide<'_>,
        to: &RehostSide<'_>,
        rect: (i32, i32, u32, u32),
        visible: bool,
    ) -> RehostOutcome {
        let _ = (token, from, rect);
        if !self.shared.controller.get() || self.shared.closed.get() {
            return RehostOutcome::KeptSource {
                failed_at: RehostStep::Hide,
                error: String::from("there is no live Chromium page to move"),
                compensation: RehostCompensation::default(),
            };
        }
        if let Err(error) = self.send(ActorCommand::MovePage {
            host: self.shared.id,
            page: to.page,
            visible,
        }) {
            return RehostOutcome::KeptSource {
                failed_at: RehostStep::Hide,
                error,
                compensation: RehostCompensation::default(),
            };
        }
        // The raster slot is only valid for the window whose bounds and scale produced it. The
        // actor starts a fresh screencast after the destination sends its placement.
        self.shared.frame.borrow_mut().take();
        self.shared.page.set(Some(to.page));
        self.shared.visible.set(visible);
        RehostOutcome::Moved
    }

    pub fn set_bounds(&self, x: i32, y: i32, width: u32, height: u32) -> Result<(), String> {
        self.shared.bounds.set((x, y, width, height));
        if self.shared.controller.get() {
            self.send(ActorCommand::Bounds {
                host: self.shared.id,
                bounds: (x, y, width, height),
                scale: self.shared.scale.get(),
            })?;
        }
        Ok(())
    }

    pub fn set_rasterization_scale(&self, scale: f64) -> Result<(), String> {
        if !scale.is_finite() || scale <= 0.0 {
            return Err(String::from("the web rasterization scale must be positive"));
        }
        self.shared.scale.set(scale);
        if self.shared.controller.get() {
            self.send(ActorCommand::Bounds {
                host: self.shared.id,
                bounds: self.shared.bounds.get(),
                scale,
            })?;
        }
        Ok(())
    }

    pub fn notify_parent_window_moved(&self) -> Result<(), String> {
        Ok(())
    }

    pub fn set_visible(&self, visible: bool) -> Result<(), String> {
        self.shared.visible.set(visible);
        if self.shared.controller.get() {
            self.send(ActorCommand::Visible {
                host: self.shared.id,
                visible,
            })?;
        }
        Ok(())
    }

    pub fn navigate(&self, url: &str) -> Result<(), String> {
        self.send(ActorCommand::Navigate {
            host: self.shared.id,
            url: url.to_owned(),
        })
    }

    pub fn reload(&self) -> Result<(), String> {
        self.send(ActorCommand::Reload {
            host: self.shared.id,
        })
    }

    pub fn stop(&self) -> Result<(), String> {
        self.send(ActorCommand::StopLoading {
            host: self.shared.id,
        })
    }

    pub fn go_back(&self) -> Result<(), String> {
        self.send(ActorCommand::History {
            host: self.shared.id,
            direction: -1,
        })
    }

    pub fn go_forward(&self) -> Result<(), String> {
        self.send(ActorCommand::History {
            host: self.shared.id,
            direction: 1,
        })
    }

    pub fn open_dev_tools(&self) -> Result<(), String> {
        Err(String::from(
            "developer tools are not part of the Linux preview pane",
        ))
    }

    #[must_use]
    pub fn zoom(&self) -> f64 {
        self.shared.zoom.get()
    }

    pub fn set_zoom(&self, factor: f64) -> Result<(), String> {
        if !factor.is_finite() || factor <= 0.0 {
            return Err(String::from("the web zoom must be positive"));
        }
        self.shared.zoom.set(factor);
        self.send(ActorCommand::Zoom {
            host: self.shared.id,
            factor,
        })
    }

    pub fn find(&self, term: &str, case_sensitive: bool) -> Result<(), String> {
        self.send(ActorCommand::Find {
            host: self.shared.id,
            term: term.to_owned(),
            case_sensitive,
        })
    }

    pub fn find_step(&self, forwards: bool) -> Result<(), String> {
        self.send(ActorCommand::FindStep {
            host: self.shared.id,
            forwards,
        })
    }

    pub fn find_stop(&self) -> Result<(), String> {
        self.send(ActorCommand::FindStop {
            host: self.shared.id,
        })
    }

    pub fn focus_page(&self) -> Result<(), String> {
        self.send(ActorCommand::Focus {
            host: self.shared.id,
        })
    }

    pub fn send_mouse(
        &self,
        event: WebMouseEvent,
        point: (i32, i32),
        buttons_down: u32,
    ) -> Result<(), String> {
        self.send(ActorCommand::Mouse {
            host: self.shared.id,
            event,
            point,
            buttons_down,
        })
    }

    pub fn send_key(&self, event: WebKeyEvent) -> Result<(), String> {
        self.send(ActorCommand::Key {
            host: self.shared.id,
            event,
        })
    }

    pub fn send_ime(&self, event: WebImeEvent) -> Result<(), String> {
        self.send(ActorCommand::Ime {
            host: self.shared.id,
            event,
        })
    }

    #[must_use]
    pub fn take_frame(&self) -> Option<WebFrame> {
        self.shared.frame.borrow_mut().take()
    }

    pub fn capture_preview(&self) -> Result<(), String> {
        self.send(ActorCommand::Capture {
            host: self.shared.id,
        })
    }

    pub fn get_favicon(&self) -> Result<(), String> {
        self.send(ActorCommand::Favicon {
            host: self.shared.id,
        })
    }

    pub fn close_pending_controller(&mut self) {
        if let (Some(generation), Some(sender)) =
            (self.shared.generation.get(), self.actor_sender())
        {
            let _ = sender.send(ActorCommand::CancelCreate {
                host: self.shared.id,
                generation,
            });
        }
    }

    #[must_use]
    pub fn has_orphans(&self) -> bool {
        false
    }

    #[must_use]
    pub fn has_events(&self) -> bool {
        !self.shared.events.borrow().is_empty() || self.shared.inbox.has_events()
    }

    pub fn close(&mut self) {
        if self.shared.closed.replace(true) {
            return;
        }
        self.shared.controller.set(false);
        self.shared.frame.borrow_mut().take();
        if let Some(sender) = self.actor_sender() {
            let _ = sender.send(ActorCommand::Close {
                host: self.shared.id,
            });
        }
    }

    #[must_use]
    pub fn browser_process_id(&self) -> u32 {
        self.shared.browser_process_id.get()
    }

    #[must_use]
    pub fn dpi_ownership(&self) -> Option<WebDpiOwnership> {
        Some(WebDpiOwnership {
            detects_monitor_scale_changes: false,
            rasterization_scale: self.shared.scale.get(),
            bounds_mode_is_raw_pixels: true,
        })
    }
}

impl Drop for WebHost {
    fn drop(&mut self) {
        self.close();
    }
}

pub fn forget_web_environment() {
    actor::forget_web_environment();
}

#[must_use]
pub fn web_environment_epoch() -> u64 {
    actor::web_environment_epoch()
}

pub struct SpareParent(std::convert::Infallible);

impl SpareParent {
    #[must_use]
    pub fn window(&self) -> NativeWindow {
        match self.0 {}
    }

    #[must_use]
    pub fn compositor(&self) -> &Compositor {
        match self.0 {}
    }

    #[must_use]
    pub fn is_window(&self) -> bool {
        match self.0 {}
    }
}

pub fn spare_parent(
    token: WaitToken<'_, doors::CompositorBirth>,
) -> Result<Option<SpareParent>, String> {
    let _ = token;
    Ok(None)
}

pub fn warm_web_environment(
    folder: &Path,
    answered: EnvironmentAnswer,
) -> Result<WebWarmUp, String> {
    let _ = (folder, answered);
    Ok(WebWarmUp::NothingToWarm)
}

pub fn webview2_runtime_version() -> Result<String, String> {
    actor::browser_version()
}

pub(crate) fn shutdown_actor() {
    actor::shutdown();
}
