//! **The page host, where there is no engine yet** — see the note on the
//! `portable` module declaration at the end of `webview.rs`.
//!
//! The twelve data types the window and the engine speak in live in
//! `webview.rs` and are compiled on every platform; this is the thirteenth,
//! and it is the only one that has anything to do with WebView2. M4-2 replaces
//! it with `WKWebView` and the two delegates X-2 measured.

use std::path::Path;

use super::{
    PageVisual, RehostCompensation, RehostOutcome, RehostSide, RehostStep, WebChord, WebEvent,
    WebGuards, WebInstallReport, WebMouseEvent, WebNavigationVerdict, WebRequestVerdict,
};
use crate::Compositor;

/// What every door of this host answers with.
fn no_engine(what: &str) -> String {
    format!("{what} is not on this platform yet")
}

/// **A web seat's engine, before there is one.**
///
/// Constructible and empty: `bt_app::webhost::WebSeat::open` builds one, hands
/// it three closures and then asks it for an environment, and it is *that* ask
/// that refuses. The three closures are held rather than dropped because they
/// are the seat's own policy — the navigation gate, the request gate and the
/// wake — and M4-2 calls all three from a `WKNavigationDelegate`; a host that
/// threw them away would be a different type by then.
pub struct WebHost {
    #[expect(
        dead_code,
        reason = "M4-2 calls it from -webView:decidePolicyForNavigationAction:"
    )]
    gate: Box<dyn Fn(&str) -> WebNavigationVerdict>,
    #[expect(
        dead_code,
        reason = "M4-2 compiles it into a WKContentRuleList — see probe X-2"
    )]
    request_gate: Box<dyn Fn(&str) -> WebRequestVerdict>,
    #[expect(dead_code, reason = "M4-2 wakes the loop when a delegate answers")]
    wake: Box<dyn Fn()>,
}

impl WebHost {
    /// Build the host. Cannot fail: the return type is `Self`, and the seat
    /// holds one whether or not a page ever opens in it.
    #[must_use]
    pub fn new(
        gate: Box<dyn Fn(&str) -> WebNavigationVerdict>,
        request_gate: Box<dyn Fn(&str) -> WebRequestVerdict>,
        wake: Box<dyn Fn()>,
    ) -> Self {
        Self {
            gate,
            request_gate,
            wake,
        }
    }

    /// Everything the engine has said since the last drain. Nothing, ever —
    /// there is no engine to say anything, and the seat's state machine reads
    /// an empty drain as "no news", which is true.
    #[must_use]
    pub fn drain(&self) -> Vec<WebEvent> {
        Vec::new()
    }

    /// The chords this window keeps from a focused page. Remembered by nobody,
    /// because no page is ever focused.
    pub fn set_claimed_chords(&self, chords: Vec<WebChord>) {
        let _ = chords;
    }

    /// Whether a controller is in service. Never.
    #[must_use]
    pub fn has_controller(&self) -> bool {
        false
    }

    /// **The refusal, and the one place it is said.**
    ///
    /// The first thing a seat asks for and the last thing that happens to it:
    /// `bt_app::webhost` turns the `Err` into the seat's fault state, which is
    /// a banner in the pane naming the reason rather than an empty rectangle.
    pub fn request_environment(&mut self, folder: &Path, generation: u64) -> Result<(), String> {
        let _ = (folder, generation);
        Err(no_engine("the web preview"))
    }

    /// Ask for a controller on this window. Unreachable: no environment.
    pub fn request_controller(
        &mut self,
        window: crate::NativeWindow,
        generation: u64,
    ) -> Result<(), String> {
        let _ = (window, generation);
        Err(no_engine("the web preview"))
    }

    /// Take the controller into service. Unreachable: no controller.
    pub fn install(
        &mut self,
        compositor: &Compositor,
        page: PageVisual,
        generation: u64,
    ) -> Result<WebInstallReport, String> {
        let _ = (compositor, page, generation);
        Err(no_engine("the web preview"))
    }

    /// Hand a live page from one window to another. Unreachable: no page.
    ///
    /// `KeptSource` and not `Lost`, because the two mean different things to
    /// the tear-out that asked: `Lost` says the page is gone and the tab may
    /// move anyway, and `KeptSource` says nothing moved — which is the truth.
    pub fn rehost(
        &mut self,
        from: &RehostSide<'_>,
        to: &RehostSide<'_>,
        rect: (i32, i32, u32, u32),
        visible: bool,
    ) -> RehostOutcome {
        let _ = (from, to, rect, visible);
        RehostOutcome::KeptSource {
            failed_at: RehostStep::Hide,
            error: no_engine("the web preview"),
            compensation: RehostCompensation::default(),
        }
    }

