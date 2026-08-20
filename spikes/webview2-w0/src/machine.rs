//! §4 of the plan, written as code so gate 10 can shoot at it.
//!
//! The whole point of the generation token is that WebView2's creation callbacks
//! are asynchronous and *cannot be cancelled*. A pane that was closed, or whose
//! browser process died and was rebuilt, will still receive the environment and
//! controller callbacks of the attempt it abandoned. Every one of those arrives
//! carrying the generation it was launched under, and a generation that is no
//! longer current is dropped on the floor — including its controller, which is
//! closed rather than adopted.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Uninitialized,
    EnvironmentPending,
    ControllerPending,
    /// Events and policies are all installed; navigation is permitted.
    Ready,
    Closing,
    Failed,
}

/// What the caller must do with a callback that arrived.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    /// Nothing: a stale callback, dropped.
    Ignore,
    /// Create the controller for this generation.
    CreateController,
    /// Install every event and policy for this generation, then report back.
    InstallEvents,
    /// Navigate. Only ever produced after `InstallEvents` was acknowledged.
    Navigate(String),
    /// A controller from an abandoned generation arrived: close it, do not keep
    /// it. Leaking it would leave a live browser process nobody points at.
    CloseOrphanController,
    /// The browser process is gone; build the whole thing again.
    RebuildFromScratch,
    /// Only the renderer died; the controller is still good.
    // Plan §4's "renderer crash Reload" clause; proven by its own test below
    // and otherwise never constructed outside it.
    #[cfg_attr(not(test), allow(dead_code))]
    Reload,
    /// Wait for `BrowserProcessExited` before touching the user data folder.
    AwaitBrowserExitBeforeCleanup,
}

pub struct Preview {
    state: State,
    generation: u64,
    /// Last write wins. Storing a URL is not navigating to it.
    desired_url: Option<String>,
    /// The URL a session file may record. Only a *successful* top-level
    /// navigation writes here.
    recoverable_url: Option<String>,
    events_installed: bool,
}

impl Default for Preview {
    fn default() -> Self {
        Self::new()
    }
}

impl Preview {
    pub fn new() -> Self {
        Self {
            state: State::Uninitialized,
            generation: 0,
            desired_url: None,
            recoverable_url: None,
            events_installed: false,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn recoverable_url(&self) -> Option<&str> {
        self.recoverable_url.as_deref()
    }

    pub fn desired_url(&self) -> Option<&str> {
        self.desired_url.as_deref()
    }

    /// Somebody asked for a URL. If the engine is not up yet this only records
    /// the wish; if it is, it navigates.
    pub fn request(&mut self, url: &str) -> Effect {
        self.desired_url = Some(url.to_owned());
        match self.state {
            State::Ready if self.events_installed => Effect::Navigate(url.to_owned()),
            State::Uninitialized | State::Failed => {
                self.generation += 1;
                self.state = State::EnvironmentPending;
                self.events_installed = false;
                Effect::Ignore
            }
            _ => Effect::Ignore,
        }
    }

    /// The environment callback for `generation` came back.
    pub fn on_environment(&mut self, generation: u64, ok: bool) -> Effect {
        if generation != self.generation || self.state != State::EnvironmentPending {
            return Effect::Ignore;
        }
        if !ok {
            self.state = State::Failed;
            return Effect::Ignore;
        }
        self.state = State::ControllerPending;
        Effect::CreateController
    }

    /// The controller callback for `generation` came back.
    pub fn on_controller(&mut self, generation: u64, ok: bool) -> Effect {
        if generation != self.generation || self.state != State::ControllerPending {
            // The controller is real and running even though nobody wants it.
            return if ok {
                Effect::CloseOrphanController
            } else {
                Effect::Ignore
            };
        }
        if !ok {
            self.state = State::Failed;
            return Effect::Ignore;
        }
        Effect::InstallEvents
    }

    /// Every handler and policy for `generation` is attached.
    pub fn on_events_installed(&mut self, generation: u64) -> Effect {
        if generation != self.generation || self.state != State::ControllerPending {
            return Effect::Ignore;
        }
        self.state = State::Ready;
        self.events_installed = true;
        match self.desired_url.clone() {
            Some(url) => Effect::Navigate(url),
            None => Effect::Ignore,
        }
    }

    /// A top-level navigation finished. Only success moves the recoverable URL —
    /// an error page, a cancelled navigation and `about:blank` all leave the
    /// last good URL where it was, so a crash during a failed load still
    /// restores what the person was looking at.
    pub fn on_navigation_completed(&mut self, generation: u64, url: &str, success: bool) -> Effect {
        if generation != self.generation || self.state != State::Ready {
            return Effect::Ignore;
        }
        if success && !is_blank(url) {
            self.recoverable_url = Some(url.to_owned());
        }
        Effect::Ignore
    }

    /// The browser process died. Everything is invalid; a new generation starts.
    pub fn on_browser_process_failed(&mut self) -> Effect {
        self.generation += 1;
        self.state = State::EnvironmentPending;
        self.events_installed = false;
        // Whatever was last good is what comes back up.
        self.desired_url = self
            .recoverable_url
            .clone()
            .or_else(|| self.desired_url.clone());
        Effect::RebuildFromScratch
    }

    /// Only the render process died. The controller survives.
    ///
    /// Plan §4's "renderer crash Reload" clause; proven by its own test below
    /// and otherwise never called outside it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn on_render_process_failed(&mut self) -> Effect {
        if self.state == State::Ready {
            Effect::Reload
        } else {
            Effect::Ignore
        }
    }

    /// The pane is going away.
    pub fn close(&mut self) -> Effect {
        self.generation += 1;
        self.state = State::Closing;
        self.events_installed = false;
        Effect::AwaitBrowserExitBeforeCleanup
    }
}

fn is_blank(url: &str) -> bool {
    url.eq_ignore_ascii_case("about:blank") || url.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boot(preview: &mut Preview, url: &str) {
        assert_eq!(preview.request(url), Effect::Ignore);
        let generation = preview.generation();
        assert_eq!(
            preview.on_environment(generation, true),
            Effect::CreateController
        );
        assert_eq!(
            preview.on_controller(generation, true),
            Effect::InstallEvents
        );
        assert_eq!(
            preview.on_events_installed(generation),
            Effect::Navigate(url.to_owned())
        );
        assert_eq!(preview.state(), State::Ready);
    }

    #[test]
    fn nothing_navigates_before_every_handler_is_on() {
        let mut preview = Preview::new();
        preview.request("https://example.com/");
        let generation = preview.generation();
        preview.on_environment(generation, true);
        // The controller exists here — and the state machine still refuses to
        // navigate, because a navigation started now would run before
        // NavigationStarting was attached.
        assert_eq!(
            preview.on_controller(generation, true),
            Effect::InstallEvents
        );
        assert_eq!(preview.state(), State::ControllerPending);
        assert_eq!(
            preview.on_events_installed(generation),
            Effect::Navigate("https://example.com/".into())
        );
    }

    #[test]
    fn a_late_environment_callback_from_a_closed_pane_is_dropped() {
        let mut preview = Preview::new();
        preview.request("https://example.com/");
        let stale = preview.generation();
        preview.close();
        assert_eq!(preview.on_environment(stale, true), Effect::Ignore);
    }

    #[test]
    fn a_late_controller_from_an_abandoned_generation_is_closed_not_adopted() {
        let mut preview = Preview::new();
        preview.request("https://example.com/");
        let stale = preview.generation();
        preview.on_environment(stale, true);
        // The browser dies before the controller callback lands.
        assert_eq!(
            preview.on_browser_process_failed(),
            Effect::RebuildFromScratch
        );
        assert_ne!(preview.generation(), stale);
        assert_eq!(
            preview.on_controller(stale, true),
            Effect::CloseOrphanController
        );
        // …and the new generation is unharmed by it.
        let current = preview.generation();
        assert_eq!(
            preview.on_environment(current, true),
            Effect::CreateController
        );
    }

    #[test]
    fn desired_url_is_last_write_wins_and_only_one_navigation_results() {
        let mut preview = Preview::new();
        preview.request("https://first.example/");
        let generation = preview.generation();
        preview.request("https://second.example/");
        preview.request("https://third.example/");
        preview.on_environment(generation, true);
        preview.on_controller(generation, true);
        assert_eq!(
            preview.on_events_installed(generation),
            Effect::Navigate("https://third.example/".into())
        );
    }

    #[test]
    fn a_failed_page_does_not_overwrite_a_recoverable_url() {
        let mut preview = Preview::new();
        boot(&mut preview, "https://good.example/");
        let generation = preview.generation();
        preview.on_navigation_completed(generation, "https://good.example/", true);
        assert_eq!(preview.recoverable_url(), Some("https://good.example/"));
        preview.on_navigation_completed(generation, "https://dead.example/", false);
        assert_eq!(preview.recoverable_url(), Some("https://good.example/"));
        preview.on_navigation_completed(generation, "about:blank", true);
        assert_eq!(preview.recoverable_url(), Some("https://good.example/"));
    }

    #[test]
    fn a_browser_crash_comes_back_to_the_last_good_url() {
        let mut preview = Preview::new();
        boot(&mut preview, "https://good.example/");
        let generation = preview.generation();
        preview.on_navigation_completed(generation, "https://good.example/", true);
        preview.request("https://broken.example/");
        preview.on_navigation_completed(generation, "https://broken.example/", false);
        assert_eq!(
            preview.on_browser_process_failed(),
            Effect::RebuildFromScratch
        );
        let generation = preview.generation();
        preview.on_environment(generation, true);
        preview.on_controller(generation, true);
        assert_eq!(
            preview.on_events_installed(generation),
            Effect::Navigate("https://good.example/".into())
        );
    }

    #[test]
    fn a_renderer_crash_only_reloads() {
        let mut preview = Preview::new();
        boot(&mut preview, "https://good.example/");
        let before = preview.generation();
        assert_eq!(preview.on_render_process_failed(), Effect::Reload);
        assert_eq!(preview.generation(), before);
        assert_eq!(preview.state(), State::Ready);
    }

    #[test]
    fn closing_waits_for_the_browser_to_exit_before_the_udf_is_touched() {
        let mut preview = Preview::new();
        boot(&mut preview, "https://good.example/");
        assert_eq!(preview.close(), Effect::AwaitBrowserExitBeforeCleanup);
        assert_eq!(preview.state(), State::Closing);
    }

    #[test]
    fn a_navigation_completed_from_a_stale_generation_cannot_write_the_recoverable_url() {
        let mut preview = Preview::new();
        boot(&mut preview, "https://good.example/");
        let stale = preview.generation();
        preview.on_browser_process_failed();
        preview.on_navigation_completed(stale, "https://attacker.example/", true);
        assert_eq!(preview.recoverable_url(), None);
    }
}