    /// Where the seat is. Nothing to tell.
    pub fn set_bounds(&self, x: i32, y: i32, width: u32, height: u32) -> Result<(), String> {
        let _ = (x, y, width, height);
        Ok(())
    }

    /// The scale a page rasterizes at. Nothing to tell.
    pub fn set_rasterization_scale(&self, scale: f64) -> Result<(), String> {
        let _ = scale;
        Ok(())
    }

    /// The window moved. Nothing to tell.
    pub fn notify_parent_window_moved(&self) -> Result<(), String> {
        Ok(())
    }

    /// On the glass, or off it. Nothing to show either way.
    pub fn set_visible(&self, visible: bool) -> Result<(), String> {
        let _ = visible;
        Ok(())
    }

    /// Go to an address. Unreachable: the seat has no engine and says so.
    pub fn navigate(&self, url: &str) -> Result<(), String> {
        let _ = url;
        Err(no_engine("the web preview"))
    }

    /// Read the page again. Unreachable.
    pub fn reload(&self) -> Result<(), String> {
        Err(no_engine("the web preview"))
    }

    /// Stop reading. There is nothing in flight.
    pub fn stop(&self) -> Result<(), String> {
        Ok(())
    }

    /// Back through the history. Unreachable.
    pub fn go_back(&self) -> Result<(), String> {
        Err(no_engine("the web preview"))
    }

    /// Forward through the history. Unreachable.
    pub fn go_forward(&self) -> Result<(), String> {
        Err(no_engine("the web preview"))
    }

    /// The engine's own inspector. Unreachable.
    pub fn open_dev_tools(&self) -> Result<(), String> {
        Err(no_engine("the web preview"))
    }

    /// The zoom the page is at. One, which is what a page nobody has zoomed is
    /// at — the row reads *100%* rather than blank.
    #[must_use]
    pub fn zoom(&self) -> f64 {
        1.0
    }

    /// Set the zoom. Unreachable.
    pub fn set_zoom(&self, factor: f64) -> Result<(), String> {
        let _ = factor;
        Err(no_engine("the web preview"))
    }

    /// Find in page. Unreachable.
    pub fn find(&self, term: &str, case_sensitive: bool) -> Result<(), String> {
        let _ = (term, case_sensitive);
        Err(no_engine("the web preview"))
    }

    /// The next match. Unreachable.
    pub fn find_step(&self, forwards: bool) -> Result<(), String> {
        let _ = forwards;
        Err(no_engine("the web preview"))
    }

    /// Put the find away. Nothing to put away.
    pub fn find_stop(&self) -> Result<(), String> {
        Ok(())
    }

    /// Give the page the keyboard. Unreachable.
    pub fn focus_page(&self) -> Result<(), String> {
        Err(no_engine("the web preview"))
    }

    /// The pointer, over a page that is not there.
    pub fn send_mouse(
        &self,
        event: WebMouseEvent,
        point: (i32, i32),
        buttons_down: u32,
    ) -> Result<(), String> {
        let _ = (event, point, buttons_down);
        Ok(())
    }

    /// A picture of the page, for the focus card. Unreachable.
    pub fn capture_preview(&self) -> Result<(), String> {
        Err(no_engine("the web preview"))
    }

    /// The page's icon. Unreachable.
    pub fn get_favicon(&self) -> Result<(), String> {
        Err(no_engine("the web preview"))
    }

    /// Close a controller nobody came for. There is none.
    pub fn close_pending_controller(&mut self) {}

    /// Close the host. Nothing to close, and it must not refuse: this runs on
    /// the way out of a seat and on the way out of the process.
    pub fn close(&mut self) {}

    /// What this seat may still promise about a page. Nothing stands, because
    /// nothing was installed — which is what `WebGuards::none` means, and it is
    /// the same value the Windows arm reports for a controller that never came.
    #[must_use]
    pub fn guards(&self) -> WebGuards {
        WebGuards::none()
    }
}

/// Drop the process-wide environment. There is none to drop.
///
/// `WKWebsiteDataStore`'s lifecycle is a different question with a different
/// answer and is M2-6's and M4-2's, not this door's — see §4.5 of the plan.
pub fn forget_web_environment() {}

/// **Which engine is installed.**
///
/// On Windows this is *is the Evergreen runtime here at all*, and the honest
/// answer on a platform whose web engine ships with the operating system would
/// be `Ok`. It is an `Err` today because the question a caller asks with it is
/// not really "is WebKit present" but "can this build show a page", and this
/// build cannot. M4-2 makes it `Ok` and takes the pages with it.
pub fn webview2_runtime_version() -> Result<String, String> {
    Err(no_engine("the web preview"))
}
